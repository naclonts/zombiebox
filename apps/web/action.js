(() => {
  "use strict";

  const SESSION_KEY = "zb_action_session";
  const U64_MAX = 18446744073709551615n;

  const FLAG_FIRE_PULSE = 1 << 0;
  const FLAG_FIRE_HOLD = 1 << 1;
  const FLAG_INTERACT = 1 << 2;
  const FLAG_RELOAD = 1 << 3;
  const FLAG_HEAL = 1 << 4;
  const FLAG_LOOK_AHEAD = 1 << 5;

  // Camera modes that integrate the right stick/mouse as a look control.
  const CAMERA_CHASE = 3;
  const CAMERA_FIRST_PERSON = 4;
  const STICK_RADIUS = 56;
  const TOUCH_LOOK_DEADZONE = 0.08;
  // Direct mouse-look sensitivity (radians of view rotation per CSS pixel).
  const MOUSE_LOOK_RADIANS_PER_PX = 0.0025;
  const MOUSE_LOOK_MAX_RADIANS = 1.2;
  let pointerLockPending = false;
  let pointerLockUnavailable = false;

  const STATUS_WON = 1 << 0;
  const STATUS_DEAD = 1 << 1;
  const STATUS_RELOADING = 1 << 2;
  const STATUS_CAN_RELOAD = 1 << 3;
  const STATUS_CAN_HEAL = 1 << 4;
  const STATUS_LOW_AMMO = 1 << 5;
  const STATUS_DANGER_CLOSE = 1 << 7;
  // Set only after a Read interaction succeeds (the prompt itself must not
  // steal focus or pause play).
  const STATUS_READ_OPEN = 1 << 8;

  const DISTRICTS = [
    "BIRCH STREET",
    "RESIDENTIAL",
    "DOWNTOWN",
    "HOSPITAL",
    "POLICE PRECINCT",
    "ARMY BASE",
    "COUNTRYSIDE",
    "WOODS",
  ];

  const WEAPON_TITLES = ["AXE", "PISTOL", "SHOTGUN", "ROCKET"];

  const $ = (id) => document.getElementById(id);

  const canvas = $("action-canvas");
  const ctx = canvas.getContext("2d", { alpha: false });

  const hudFloor = $("hud-floor");
  const hudHealthBar = $("hud-health-bar");
  const healthSegs = Array.from(hudHealthBar.querySelectorAll(".health-seg"));
  const hudWeaponTitle = $("hud-weapon-title");
  const hudWeaponReadout = $("hud-weapon-readout");
  const hudEyebrow = $("hud-eyebrow");
  const hudObjective = $("hud-objective");
  const hudCaches = $("hud-caches");
  const hudThreats = $("hud-threats");
  const hudFps = $("hud-fps");
  const rideReadout = $("ride-readout");
  const rideSpeed = $("ride-speed");
  const rideCondition = $("ride-condition");
  const rideGrip = $("ride-grip");

  const btnAtlas = $("btn-atlas");
  const btnLook = $("btn-look");
  const btnCamera = $("btn-camera");
  const btnSound = $("btn-sound");
  const btnPause = $("btn-pause");
  const btnInteract = $("btn-interact");
  const btnHeal = $("btn-heal");
  const btnReload = $("btn-reload");
  const slotButtons = [0, 1, 2, 3].map((i) => $("slot-" + i));

  const toastEl = $("action-toast");
  const overlayEl = $("action-overlay");
  const modalEyebrow = $("modal-eyebrow");
  const modalTitle = $("modal-title");
  const modalCopy = $("modal-copy");
  const setupHeading = $("setup-heading");
  const scenarioButtons = Array.from(document.querySelectorAll(".scenario-btn"));
  const seedInput = $("seed-input");
  const btnRandomSeed = $("btn-random-seed");
  const seedError = $("seed-error");
  const btnPrimary = $("btn-primary");
  const btnNewRun = $("btn-new-run");
  const btnModalAtlas = $("btn-modal-atlas");

  const stickMoveEl = $("stick-move");
  const stickMoveKnob = $("stick-move-knob");
  const stickAimEl = $("stick-aim");
  const stickAimKnob = $("stick-aim-knob");

  const state = {
    runId: null,
    runSeed: "42",
    runMode: "county",
    started: false,
    modalKind: "start", // start | paused | won | dead | error
    setupMode: "county",
    setupSeed: "42",
    playing: false,
    inFlight: false,
    lookAhead: true,
    cameraMode: 0,
    muted: false,
    lastTNs: 0,
    toastTimer: 0,
    fpsWindowStart: 0,
    fpsFrames: 0,
    hud: null,
    playerX: 0,
    playerY: 0,
    playerFloor: 0,
    dead: false,
    controlMode: 0,
    mountedIsometric: false,
    readId: 0,
  };

  const input = {
    keysDown: new Set(),
    shiftHeld: false,
    mouseHold: false,
    spaceHold: false,
    mouseAimX: 0,
    mouseAimY: 0,
    mouseClientX: 0,
    mouseClientY: 0,
    hasMouseAim: false,
    mouseLookYaw: 0,
    mouseLookPitch: 0,
    mouseTracking: false,
    mouseLastClientX: 0,
    mouseLastClientY: 0,
    pendingFlags: 0,
    pendingWeapon: -1,
    touchMove: {
      id: null,
      ox: 0,
      oy: 0,
      mx: 0,
      my: 0,
    },
    touchAim: {
      id: null,
      ox: 0,
      oy: 0,
      ax: 0,
      ay: 0,
      hasAim: false,
      lookX: 0,
      lookY: 0,
      fireHold: false,
      downAt: 0,
      maxDist: 0,
    },
  };


  // --- Audio Synthesis ---
  let audioCtx = null;
  let noiseBuffer = null;
  let engineOsc = null;
  let engineGain = null;

  function ensureAudio() {
    if (state.muted) return null;
    const Ctor = window.AudioContext || window.webkitAudioContext;
    if (!Ctor) return null;
    if (!audioCtx) {
      audioCtx = new Ctor();
      const length = Math.floor(audioCtx.sampleRate * 0.5);
      noiseBuffer = audioCtx.createBuffer(1, length, audioCtx.sampleRate);
      const data = noiseBuffer.getChannelData(0);
      for (let i = 0; i < length; i++) {
        data[i] = Math.random() * 2 - 1;
      }
    }
    if (audioCtx.state === "suspended") {
      audioCtx.resume().catch(() => {});
    }
    return audioCtx;
  }

  function playTone(ac, { type = "triangle", f0, f1, dur, gain = 0.12, delay = 0 }) {
    const t0 = ac.currentTime + delay;
    const osc = ac.createOscillator();
    const g = ac.createGain();
    osc.type = type;
    osc.frequency.setValueAtTime(Math.max(20, f0), t0);
    osc.frequency.exponentialRampToValueAtTime(Math.max(20, f1), t0 + dur);
    g.gain.setValueAtTime(gain, t0);
    g.gain.exponentialRampToValueAtTime(0.001, t0 + dur);
    osc.connect(g).connect(ac.destination);
    osc.start(t0);
    osc.stop(t0 + dur + 0.01);
  }

  function playNoise(ac, { filterType = "bandpass", freq, q = 1, dur, gain = 0.14, delay = 0 }) {
    if (!noiseBuffer) return;
    const t0 = ac.currentTime + delay;
    const src = ac.createBufferSource();
    src.buffer = noiseBuffer;
    const filter = ac.createBiquadFilter();
    filter.type = filterType;
    filter.frequency.setValueAtTime(freq, t0);
    filter.Q.setValueAtTime(q, t0);
    const g = ac.createGain();
    g.gain.setValueAtTime(gain, t0);
    g.gain.exponentialRampToValueAtTime(0.001, t0 + dur);
    src.connect(filter).connect(g).connect(ac.destination);
    src.start(t0);
    src.stop(t0 + dur + 0.01);
  }

  function playSfxMask(lo) {
    if (!lo || state.muted) return;
    const ac = audioCtx;
    if (!ac || ac.state !== "running") return;
    const has = (bit) => ((lo >>> bit) & 1) !== 0;

    if (has(0)) {
      playTone(ac, { type: "triangle", f0: 290, f1: 95, dur: 0.08, gain: 0.09 });
      playNoise(ac, { filterType: "bandpass", freq: 900, q: 1.2, dur: 0.07, gain: 0.07 });
    }
    if (has(1)) {
      playTone(ac, { type: "sawtooth", f0: 150, f1: 52, dur: 0.09, gain: 0.12 });
      playNoise(ac, { filterType: "lowpass", freq: 700, dur: 0.07, gain: 0.1 });
    }
    if (has(3)) {
      playNoise(ac, { filterType: "bandpass", freq: 1650, q: 0.8, dur: 0.09, gain: 0.16 });
      playTone(ac, { type: "triangle", f0: 340, f1: 68, dur: 0.1, gain: 0.14 });
    }
    if (has(4)) {
      playNoise(ac, { filterType: "lowpass", freq: 1200, dur: 0.21, gain: 0.22 });
      playTone(ac, { type: "sawtooth", f0: 195, f1: 40, dur: 0.18, gain: 0.16 });
    }
    if (has(5)) {
      playNoise(ac, { filterType: "bandpass", freq: 620, q: 0.7, dur: 0.26, gain: 0.18 });
      playTone(ac, { type: "sawtooth", f0: 170, f1: 75, dur: 0.24, gain: 0.13 });
    }
    if (has(6)) {
      playNoise(ac, { filterType: "lowpass", freq: 520, dur: 0.38, gain: 0.28 });
      playTone(ac, { type: "sawtooth", f0: 105, f1: 24, dur: 0.36, gain: 0.22 });
    }
    if (has(7)) {
      playTone(ac, { type: "square", f0: 880, f1: 420, dur: 0.03, gain: 0.07 });
    }
    if (has(8)) {
      playTone(ac, { type: "triangle", f0: 420, f1: 620, dur: 0.055, gain: 0.08 });
    }
    if (has(9)) {
      playTone(ac, { type: "triangle", f0: 520, f1: 560, dur: 0.045, gain: 0.08 });
      playTone(ac, { type: "triangle", f0: 760, f1: 820, dur: 0.06, gain: 0.09, delay: 0.045 });
    }
    if (has(10)) {
      playNoise(ac, { filterType: "highpass", freq: 2400, dur: 0.14, gain: 0.13 });
      playTone(ac, { type: "sine", f0: 1900, f1: 2500, dur: 0.06, gain: 0.06 });
    }
    if (has(14)) {
      playTone(ac, { type: "triangle", f0: 185, f1: 118, dur: 0.075, gain: 0.08 });
    }
    if (has(16)) {
      playTone(ac, { type: "triangle", f0: 155, f1: 82, dur: 0.045, gain: 0.06 });
    }
    if (has(17)) {
      playTone(ac, { type: "triangle", f0: 440, f1: 440, dur: 0.06, gain: 0.09 });
      playTone(ac, { type: "triangle", f0: 660, f1: 660, dur: 0.09, gain: 0.1, delay: 0.06 });
    }
    if (has(18)) {
      playTone(ac, { type: "sine", f0: 392, f1: 392, dur: 0.07, gain: 0.09 });
      playTone(ac, { type: "sine", f0: 523, f1: 523, dur: 0.07, gain: 0.09, delay: 0.06 });
      playTone(ac, { type: "sine", f0: 659, f1: 659, dur: 0.11, gain: 0.1, delay: 0.12 });
    }
    if (has(20)) {
      playTone(ac, { type: "sawtooth", f0: 115, f1: 168, dur: 0.12, gain: 0.07 });
    }
    if (has(22)) {
      playTone(ac, { type: "triangle", f0: 160, f1: 48, dur: 0.13, gain: 0.11 });
    }
    if (has(25)) {
      playTone(ac, { type: "sawtooth", f0: 165, f1: 58, dur: 0.16, gain: 0.15 });
      playNoise(ac, { filterType: "lowpass", freq: 650, dur: 0.11, gain: 0.11 });
    }
    if (has(26)) {
      [440, 554, 659, 880].forEach((f, idx) => {
        playTone(ac, { type: "triangle", f0: f, f1: f, dur: 0.12, gain: 0.1, delay: idx * 0.09 });
      });
    }
    if (has(27)) {
      [260, 196, 130].forEach((f, idx) => {
        playTone(ac, { type: "sawtooth", f0: f, f1: f * 0.9, dur: 0.18, gain: 0.1, delay: idx * 0.14 });
      });
    }
    if (has(29)) {
      playNoise(ac, { filterType: "lowpass", freq: 520, dur: 0.28, gain: 0.25 });
      playTone(ac, { type: "sawtooth", f0: 120, f1: 34, dur: 0.3, gain: 0.2 });
    }
    if (has(30)) {
      playNoise(ac, { filterType: "lowpass", freq: 1100, dur: 0.12, gain: 0.18 });
      playTone(ac, { type: "triangle", f0: 210, f1: 70, dur: 0.12, gain: 0.15 });
    }
    if (has(31)) {
      playNoise(ac, { filterType: "bandpass", freq: 340, q: 0.5, dur: 0.24, gain: 0.1 });
    }
  }

  function syncEngineLoop(hud) {
    const mounted = hud.controlKind === 1 && !state.muted && audioCtx?.state === "running";
    if (!mounted) {
      if (engineGain && audioCtx) {
        engineGain.gain.cancelScheduledValues(audioCtx.currentTime);
        engineGain.gain.setTargetAtTime(0, audioCtx.currentTime, 0.04);
      }
      if (engineOsc) {
        const old = engineOsc;
        engineOsc = null;
        setTimeout(() => { try { old.stop(); old.disconnect(); } catch {} }, 180);
      }
      engineGain = null;
      return;
    }
    if (!engineOsc) {
      engineOsc = audioCtx.createOscillator();
      engineGain = audioCtx.createGain();
      engineOsc.type = "sawtooth";
      engineGain.gain.value = 0;
      engineOsc.connect(engineGain).connect(audioCtx.destination);
      engineOsc.start();
    }
    engineOsc.frequency.setTargetAtTime(42 + hud.speedMphX10 * 0.16 + hud.engineLoad * 0.42, audioCtx.currentTime, 0.08);
    engineGain.gain.setTargetAtTime(0.012 + hud.engineLoad * 0.00022, audioCtx.currentTime, 0.08);
  }

  // --- Utilities ---
  function validateSeed(raw) {
    const s = String(raw ?? "").trim();
    if (!/^\d+$/.test(s)) return null;
    try {
      const val = BigInt(s);
      if (val >= 0n && val <= U64_MAX) return s;
    } catch {
      return null;
    }
    return null;
  }

  function randomU64Seed() {
    if (window.crypto && crypto.getRandomValues) {
      const arr = new Uint32Array(2);
      crypto.getRandomValues(arr);
      return ((BigInt(arr[0]) << 32n) | BigInt(arr[1])).toString();
    }
    return String(Math.floor(Math.random() * 1_000_000_000) + 1);
  }

  function saveSession() {
    if (!state.runId) return;
    try {
      sessionStorage.setItem(
        SESSION_KEY,
        JSON.stringify({
          id: state.runId,
          seed: state.runSeed,
          mode: state.runMode,
          started: state.started,
        }),
      );
    } catch {
      // Ignore storage quota / private mode failures.
    }
  }

  function loadSession() {
    try {
      const raw = sessionStorage.getItem(SESSION_KEY);
      if (!raw) return null;
      const parsed = JSON.parse(raw);
      if (!parsed || !parsed.id) return null;
      const seed = validateSeed(parsed.seed) ?? "42";
      const mode = parsed.mode === "birch" ? "birch" : "county";
      return {
        id: String(parsed.id),
        seed,
        mode,
        started: Boolean(parsed.started),
      };
    } catch {
      return null;
    }
  }

  function showToast(msg, durationMs = 2600) {
    clearTimeout(state.toastTimer);
    toastEl.textContent = msg;
    toastEl.hidden = false;
    state.toastTimer = window.setTimeout(() => {
      toastEl.hidden = true;
    }, durationMs);
  }

  function computeRenderSize() {
    const rect = canvas.getBoundingClientRect();
    const rw = rect.width > 0 ? rect.width : window.innerWidth;
    const rh = rect.height > 0 ? rect.height : window.innerHeight;
    const aspect = Math.max(0.5, Math.min(3.0, rw / Math.max(1, rh)));
    let h = 360;
    let w = Math.round(h * aspect);
    if (w > 640) {
      w = 640;
      h = Math.round(w / aspect);
    }
    w = Math.max(320, Math.min(640, w)) & ~1;
    h = Math.max(180, Math.min(360, h)) & ~1;
    return { w, h };
  }

  function getMonotonicNs() {
    const nowNs = Math.round(performance.now() * 1e6);
    state.lastTNs = Math.max(state.lastTNs + 1, nowNs);
    return state.lastTNs;
  }

  function resetFpsWindow() {
    state.fpsWindowStart = 0;
    state.fpsFrames = 0;
  }

  // Measured from frames that were actually drawn from the host, so network or
  // decode stalls are reflected in the readout.
  function recordPresentedFrame() {
    const now = performance.now();
    if (state.fpsWindowStart === 0) {
      state.fpsWindowStart = now;
      return;
    }
    const elapsed = now - state.fpsWindowStart;
    if (elapsed > 2000) {
      resetFpsWindow();
      return;
    }
    state.fpsFrames += 1;
    if (elapsed >= 500) {
      hudFps.textContent = `${Math.round((state.fpsFrames * 1000) / elapsed)} FPS`;
      resetFpsWindow();
    }
  }

  function clearHeldInput() {
    input.keysDown.clear();
    input.shiftHeld = false;
    input.mouseHold = false;
    input.spaceHold = false;
    input.pendingFlags = 0;
    input.pendingWeapon = -1;
    input.touchMove.id = null;
    input.touchMove.mx = 0;
    input.touchMove.my = 0;
    input.touchAim.id = null;
    input.touchAim.ax = 0;
    input.touchAim.ay = 0;
    input.touchAim.hasAim = false;
    input.touchAim.fireHold = false;
    input.touchAim.lookX = 0;
    input.touchAim.lookY = 0;
    input.mouseLookYaw = 0;
    input.mouseLookPitch = 0;
    input.mouseTracking = false;
    updateStickVisuals();
  }

  function updateStickVisuals() {
    if (input.touchMove.id !== null) {
      stickMoveEl.hidden = false;
      stickMoveEl.style.left = input.touchMove.ox + "px";
      stickMoveEl.style.top = input.touchMove.oy + "px";
      stickMoveKnob.style.transform = `translate(${input.touchMove.mx * 40}px, ${input.touchMove.my * 40}px)`;
    } else {
      stickMoveEl.hidden = true;
    }

    if (input.touchAim.id !== null) {
      stickAimEl.hidden = false;
      stickAimEl.style.left = input.touchAim.ox + "px";
      stickAimEl.style.top = input.touchAim.oy + "px";
      stickAimEl.classList.toggle("is-firing", input.touchAim.fireHold);
      const kx = input.touchAim.hasAim ? input.touchAim.ax * Math.min(44, Math.max(14, input.touchAim.maxDist * 0.7)) : 0;
      const ky = input.touchAim.hasAim ? input.touchAim.ay * Math.min(44, Math.max(14, input.touchAim.maxDist * 0.7)) : 0;
      stickAimKnob.style.transform = `translate(${kx}px, ${ky}px)`;
    } else {
      stickAimEl.hidden = true;
    }
  }

  // --- HUD & Modal Rendering ---
  function interactLabelFor(kind, floor) {
    switch (kind) {
      case 1:
        return "[E] SEARCH";
      case 2:
        return "[E] OPEN DOOR";
      case 3:
        return "[E] CLOSE DOOR";
      case 4:
        return "[E] CLIMB";
      case 5:
        return `[E] DROP ${Math.max(1, floor)}F`;
      case 6:
        return `[E] STAIRS ${floor + 2}F`;
      case 7:
        return `[E] STAIRS ${Math.max(1, floor)}F`;
      case 8:
        return "[E] MOUNT MOTORCYCLE";
      case 9:
        return "[E] DISMOUNT";
      case 10:
        return "[E] RIGHT MOTORCYCLE";
      case 11:
        return "[E] READ";
      default:
        return "";
    }
  }

  function objectiveTextFor(hud) {
    switch (hud.objectiveKind) {
      case 2:
        return "Block cleared";
      case 3:
        return "Survivor lost";
      case 1:
        return `Clear Birch Street (${hud.zombies} left)`;
      case 4:
        return "Explore county & scavenge supplies";
      case 0:
      default:
        return hud.mode === 1
          ? "Explore county & scavenge supplies"
          : "Scavenge nearby buildings for gear";
    }
  }

  function updateHud(hud) {
    const previousControlMode = state.controlMode;
    state.controlMode = hud.controlKind;
    if (previousControlMode !== state.controlMode) {
      clearHeldInput();
      if (state.controlMode !== 0) releaseMouseLock();
    }
    syncEngineLoop(hud);
    state.hud = hud;
    state.playerX = hud.playerX;
    state.playerY = hud.playerY;
    state.playerFloor = hud.floor;
    state.dead = (hud.statusFlags & STATUS_DEAD) !== 0;

    hudFloor.textContent = `${hud.floor + 1}F`;

    const maxHp = Math.max(1, hud.maxHealth || 6);
    let filled = Math.round((hud.health / maxHp) * healthSegs.length);
    if (hud.health > 0 && filled === 0) filled = 1;
    hudHealthBar.classList.toggle("is-critical", hud.health <= 2);
    hudHealthBar.setAttribute("aria-label", `Health ${hud.health} of ${maxHp}`);
    healthSegs.forEach((seg, idx) => {
      seg.classList.toggle("is-filled", idx < filled);
    });

    const reloading = (hud.statusFlags & STATUS_RELOADING) !== 0;
    const canReload = (hud.statusFlags & STATUS_CAN_RELOAD) !== 0;
    const canHeal = (hud.statusFlags & STATUS_CAN_HEAL) !== 0;
    const lowAmmo = (hud.statusFlags & STATUS_LOW_AMMO) !== 0;
    const dangerClose = (hud.statusFlags & STATUS_DANGER_CLOSE) !== 0;

    if (hud.equippedWeapon === 255) {
      hudWeaponTitle.textContent = "UNARMED";
      hudWeaponReadout.textContent = "Shove";
      hudWeaponReadout.classList.remove("is-low");
    } else if (hud.equippedWeapon === 0) {
      hudWeaponTitle.textContent = "AXE";
      hudWeaponReadout.textContent = "MELEE";
      hudWeaponReadout.classList.remove("is-low");
    } else {
      hudWeaponTitle.textContent = WEAPON_TITLES[hud.equippedWeapon] || "WEAPON";
      hudWeaponReadout.textContent = reloading
        ? `RELOAD ${hud.reloadProgress}%`
        : `${hud.activeMag}/${hud.activeReserve}`;
      hudWeaponReadout.classList.toggle("is-low", lowAmmo && !reloading);
    }

    if (dangerClose) {
      hudEyebrow.textContent = "DANGER CLOSE";
      hudEyebrow.classList.add("is-danger");
    } else if (hud.mode === 0) {
      hudEyebrow.textContent = "BIRCH STREET";
      hudEyebrow.classList.remove("is-danger");
    } else {
      const distName = DISTRICTS[hud.district] || "COUNTY";
      hudEyebrow.textContent = `${distName} · ${hud.playerX},${hud.playerY}`;
      hudEyebrow.classList.remove("is-danger");
    }

    hudObjective.textContent = objectiveTextFor(hud);

    if (hud.mode === 0) {
      hudCaches.hidden = false;
      hudCaches.textContent = `CACHES ${hud.cachesFound}/${hud.cachesTotal}`;
    } else {
      hudCaches.hidden = true;
    }

    hudThreats.textContent =
      hud.mode === 1
        ? `NEARBY: ${hud.zombies}`
        : `INFECTED: ${hud.zombies}/${hud.zombiesTotal}`;

    const riding = hud.controlKind === 1;
    rideReadout.hidden = !riding;
    const labels = ["ISO", "TOP", "ORBIT", "CHASE", "1ST"];
    btnCamera.textContent = `${labels[state.cameraMode]} [C]`;
    btnCamera.setAttribute("aria-label", `Camera view: ${["Isometric", "Overhead", "Orbit", "Chase", "First person"][state.cameraMode]}`);
    if (riding) {
      rideSpeed.textContent = `${(hud.speedMphX10 / 10).toFixed(1)} MPH`;
      rideCondition.textContent = `${hud.conditionPct}%`;
      const slipping = hud.gripUse >= 85;
      rideGrip.textContent = slipping ? "LOW GRIP · EASE OFF" : "GRIP OK";
      rideGrip.classList.toggle("is-slipping", slipping);
      btnInteract.textContent = "[E] DISMOUNT";
    }

    if (hud.controlKind === 1 || hud.interactKind !== 0) {
      btnInteract.hidden = false;
      if (!riding) btnInteract.textContent = interactLabelFor(hud.interactKind, hud.floor);
    } else {
      btnInteract.hidden = true;
    }

    const totals = [
      null,
      hud.pistolMag + hud.pistolReserve,
      hud.shotgunMag + hud.shotgunReserve,
      hud.rocketMag + hud.rocketReserve,
    ];
    slotButtons.forEach((btn, idx) => {
      const owned = (hud.ownedWeaponsMask & (1 << idx)) !== 0;
      const active = hud.equippedWeapon === idx;
      btn.classList.toggle("is-locked", !owned);
      btn.classList.toggle("is-active", active);
      btn.disabled = !owned;
      const ammoEl = btn.querySelector(".slot-ammo");
      ammoEl.textContent = !owned ? "--" : idx === 0 ? "MELEE" : String(totals[idx]);
    });

    btnHeal.disabled = !canHeal;
    btnHeal.innerHTML = `+ MED x${hud.medkits} <small>[Q/H]</small>`;
    btnReload.hidden = !(canReload && !reloading);
  }

  function syncSetupUi() {
    scenarioButtons.forEach((btn) => {
      const selected = btn.dataset.scenario === state.setupMode;
      btn.classList.toggle("is-selected", selected);
      btn.setAttribute("aria-pressed", String(selected));
    });
    const valid = validateSeed(seedInput.value) !== null;
    seedError.hidden = valid;
    if (state.modalKind === "start") {
      btnPrimary.textContent =
        state.setupMode === "county" ? "Enter the county" : "Enter the block";
      modalEyebrow.textContent =
        state.setupMode === "county"
          ? `PROCEDURAL COUNTY · SEED ${valid ? seedInput.value.trim() : state.setupSeed}`
          : "BIRCH STREET · RESIDENTIAL BLOCK";
    }
  }

  function openModal(kind, customCopy) {
    state.modalKind = kind;
    state.playing = false;
    releaseMouseLock();
    syncEngineLoop({ controlKind: 0 });
    clearHeldInput();
    overlayEl.hidden = false;
    overlayEl.classList.toggle("is-reading", kind === "read");

    const showSetupHeading = kind !== "start";
    setupHeading.hidden = !showSetupHeading;
    btnNewRun.hidden = kind !== "paused";

    if (kind === "start") {
      modalTitle.textContent = "Zombiebox";
      modalCopy.textContent =
        "Real-time isometric survival across a seed-driven county or Birch Street. Scavenge buildings, watch your noise, and outlast the dusk.";
      syncSetupUi();
    } else if (kind === "paused") {
      modalEyebrow.textContent =
        state.runMode === "county"
          ? `PROCEDURAL COUNTY · SEED ${state.runSeed}`
          : "BIRCH STREET · RESIDENTIAL BLOCK";
      modalTitle.textContent = "Paused";
      modalCopy.textContent =
        "Clock suspended cleanly. Resume where you left off or configure a new run below.";
      btnPrimary.textContent = "Resume";
    } else if (kind === "won") {
      modalEyebrow.textContent =
        state.runMode === "county"
          ? `PROCEDURAL COUNTY · SEED ${state.runSeed}`
          : "BIRCH STREET · RESIDENTIAL BLOCK";
      modalTitle.textContent = "Block Cleared";
      modalCopy.textContent =
        "Every infected threat on Birch Street is down. Start another run or explore the procedural county.";
      btnPrimary.textContent = "Play again";
    } else if (kind === "read") {
      modalEyebrow.textContent = "READABLE";
      modalTitle.textContent = customCopy?.title || "Readable note";
      modalCopy.textContent = customCopy?.body || "";
      btnPrimary.textContent = "Close";
    } else if (kind === "dead") {
      modalEyebrow.textContent =
        state.runMode === "county"
          ? `PROCEDURAL COUNTY · SEED ${state.runSeed}`
          : "BIRCH STREET · RESIDENTIAL BLOCK";
      modalTitle.textContent = "Survivor Lost";
      modalCopy.textContent =
        "The dusk claimed you. Adjust your scenario or seed and drop back in.";
      btnPrimary.textContent = "Try again";
    } else {
      modalEyebrow.textContent = "HOST CONNECTION";
      modalTitle.textContent = "Connection Interrupted";
      modalCopy.textContent =
        customCopy || "Could not reach the Zombiebox action host. Try again.";
      btnPrimary.textContent = "Try again";
    }
    btnPrimary.focus();
  }

  async function openReadable(id) {
    try {
      const response = await fetch(`/api/action/readable/${id}`);
      if (!response.ok) throw new Error("readable unavailable");
      openModal("read", await response.json());
    } catch {
      openModal("read", {title: "Readable note", body: "The writing is too faded to make out."});
    }
  }

  function closeModalAndPlay() {
    overlayEl.hidden = true;
    state.playing = true;
    resetFpsWindow();
    state.lastTNs = Math.max(state.lastTNs, Math.round(performance.now() * 1e6));
  }

  // --- Backend API ---
  async function createActionRun(mode, seed, deployCoords) {
    let body = `mode=${encodeURIComponent(mode)}&seed=${encodeURIComponent(seed)}`;
    if (deployCoords) {
      body += `&x=${deployCoords.x}&y=${deployCoords.y}&floor=${deployCoords.floor}`;
    }
    const res = await fetch("/api/action/runs", {
      method: "POST",
      headers: { "Content-Type": "text/plain" },
      body,
    });
    if (!res.ok) throw new Error(`Create run failed (${res.status})`);
    return res.json();
  }

  async function teleportActionRun(id, seed, x, y, floor) {
    const res = await fetch(`/api/action/${encodeURIComponent(id)}/teleport`, {
      method: "POST",
      headers: { "Content-Type": "text/plain" },
      body: `seed=${encodeURIComponent(seed)}&x=${x}&y=${y}&floor=${floor}`,
    });
    if (res.status === 404) return null;
    if (!res.ok) throw new Error(`Teleport failed (${res.status})`);
    return res.json();
  }

  async function resetClock(id) {
    if (!id) return;
    try {
      await fetch(`/api/action/${encodeURIComponent(id)}/reset-clock`, {
        method: "POST",
        headers: { "Content-Type": "text/plain" },
        body: "",
        keepalive: true,
      });
    } catch {
      // Ignore network errors when suspending/unloading.
    }
  }

  function computeInputVectors() {
    let mx = 0;
    let my = 0;

    if (input.touchMove.id !== null) {
      mx = input.touchMove.mx;
      my = input.touchMove.my;
    } else {
      let kx = 0;
      let ky = 0;
      if (input.keysDown.has("a") || input.keysDown.has("ArrowLeft")) kx -= 1;
      if (input.keysDown.has("d") || input.keysDown.has("ArrowRight")) kx += 1;
      if (input.keysDown.has("w") || input.keysDown.has("ArrowUp")) ky -= 1;
      if (input.keysDown.has("s") || input.keysDown.has("ArrowDown")) ky += 1;
      const len = Math.hypot(kx, ky);
      if (len > 0) {
        const scale = input.shiftHeld ? 0.45 : 1.0;
        mx = (kx / len) * scale;
        my = (ky / len) * scale;
      }
    }

    let ax = 0;
    let ay = 0;
    if (isLookCamera()) {
      // Chase/First Person touch input is a rate. Mouse-look is sent separately as
      // a direct angular delta below so it stops as soon as the mouse stops.
      if (input.touchAim.id !== null) {
        ax = input.touchAim.lookX;
        ay = input.touchAim.lookY;
        const lookLen = Math.hypot(ax, ay);
        if (lookLen > 1) {
          ax /= lookLen;
          ay /= lookLen;
        }
      }
    } else if (input.touchAim.id !== null && input.touchAim.hasAim) {
      ax = input.touchAim.ax;
      ay = input.touchAim.ay;
    } else if (input.hasMouseAim) {
      ax = input.mouseAimX;
      ay = input.mouseAimY;
    }

    return { mx, my, ax, ay };
  }

  function decodeAndRenderFrame(buf, drawImage = true) {
    if (buf.byteLength < 64) return null;
    const view = new DataView(buf);
    if (
      view.getUint8(0) !== 0x5a ||
      view.getUint8(1) !== 0x42 ||
      view.getUint8(2) !== 0x46 ||
      (view.getUint8(3) !== 0x32 && view.getUint8(3) !== 0x33)
    ) {
      throw new Error("Invalid ZBF frame header");
    }

    const fw = view.getUint16(4, true);
    const fh = view.getUint16(6, true);
    const isV3 = view.getUint8(3) === 0x33;
    const headerSize = isV3 ? 72 : 64;
    const needed = headerSize + fw * fh * 4;
    if (buf.byteLength < needed) {
      throw new Error("Truncated ZBF2 pixel buffer");
    }

    if (canvas.width !== fw || canvas.height !== fh) {
      canvas.width = fw;
      canvas.height = fh;
    }
    const pixels = new Uint8ClampedArray(buf, headerSize, fw * fh * 4);
    if (drawImage) {
      const imageData = new ImageData(pixels, fw, fh);
      ctx.putImageData(imageData, 0, 0);
      if (state.playing) recordPresentedFrame();
    }

    const audioLo = view.getUint32(8, true);
    const hud = {
      audioLo,
      playerX: view.getInt32(24, true),
      playerY: view.getInt32(28, true),
      activeReserve: view.getUint16(32, true),
      pistolReserve: view.getUint16(34, true),
      shotgunReserve: view.getUint16(36, true),
      rocketReserve: view.getUint16(38, true),
      zombies: view.getUint16(40, true),
      zombiesTotal: view.getUint16(42, true),
      statusFlags: view.getUint16(44, true),
      health: view.getUint8(46),
      maxHealth: view.getUint8(47),
      floor: view.getUint8(48),
      equippedWeapon: view.getUint8(49),
      ownedWeaponsMask: view.getUint8(50),
      activeMag: view.getUint8(51),
      activeMagCap: view.getUint8(52),
      pistolMag: view.getUint8(53),
      shotgunMag: view.getUint8(54),
      rocketMag: view.getUint8(55),
      reloadProgress: view.getUint8(56),
      medkits: view.getUint8(57),
      cachesFound: view.getUint8(58),
      cachesTotal: view.getUint8(59),
      interactKind: view.getUint8(60),
      objectiveKind: view.getUint8(61),
      district: view.getUint8(62),
      mode: view.getUint8(63),
      controlKind: isV3 ? view.getUint8(64) : 0,
      gripUse: isV3 ? view.getUint8(65) : 0,
      speedMphX10: isV3 ? view.getUint16(66, true) : 0,
      engineLoad: isV3 ? view.getUint8(68) : 0,
      ridingFlags: isV3 ? view.getUint8(69) : 0,
      conditionPct: isV3 ? view.getUint8(70) : 0,
      readId: isV3 ? view.getUint8(71) : 0,
    };
    return hud;
  }

  async function requestFrame(advance) {
    if (!state.runId || state.inFlight) return;
    state.inFlight = true;
    const requestedCameraMode = state.cameraMode;

    const { w, h } = computeRenderSize();
    const tNs = getMonotonicNs();
    let mx = 0;
    let my = 0;
    let ax = 0;
    let ay = 0;
    let lookYaw = 0;
    let lookPitch = 0;
    let flags = state.lookAhead ? FLAG_LOOK_AHEAD : 0;
    let selectWeapon = -1;

    if (advance) {
      const vec = computeInputVectors();
      mx = vec.mx;
      my = vec.my;
      ax = vec.ax;
      ay = vec.ay;
      if (state.controlMode === 1) {
        const keyboardLean = (input.keysDown.has("l") ? 1 : 0) - (input.keysDown.has("j") ? 1 : 0);
        if (input.touchAim.id !== null) {
          ax = input.touchAim.lookX;
        } else if (keyboardLean !== 0) {
          ax = keyboardLean;
        } else if (input.hasMouseAim) {
          const rect = canvas.getBoundingClientRect();
          if (rect.width > 0) {
            const cx = rect.left + rect.width / 2;
            const dx = input.mouseClientX - cx;
            const deadZone = rect.width * 0.03;
            if (Math.abs(dx) <= deadZone) {
              ax = 0;
            } else {
              const maxDist = rect.width * 0.35;
              const sign = Math.sign(dx);
              const mag = Math.min(1, (Math.abs(dx) - deadZone) / (maxDist - deadZone));
              ax = sign * mag;
            }
          } else {
            ax = input.mouseAimX;
          }
        } else {
          ax = 0;
        }
        ay = 0;
      }
      if (isLookCamera()) {
        if (state.controlMode === 1) {
          lookYaw = 0;
          lookPitch = 0;
        } else {
          lookYaw = input.mouseLookYaw;
          lookPitch = input.mouseLookPitch;
        }
      }
      input.mouseLookYaw = 0;
      input.mouseLookPitch = 0;

      flags |= input.pendingFlags;
      input.pendingFlags = 0;
      if (state.controlMode === 0 && (input.mouseHold || input.spaceHold || input.touchAim.fireHold)) {
        flags |= FLAG_FIRE_HOLD;
      }
      if (state.controlMode === 1) flags &= ~(FLAG_FIRE_PULSE | FLAG_FIRE_HOLD | FLAG_RELOAD | FLAG_HEAL);
      selectWeapon = input.pendingWeapon;
      input.pendingWeapon = -1;
      if (state.controlMode === 1) selectWeapon = -1;
    }

    const body =
      `w=${w}&h=${h}&t_ns=${tNs}` +
      `&mx=${mx.toFixed(4)}&my=${my.toFixed(4)}` +
      `&ax=${ax.toFixed(4)}&ay=${ay.toFixed(4)}` +
      `&lyaw=${lookYaw.toFixed(5)}&lpitch=${lookPitch.toFixed(5)}` +
      `&flags=${flags}&weapon=${selectWeapon}&advance=${advance ? 1 : 0}` +
      `&camera=${state.cameraMode}&frame_version=3` +
      `&control_kind=${state.controlMode}` +
      `&steer=${state.controlMode ? mx.toFixed(4) : "0"}` +
      `&throttle=${state.controlMode ? Math.max(0, -my).toFixed(4) : "0"}` +
      `&brake=${state.controlMode ? Math.max(0, my).toFixed(4) : "0"}` +
      `&lean=${state.controlMode ? ax.toFixed(4) : "0"}` +
      `&mounted_isometric=${state.mountedIsometric ? 1 : 0}`;

    try {
      const res = await fetch(
        `/api/action/${encodeURIComponent(state.runId)}/frame`,
        {
          method: "POST",
          headers: { "Content-Type": "text/plain" },
          body,
        },
      );
      if (res.status === 404) {
        const err = new Error("Session expired");
        err.status = 404;
        throw err;
      }
      if (!res.ok) {
        const detail = (await res.text()).trim();
        const err = new Error(detail || `Frame failed (${res.status})`);
        err.status = res.status;
        throw err;
      }
      const buf = await res.arrayBuffer();
      const hud = decodeAndRenderFrame(buf, requestedCameraMode === state.cameraMode);
      if (hud) {
        const prevFlags = state.hud ? state.hud.statusFlags : 0;
        updateHud(hud);
        if (advance) {
          playSfxMask(hud.audioLo);
          const newlyWon = (hud.statusFlags & STATUS_WON) !== 0 && (prevFlags & STATUS_WON) === 0;
          const newlyDead = (hud.statusFlags & STATUS_DEAD) !== 0 && (prevFlags & STATUS_DEAD) === 0;
          if (newlyDead) {
            openModal("dead");
          } else if (newlyWon) {
            openModal("won");
          } else if ((hud.statusFlags & STATUS_READ_OPEN) !== 0 && hud.readId && hud.readId !== state.readId) {
            state.readId = hud.readId;
            void openReadable(hud.readId);
          } else if ((hud.statusFlags & STATUS_READ_OPEN) === 0) {
            state.readId = 0;
          }
        }
      }
    } catch (err) {
      state.playing = false;
      if (err.status === 404) {
        state.runId = null;
        state.started = false;
      }
      openModal(
        "error",
        err.status === 404
          ? "Your session expired on the server. Start a fresh run to continue."
          : err.message || "Lost connection to the Zombiebox host.",
      );
    } finally {
      state.inFlight = false;
      if (requestedCameraMode !== state.cameraMode && state.runId && !state.playing) {
        requestFrame(false);
      }
    }
  }

  function tickLoop() {
    if (state.playing && !state.inFlight) {
      requestFrame(true);
    }
    requestAnimationFrame(tickLoop);
  }

  // --- Lifecycle & Navigation ---
  async function startNewRun(mode, seed) {
    const validSeed = validateSeed(seed);
    if (!validSeed) {
      seedError.hidden = false;
      seedInput.focus();
      return;
    }
    seedError.hidden = true;
    btnPrimary.disabled = true;
    btnNewRun.disabled = true;
    try {
      const created = await createActionRun(mode, validSeed, null);
      state.runId = String(created.id);
      state.runSeed = String(created.seed);
      state.runMode = created.mode === "birch" ? "birch" : "county";
      state.started = true;
      state.dead = false;
      state.hud = null;
      resetFpsWindow();
      hudFps.textContent = "-- FPS";
      state.playerX = created.x ?? 0;
      state.playerY = created.y ?? 0;
      state.playerFloor = created.floor ?? 0;
      saveSession();
      closeModalAndPlay();
      await requestFrame(false);
    } catch {
      openModal("error", "Could not start a new run on the game host.");
    } finally {
      btnPrimary.disabled = false;
      btnNewRun.disabled = false;
    }
  }

  async function pauseGame() {
    if (!state.playing) return;
    state.playing = false;
    clearHeldInput();
    await resetClock(state.runId);
    openModal("paused");
  }

  async function openAtlas() {
    ensureAudio();
    const wasPlaying = state.playing;
    if (wasPlaying) {
      state.playing = false;
      releaseMouseLock();
      clearHeldInput();
    }
    if (state.runId) {
      await resetClock(state.runId);
    }
    const isActive = state.started && state.runMode === "county" && !state.dead;
    const validSetup = validateSeed(seedInput.value);
    const atlasSeed = isActive ? state.runSeed : validSetup || state.runSeed || "42";
    const params = new URLSearchParams({
      seed: atlasSeed,
      active: isActive ? "1" : "0",
      px: String(state.playerX),
      py: String(state.playerY),
      pf: String(state.playerFloor),
    });
    location.href = `/county.html?${params.toString()}`;
  }

  async function initStartup() {
    const params = new URLSearchParams(location.search);
    const saved = loadSession();

    if (params.has("tx") && params.has("ty")) {
      const tx = Number.parseInt(params.get("tx"), 10);
      const ty = Number.parseInt(params.get("ty"), 10);
      const tf = Number.parseInt(params.get("tf") || "0", 10) || 0;
      const paramSeed = validateSeed(params.get("seed")) || saved?.seed || "42";
      history.replaceState(null, "", "/");

      if (Number.isFinite(tx) && Number.isFinite(ty)) {
        state.setupMode = "county";
        state.setupSeed = paramSeed;
        seedInput.value = paramSeed;
        syncSetupUi();
        try {
          let data = null;
          if (saved && saved.started) {
            data = await teleportActionRun(saved.id, paramSeed, tx, ty, tf);
          }
          if (!data) {
            data = await createActionRun("county", paramSeed, {
              x: tx,
              y: ty,
              floor: tf,
            });
          }
          state.runId = String(data.id);
          state.runSeed = String(data.seed);
          state.runMode = "county";
          state.started = true;
          state.dead = false;
          state.playerX = data.x;
          state.playerY = data.y;
          state.playerFloor = data.floor ?? tf;
          saveSession();
          closeModalAndPlay();
          await requestFrame(false);
          showToast(
            data.preserved
              ? `Teleported to (${data.x}, ${data.y}) · gear preserved`
              : `Deployed at (${data.x}, ${data.y}) · Seed ${data.seed}`,
          );
          return;
        } catch {
          openModal("error", "Teleport to county coordinates failed.");
          return;
        }
      }
    }

    if (params.get("resume") === "1" && saved) {
      history.replaceState(null, "", "/");
      state.runId = saved.id;
      state.runSeed = saved.seed;
      state.runMode = saved.mode;
      state.started = saved.started;
      state.setupMode = saved.mode;
      state.setupSeed = saved.seed;
      seedInput.value = saved.seed;
      syncSetupUi();
      try {
        await requestFrame(false);
        if (state.runId) {
          openModal(saved.started ? "paused" : "start");
          return;
        }
      } catch {
        // Fall through to creating a default preview run.
      }
    }

    try {
      const preview = await createActionRun("county", "42", null);
      state.runId = String(preview.id);
      state.runSeed = String(preview.seed);
      state.runMode = "county";
      state.started = false;
      state.setupMode = "county";
      state.setupSeed = "42";
      seedInput.value = "42";
      state.playerX = preview.x ?? 0;
      state.playerY = preview.y ?? 0;
      state.playerFloor = preview.floor ?? 0;
      saveSession();
      await requestFrame(false);
      openModal("start");
    } catch {
      openModal("error", "Could not connect to the Zombiebox host.");
    }
  }

  // --- Input Event Wiring ---
  function isLookCamera(mode = state.cameraMode) {
    return mode === CAMERA_CHASE || mode === CAMERA_FIRST_PERSON;
  }

  function releaseMouseLock() {
    if (document.pointerLockElement === canvas && document.exitPointerLock) {
      document.exitPointerLock();
    }
    input.mouseTracking = false;
    input.mouseLookYaw = 0;
    input.mouseLookPitch = 0;
    input.mouseHold = false;
  }

  function requestMouseLock() {
    if (pointerLockPending) return true;
    if (pointerLockUnavailable || !canvas.requestPointerLock) return false;
    pointerLockPending = true;
    try {
      // Older implementations return undefined and report errors via an event.
      canvas.requestPointerLock()?.catch?.(handleMouseLockError);
      return true;
    } catch {
      handleMouseLockError();
      return false;
    }
  }

  function accumulateMouseLook(clientX, clientY) {
    if (!input.mouseTracking) {
      input.mouseTracking = true;
      input.mouseLastClientX = clientX;
      input.mouseLastClientY = clientY;
      return;
    }
    const dx = clientX - input.mouseLastClientX;
    const dy = clientY - input.mouseLastClientY;
    input.mouseLastClientX = clientX;
    input.mouseLastClientY = clientY;
    accumulateMouseDelta(dx, dy);
  }

  function accumulateMouseDelta(dx, dy) {
    if (!Number.isFinite(dx) || !Number.isFinite(dy)) return;
    const limit = MOUSE_LOOK_MAX_RADIANS;
    input.mouseLookYaw = Math.max(-limit, Math.min(limit, input.mouseLookYaw + dx * MOUSE_LOOK_RADIANS_PER_PX));
    input.mouseLookPitch = Math.max(-limit, Math.min(limit, input.mouseLookPitch + dy * MOUSE_LOOK_RADIANS_PER_PX));
  }

  function updateMouseAim(clientX, clientY) {
    input.mouseClientX = clientX;
    input.mouseClientY = clientY;
    const rect = canvas.getBoundingClientRect();
    if (!rect.width || !rect.height) return;
    const cx = rect.left + rect.width / 2;
    const cy = rect.top + rect.height / 2;
    const dx = clientX - cx;
    const dy = clientY - cy;
    const len = Math.hypot(dx, dy);
    if (len >= 2) {
      input.mouseAimX = dx / len;
      input.mouseAimY = dy / len;
      input.hasMouseAim = true;
    }
  }

  canvas.addEventListener("pointerdown", (e) => {
    ensureAudio();
    if (!state.playing) return;
    e.preventDefault();

    if (e.pointerType === "touch") {
      const rect = canvas.getBoundingClientRect();
      const isLeft = e.clientX < rect.left + rect.width * 0.5;
      try {
        canvas.setPointerCapture(e.pointerId);
      } catch {
        // Ignore if pointer capture is unavailable.
      }
      if (isLeft && input.touchMove.id === null) {
        input.touchMove.id = e.pointerId;
        input.touchMove.ox = e.clientX - rect.left;
        input.touchMove.oy = e.clientY - rect.top;
        input.touchMove.mx = 0;
        input.touchMove.my = 0;
        updateStickVisuals();
      } else if (!isLeft && input.touchAim.id === null) {
        input.touchAim.id = e.pointerId;
        input.touchAim.ox = e.clientX - rect.left;
        input.touchAim.oy = e.clientY - rect.top;
        input.touchAim.fireHold = false;
        input.touchAim.lookX = 0;
        input.touchAim.lookY = 0;
        input.touchAim.downAt = performance.now();
        input.touchAim.maxDist = 0;
        updateStickVisuals();
      }
      return;
    }

    if (e.button === 0) {
      updateMouseAim(e.clientX, e.clientY);
      if (isLookCamera() && state.controlMode === 0 && document.pointerLockElement !== canvas) {
        // The first click only captures the pointer. It must not fire a shot.
        if (requestMouseLock()) return;
      }
      if (isLookCamera() && document.pointerLockElement !== canvas) {
        try { canvas.setPointerCapture(e.pointerId); } catch { /* optional fallback */ }
        input.mouseTracking = true;
        input.mouseLastClientX = e.clientX;
        input.mouseLastClientY = e.clientY;
      }
      if (state.controlMode === 0) {
        input.mouseHold = true;
        input.pendingFlags |= FLAG_FIRE_PULSE;
      }
    }
  });

  canvas.addEventListener("pointermove", (e) => {
    if (e.pointerType === "touch") {
      const rect = canvas.getBoundingClientRect();
      const maxRadius = STICK_RADIUS;
      if (e.pointerId === input.touchMove.id) {
        const dx = e.clientX - rect.left - input.touchMove.ox;
        const dy = e.clientY - rect.top - input.touchMove.oy;
        const dist = Math.hypot(dx, dy);
        const norm = Math.min(1, dist / maxRadius);
        if (norm < 0.08) {
          input.touchMove.mx = 0;
          input.touchMove.my = 0;
        } else {
          input.touchMove.mx = (dx / dist) * norm;
          input.touchMove.my = (dy / dist) * norm;
        }
        updateStickVisuals();
      } else if (e.pointerId === input.touchAim.id) {
        const dx = e.clientX - rect.left - input.touchAim.ox;
        const dy = e.clientY - rect.top - input.touchAim.oy;
        const dist = Math.hypot(dx, dy);
        input.touchAim.maxDist = Math.max(input.touchAim.maxDist, dist);
        const deadZone = maxRadius * TOUCH_LOOK_DEADZONE;
        if (dist > deadZone) {
          const analog = Math.min(1, (dist - deadZone) / (maxRadius - deadZone));
          const nx = dx / dist;
          const ny = dy / dist;
          input.touchAim.ax = nx;
          input.touchAim.ay = ny;
          input.touchAim.lookX = nx * analog;
          input.touchAim.lookY = ny * analog;
          input.touchAim.hasAim = true;
        } else {
          input.touchAim.lookX = 0;
          input.touchAim.lookY = 0;
        }
        input.touchAim.fireHold = state.controlMode === 0 && dist >= maxRadius * 0.78;
        updateStickVisuals();
      }
      return;
    }
    if (document.pointerLockElement === canvas) return;
    updateMouseAim(e.clientX, e.clientY);
    if (isLookCamera() && state.playing && state.controlMode === 0) {
      accumulateMouseLook(e.clientX, e.clientY);
    } else {
      input.mouseTracking = false;
    }
  });

  document.addEventListener("mousemove", (e) => {
    if (document.pointerLockElement === canvas && state.playing && state.controlMode === 0 && isLookCamera()) {
      accumulateMouseDelta(e.movementX, e.movementY);
    }
  });

  canvas.addEventListener("pointerleave", () => {
    if (document.pointerLockElement !== canvas) {
      input.mouseTracking = false;
      input.mouseLookYaw = 0;
      input.mouseLookPitch = 0;
    }
  });

  document.addEventListener("pointerlockchange", () => {
    pointerLockPending = false;
    input.mouseTracking = false;
    input.mouseLookYaw = 0;
    input.mouseLookPitch = 0;
    if (document.pointerLockElement !== canvas) {
      input.mouseHold = false;
    } else if (!state.playing || state.controlMode !== 0 || !isLookCamera()) {
      releaseMouseLock();
    }
  });

  function handleMouseLockError() {
    if (!pointerLockPending) return;
    pointerLockPending = false;
    pointerLockUnavailable = true;
    input.mouseTracking = false;
  }

  document.addEventListener("pointerlockerror", handleMouseLockError);

  function releasePointer(e) {
    if (e.pointerType === "touch") {
      if (e.pointerId === input.touchMove.id) {
        input.touchMove.id = null;
        input.touchMove.mx = 0;
        input.touchMove.my = 0;
        updateStickVisuals();
      } else if (e.pointerId === input.touchAim.id) {
        const elapsed = performance.now() - input.touchAim.downAt;
        if (state.controlMode === 0 && elapsed < 240 && input.touchAim.maxDist < 20 && state.playing) {
          input.pendingFlags |= FLAG_FIRE_PULSE;
        }
        input.touchAim.id = null;
        input.touchAim.fireHold = false;
        input.touchAim.lookX = 0;
        input.touchAim.lookY = 0;
        updateStickVisuals();
      }
      return;
    }
    if (e.button === 0) {
      input.mouseHold = false;
      if (e.pointerType === "mouse" && document.pointerLockElement !== canvas) {
        try { canvas.releasePointerCapture(e.pointerId); } catch { /* optional fallback */ }
      }
    }
  }

  canvas.addEventListener("pointerup", releasePointer);
  canvas.addEventListener("pointercancel", releasePointer);
  canvas.addEventListener("contextmenu", (e) => e.preventDefault());

  document.addEventListener("keydown", (e) => {
    const active = document.activeElement;
    if (active === seedInput) {
      if (e.key === "Escape" && state.started && state.modalKind === "paused") {
        btnPrimary.click();
      }
      return;
    }

    const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
    if (k === "Shift") {
      input.shiftHeld = true;
    }

    if (k === "Escape") {
      e.preventDefault();
      if (state.playing) {
        pauseGame();
      } else if (state.modalKind === "paused" && state.runId) {
        closeModalAndPlay();
      }
      return;
    }

    if (k === "m" && !e.repeat) {
      e.preventDefault();
      openAtlas();
      return;
    }

    if (k === "c" && !e.repeat) {
      e.preventDefault();
      cycleCamera();
      return;
    }

    if (!state.playing) return;

    if (["w", "a", "s", "d", "j", "l", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"].includes(k)) {
      e.preventDefault();
      input.keysDown.add(k);
      return;
    }

    if (k === " ") {
      e.preventDefault();
      if (state.controlMode === 0 && !e.repeat) {
        ensureAudio();
        input.spaceHold = true;
        input.pendingFlags |= FLAG_FIRE_PULSE;
      }
      return;
    }

    if (e.repeat) return;

    if (k === "e") {
      e.preventDefault();
      ensureAudio();
      input.pendingFlags |= FLAG_INTERACT;
    } else if (state.controlMode === 0 && k === "r") {
      e.preventDefault();
      ensureAudio();
      input.pendingFlags |= FLAG_RELOAD;
    } else if (state.controlMode === 0 && (k === "q" || k === "h")) {
      e.preventDefault();
      ensureAudio();
      input.pendingFlags |= FLAG_HEAL;
    } else if (state.controlMode === 0 && k >= "1" && k <= "4") {
      e.preventDefault();
      ensureAudio();
      input.pendingWeapon = Number(k) - 1;
    }
  });

  document.addEventListener("keyup", (e) => {
    const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
    input.keysDown.delete(k);
    if (k === "Shift") {
      input.shiftHeld = false;
    }
    if (k === " ") {
      input.spaceHold = false;
    }
  });

  window.addEventListener("blur", () => {
    if (state.playing) {
      pauseGame();
    } else {
      clearHeldInput();
    }
  });

  document.addEventListener("visibilitychange", () => {
    if (document.hidden) {
      if (state.playing) {
        pauseGame();
      } else {
        clearHeldInput();
      }
    }
  });

  window.addEventListener("resize", () => {
    if (!state.playing && state.runId) {
      requestFrame(false);
    }
  });

  // --- HUD & Modal Button Listeners ---
  btnLook.addEventListener("click", () => {
    state.lookAhead = !state.lookAhead;
    btnLook.classList.toggle("is-active", state.lookAhead);
    btnLook.setAttribute("aria-pressed", String(state.lookAhead));
    btnLook.blur();
    if (!state.playing && state.runId) requestFrame(false);
  });

  function cycleCamera() {
    state.cameraMode = (state.cameraMode + 1) % 5;
    if (!isLookCamera()) {
      releaseMouseLock();
      pointerLockUnavailable = false;
    }
    state.mountedIsometric = state.cameraMode === 0;
    input.mouseLookYaw = 0;
    input.mouseLookPitch = 0;
    input.mouseTracking = false;
    input.touchAim.lookX = 0;
    input.touchAim.lookY = 0;
    const labels = ["ISO", "TOP", "ORBIT", "CHASE", "1ST"];
    btnCamera.textContent = `${labels[state.cameraMode]} [C]`;
    btnCamera.setAttribute("aria-label", `Camera view: ${["Isometric", "Overhead", "Orbit", "Chase", "First person"][state.cameraMode]}`);
    btnCamera.blur();
    if (!state.playing && state.runId) requestFrame(false);
  }

  btnCamera.addEventListener("click", cycleCamera);

  btnSound.addEventListener("click", () => {
    state.muted = !state.muted;
    if (state.muted) syncEngineLoop({ controlKind: 0 });
    btnSound.setAttribute("aria-pressed", String(state.muted));
    btnSound.setAttribute("aria-label", state.muted ? "Unmute sound" : "Mute sound");
    if (!state.muted) ensureAudio();
    btnSound.blur();
  });

  btnPause.addEventListener("click", () => {
    btnPause.blur();
    pauseGame();
  });

  btnAtlas.addEventListener("click", () => {
    btnAtlas.blur();
    openAtlas();
  });

  btnModalAtlas.addEventListener("click", () => {
    openAtlas();
  });

  btnInteract.addEventListener("click", () => {
    ensureAudio();
    if (state.playing) input.pendingFlags |= FLAG_INTERACT;
    btnInteract.blur();
  });

  btnHeal.addEventListener("click", () => {
    ensureAudio();
    if (state.playing) input.pendingFlags |= FLAG_HEAL;
    btnHeal.blur();
  });

  btnReload.addEventListener("click", () => {
    ensureAudio();
    if (state.playing) input.pendingFlags |= FLAG_RELOAD;
    btnReload.blur();
  });

  slotButtons.forEach((btn, idx) => {
    btn.addEventListener("click", () => {
      ensureAudio();
      if (!state.playing) return;
      if (state.hud && state.hud.equippedWeapon === idx) {
        const reloading = (state.hud.statusFlags & STATUS_RELOADING) !== 0;
        const canReload = (state.hud.statusFlags & STATUS_CAN_RELOAD) !== 0;
        if (canReload && !reloading) {
          input.pendingFlags |= FLAG_RELOAD;
        }
      } else {
        input.pendingWeapon = idx;
      }
      btn.blur();
    });
  });

  scenarioButtons.forEach((btn) => {
    btn.addEventListener("click", () => {
      state.setupMode = btn.dataset.scenario === "birch" ? "birch" : "county";
      syncSetupUi();
    });
  });

  seedInput.addEventListener("input", () => {
    const valid = validateSeed(seedInput.value);
    if (valid !== null) state.setupSeed = valid;
    syncSetupUi();
  });

  btnRandomSeed.addEventListener("click", () => {
    const s = randomU64Seed();
    seedInput.value = s;
    state.setupSeed = s;
    syncSetupUi();
  });

  btnPrimary.addEventListener("click", async () => {
    ensureAudio();
    if (state.modalKind === "read") {
      closeModalAndPlay();
      requestFrame(false);
      return;
    }
    if (state.modalKind === "paused" && state.runId) {
      closeModalAndPlay();
      return;
    }
    if (state.modalKind === "start") {
      const validSeed = validateSeed(seedInput.value);
      if (!validSeed) {
        seedError.hidden = false;
        seedInput.focus();
        return;
      }
      if (
        state.runId &&
        !state.started &&
        state.runMode === state.setupMode &&
        state.runSeed === validSeed
      ) {
        state.started = true;
        saveSession();
        closeModalAndPlay();
        return;
      }
      await startNewRun(state.setupMode, validSeed);
      return;
    }
    await startNewRun(state.setupMode, seedInput.value);
  });

  btnNewRun.addEventListener("click", async () => {
    ensureAudio();
    await startNewRun(state.setupMode, seedInput.value);
  });

  initStartup();
  requestAnimationFrame(tickLoop);
})();
