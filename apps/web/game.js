(() => {
  "use strict";
  const $ = (id) => document.getElementById(id);
  const canvas = $("world");
  const tickMs = 50;
  const maxBatch = 3;
  const maxHealth = 5;
  let state = null;
  let running = false;
  let inFlight = false;
  let accumulator = 0;
  let lastFrame = 0;
  let held = null;
  let heldTicks = 0;
  let queue = [];
  let effects = [];
  let noticeEndsAt = 0;
  let mode = "start";
  let audio = null;
  let muted = false;
  let openingRun;

  function notice(message) {
    const toast = $("toast");
    toast.hidden = false;
    toast.textContent = message;
    noticeEndsAt = performance.now() + 2200;
  }

  function enableAudio() {
    if (muted || !window.AudioContext) return;
    audio ??= new AudioContext();
    if (audio.state === "suspended") audio.resume().catch(() => {});
  }

  function playSound(type) {
    if (muted || audio?.state !== "running") return;
    const tone = audio.createOscillator();
    const gain = audio.createGain();
    const start = audio.currentTime;
    const duration = type === "gunshot" ? 0.15 : 0.2;
    const frequency = { gunshot: 145, scavenged: 380, hurt: 110, clear: 520 }[type];
    tone.type = type === "gunshot" || type === "hurt" ? "triangle" : "sine";
    tone.frequency.setValueAtTime(frequency, start);
    tone.frequency.exponentialRampToValueAtTime(
      Math.max(35, frequency * (type === "clear" || type === "scavenged" ? 1.5 : 0.25)),
      start + duration,
    );
    gain.gain.setValueAtTime(type === "gunshot" ? 0.16 : 0.09, start);
    gain.gain.exponentialRampToValueAtTime(0.001, start + duration);
    tone.connect(gain).connect(audio.destination);
    tone.start(start);
    tone.stop(start + duration);
  }

  function setBackgroundInert(inert) {
    document
      .querySelectorAll(".top, .hud, .world, .bottom")
      .forEach((element) => {
        element.inert = inert;
      });
  }
  function setModal(title, copy, button, showInstructions) {
    $("modal-title").innerHTML = title;
    $("modal-copy").textContent = copy;
    $("start").innerHTML = button + " <span>↗</span>";
    $("instructions").hidden = !showInstructions;
    $("overlay").hidden = false;
    setBackgroundInert(true);
    $("start").focus();
    running = false;
    clearInput();
  }
  function clearInput() {
    held = null;
    heldTicks = 0;
    queue = [];
    accumulator = 0;
    lastFrame = 0;
    document
      .querySelectorAll(".pressed")
      .forEach((b) => b.classList.remove("pressed"));
  }
  function onCache() {
    return state.tiles.caches.some(
      (cache) => cache.x === state.player.x && cache.y === state.player.y,
    );
  }

  function updateUI() {
    $("health").textContent = state.health + "/" + maxHealth;
    $("health-bar").style.width = (state.health / maxHealth) * 100 + "%";
    $("weapon-label").textContent = state.armed ? "PISTOL" : "UNARMED";
    $("supplies").textContent = state.armed ? state.supplies + " rounds" : "—";
    $("threats").textContent = state.zombies.length;
    const seconds = Math.floor((state.tick * tickMs) / 1000);
    $("tick").textContent = [Math.floor(seconds / 60), seconds % 60]
      .map((part) => String(part).padStart(2, "0"))
      .join(":");
    $("objective").textContent = state.armed
      ? "Clear the block."
      : "Find the pistol inside the house.";
    const cacheHere = onCache();
    $("loot").classList.toggle("ready", cacheHere);
    $("loot-hint").textContent = cacheHere ? "CACHE HERE" : "AT A CACHE";
    $("attack").classList.toggle("ready", Boolean(state.aim));
    const fireHint = !state.armed ? "UNARMED" : "FIND A TARGET";
    $("fire-hint").textContent = state.aim ? "CLEAR SHOT" : fireHint;
  }

  function queueInput(command) {
    if (running && queue.length < 8) queue.push(command);
  }

  async function createRun() {
    const response = await fetch("/api/runs", {
      method: "POST",
      headers: { "Content-Type": "text/plain" },
      body: "42",
    });
    if (!response.ok) throw new Error("Unable to start");
    return response.json();
  }

  async function startRun() {
    clearInput();
    $("start").disabled = true;
    $("start").textContent = "CONNECTING…";
    try {
      const initial = await openingRun;
      openingRun = null;
      state = initial ?? (await createRun());
      effects = [];
      updateUI();
      $("overlay").hidden = true;
      setBackgroundInert(false);
      running = true;
      mode = "playing";
      notice("Follow the lit doorway");
      if (document.hidden) pause();
    } catch (err) {
      mode = "error";
      setModal(
        "Connection<br><em>interrupted.</em>",
        "Could not reach the game host. Check your connection and try again.",
        "TRY AGAIN",
        false,
      );
    } finally {
      $("start").disabled = false;
      if (!$("overlay").hidden) $("start").focus();
    }
  }
  function pause() {
    if (!running) return;
    mode = "paused";
    setModal(
      "Take a breath.<br><em>The block can wait.</em>",
      "The simulation is paused. Resume when you are ready.",
      "RESUME",
      false,
    );
  }
  function finish() {
    if (!state || !running) return;
    if (state.health <= 0) {
      mode = "finished";
      setModal(
        "The light<br><em>fades out.</em>",
        "You fell on the block. Every run is another chance.",
        "TRY AGAIN",
        false,
      );
    } else if (!state.zombies.length) {
      mode = "finished";
      running = false;
      clearInput();
      $("objective").textContent = "Block cleared.";
      setTimeout(() => {
        playSound("clear");
        setModal(
          "The block<br><em>is quiet.</em>",
          "You cleared all three threats. For now, the street is yours.",
          "PLAY AGAIN",
          false,
        );
      }, 320);
    }
  }
  async function step(commands) {
    const id = state.id;
    inFlight = true;
    try {
      const res = await fetch(`/api/runs/${encodeURIComponent(id)}/step`, {
        method: "POST",
        headers: {
          "Content-Type": "text/plain",
        },
        body: commands,
      });
      if (!res.ok) throw new Error("Step failed");
      const data = await res.json();
      state = data;
      const now = performance.now();
      let fired = false;
      let scavenged = false;
      for (const event of data.events) {
        if (event.type === "gunshot") {
          effects.push({ ...event, start: now });
          playSound("gunshot");
          fired = true;
        } else if (event.type === "scavenged") {
          scavenged = true;
          notice("Pistol equipped");
          playSound("scavenged");
        } else if (event.type === "hurt") {
          effects.push({ type: "hurt", at: state.player, start: now });
          notice("Move away from the infected");
          playSound("hurt");
        }
      }
      if (state.health > 0) {
        if (commands.includes("L") && !scavenged) {
          notice(
            state.armed ? "The crate has been emptied" : "Stand on the marked crate inside",
          );
        }
        if (commands.includes("A") && !fired) {
          if (!state.armed) notice("Find the pistol inside the house");
          else notice(state.supplies ? "No clear shot" : "Out of rounds");
        }
      }
      updateUI();
      finish();
    } catch (err) {
      mode = "error";
      setModal(
        "Connection<br><em>interrupted.</em>",
        "The game host could not be reached. Start a new run to try again.",
        "TRY AGAIN",
        false,
      );
    } finally {
      inFlight = false;
    }
  }
  function command() {
    if (queue.length) return queue.shift();
    if (held) {
      if (heldTicks++ % 3 === 0) return held;
    }
    return ".";
  }
  function frame(now) {
    const elapsed = lastFrame ? now - lastFrame : 0;
    lastFrame = now;
    if (running && state) {
      accumulator = Math.min(accumulator + elapsed, tickMs * maxBatch);
      if (!inFlight && accumulator >= tickMs) {
        const ticks = Math.floor(accumulator / tickMs);
        accumulator -= ticks * tickMs;
        let batch = "";
        for (let tick = 0; tick < ticks; tick++) batch += command();
        step(batch);
      }
    }
    if (noticeEndsAt && now >= noticeEndsAt) {
      $("toast").hidden = true;
      noticeEndsAt = 0;
    }
    effects = effects.filter((effect) => now - effect.start < 700);
    window.ZombieboxScene.draw(canvas, state, now, effects);
    requestAnimationFrame(frame);
  }

  $("start").addEventListener("click", () => {
    enableAudio();
    if (mode === "paused") {
      $("overlay").hidden = true;
      setBackgroundInert(false);
      running = true;
      mode = "playing";
      lastFrame = 0;
      finish();
    } else startRun();
  });
  $("sound").addEventListener("click", () => {
    muted = !muted;
    $("sound").setAttribute("aria-pressed", String(muted));
    $("sound").setAttribute("aria-label", muted ? "Unmute sound" : "Mute sound");
    enableAudio();
  });
  $("help").addEventListener("click", () => {
    if (running) {
      mode = "paused";
      setModal(
        "How to<br><em>survive.</em>",
        "Get inside the house, scavenge the marked crate, then line up a clear shot.",
        "RESUME",
        true,
      );
    }
  });
  for (const b of document.querySelectorAll("[data-move]")) {
    const release = () => {
      if (held === b.dataset.move) held = null;
      b.classList.remove("pressed");
    };
    b.addEventListener("pointerdown", (e) => {
      e.preventDefault();
      if (!running) return;
      held = b.dataset.move;
      heldTicks = 1;
      queueInput(held);
      b.classList.add("pressed");
      b.setPointerCapture(e.pointerId);
    });
    b.addEventListener("click", (event) => {
      if (event.detail === 0) queueInput(b.dataset.move);
    });
    b.addEventListener("pointerup", release);
    b.addEventListener("pointercancel", release);
    b.addEventListener("lostpointercapture", release);
  }
  $("loot").addEventListener("click", () => {
    queueInput("L");
  });
  $("attack").addEventListener("click", () => {
    queueInput("A");
  });
  const keys = {
    ArrowUp: "N",
    ArrowRight: "E",
    ArrowDown: "S",
    ArrowLeft: "W",
    w: "N",
    d: "E",
    s: "S",
    a: "W",
  };
  document.addEventListener("keydown", (e) => {
    if (!running) return;
    const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
    if (keys[k]) {
      e.preventDefault();
      if (!e.repeat) {
        held = keys[k];
        heldTicks = 1;
        queueInput(held);
      }
    } else if (!e.repeat && (k === " " || k === "l")) {
      if (k === " " && e.target instanceof HTMLButtonElement) return;
      e.preventDefault();
      queueInput(k === " " ? "A" : "L");
    } else if (k === "Escape") pause();
  });
  document.addEventListener("keyup", (e) => {
    const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
    if (keys[k] === held) held = null;
  });
  window.addEventListener("blur", pause);
  document.addEventListener("visibilitychange", () => {
    if (document.hidden) pause();
  });
  openingRun = createRun()
    .then((preview) => {
      state = preview;
      updateUI();
      return preview;
    })
    .catch(() => null);
  requestAnimationFrame(frame);
})();
