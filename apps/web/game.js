(() => {
  "use strict";
  const $ = (id) => document.getElementById(id),
    canvas = $("world"),
    ctx = canvas.getContext("2d");
  let state = null,
    running = false,
    inFlight = false,
    generation = 0,
    accumulator = 0,
    last = 0,
    held = null,
    heldTicks = 0,
    queue = [],
    effects = [],
    noticeTimer = 0,
    mode = "start";
  const colors = {
    r: "#64675a",
    g: "#516c50",
    p: "#858778",
    f: "#736f50",
    v: "#777c6c",
  };
  function notice(message) {
    $("toast").textContent = message;
    noticeTimer = performance.now() + 2400;
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
    last = 0;
    document
      .querySelectorAll(".pressed")
      .forEach((b) => b.classList.remove("pressed"));
  }
  function updateUI() {
    if (!state) return;
    $("health").textContent = state.health + "/5";
    $("health-bar").style.width =
      Math.max(0, Math.min(100, (state.health / 5) * 100)) + "%";
    $("supplies").textContent = state.supplies;
    $("threats").textContent = state.zombies.length;
    $("tick").textContent = `DAY 01 · ${String(
      Math.floor(state.tick / 1200),
    ).padStart(2, "0")}:${String(Math.floor(state.tick / 20) % 60).padStart(
      2,
      "0",
    )}`;
    $("objective").textContent = state.supplies
      ? "Clear the remaining threats."
      : "Scavenge supplies. Clear the threats.";
  }
  async function startRun() {
    const current = ++generation;
    clearInput();
    inFlight = false;
    $("start").disabled = true;
    $("start").textContent = "CONNECTING…";
    try {
      const res = await fetch("/api/runs", {
        method: "POST",
        headers: {
          "Content-Type": "text/plain",
        },
        body: "42",
      });
      if (!res.ok) throw new Error("Unable to start");
      const data = await res.json();
      if (current !== generation) return;
      state = data;
      effects = [];
      updateUI();
      $("overlay").hidden = true;
      setBackgroundInert(false);
      running = true;
      mode = "playing";
      notice("Find the supply cache");
    } catch (err) {
      if (current === generation) {
        mode = "error";
        setModal(
          "Connection<br><em>interrupted.</em>",
          "Could not reach the game host. Check your connection and try again.",
          "TRY AGAIN",
          false,
        );
      }
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
      setModal(
        "The block<br><em>is quiet.</em>",
        "You cleared all three threats. For now, the street is yours.",
        "PLAY AGAIN",
        false,
      );
    }
  }
  async function step(commands) {
    const current = generation,
      id = state.id;
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
      if (current !== generation) return;
      state = data;
      const now = performance.now();
      for (const e of data.events || []) {
        if (e.type === "explosion") {
          effects.push({
            x: e.x ?? data.player.x,
            y: e.y ?? data.player.y,
            start: now,
          });
          notice("Blast!");
        } else if (e.type === "scavenged") notice("Supplies found");
        else if (e.type === "hurt") notice("You were hit");
      }
      updateUI();
      finish();
    } catch (err) {
      if (current === generation) {
        mode = "error";
        setModal(
          "Connection<br><em>interrupted.</em>",
          "The game host could not be reached. Start a new run to try again.",
          "TRY AGAIN",
          false,
        );
      }
    } finally {
      if (current === generation) inFlight = false;
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
    if (!last) last = now;
    const elapsed = Math.min(now - last, 150);
    last = now;
    if (running && state) {
      accumulator += elapsed;
      if (!inFlight && accumulator >= 50) {
        const n = Math.min(3, Math.floor(accumulator / 50));
        accumulator = Math.min(accumulator - n * 50, 49);
        let batch = "";
        for (let i = 0; i < n; i++) batch += command();
        step(batch);
      } else if (inFlight) accumulator = Math.min(accumulator, 150);
    }
    draw(now);
    requestAnimationFrame(frame);
  }
  function diamond(x, y, w, h, color) {
    ctx.fillStyle = color;
    ctx.beginPath();
    ctx.moveTo(x, y - h / 2);
    ctx.lineTo(x + w / 2, y);
    ctx.lineTo(x, y + h / 2);
    ctx.lineTo(x - w / 2, y);
    ctx.closePath();
    ctx.fill();
  }
  function draw(now) {
    const rect = canvas.getBoundingClientRect(),
      dpr = Math.min(devicePixelRatio || 1, 2),
      w = rect.width,
      h = rect.height;
    if (
      canvas.width !== Math.round(w * dpr) ||
      canvas.height !== Math.round(h * dpr)
    ) {
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(h * dpr);
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.fillStyle = "#34483a";
    ctx.fillRect(0, 0, w, h);
    if (!state) return;
    const t = state.tiles,
      tw = Math.max(27, Math.min(54, w / 8)),
      th = tw / 2,
      cx = w / 2,
      cy = h * 0.48;
    const project = (x, y) => ({
      x: cx + ((x - state.player.x - (y - state.player.y)) * tw) / 2,
      y: cy + ((x - state.player.x + y - state.player.y) * th) / 2,
    });
    for (let y = t.y; y < t.y + t.height; y++)
      for (let x = t.x; x < t.x + t.width; x++) {
        const p = project(x, y),
          index = (y - t.y) * t.width + x - t.x,
          type = t.ground[index],
          shade = ((x * 17 + y * 31) % 5) * 2;
        diamond(p.x, p.y, tw + 1, th + 1, colors[type] || colors.g);
        ctx.fillStyle = `rgba(15,25,18,${shade / 100})`;
        diamond(p.x, p.y, tw - 1, th - 1, ctx.fillStyle);
        if (type === "r" && (x + y) % 3 === 0) {
          ctx.strokeStyle = "#a8a48a66";
          ctx.lineWidth = 1;
          ctx.beginPath();
          ctx.moveTo(p.x - 4, p.y - 2);
          ctx.lineTo(p.x + 4, p.y + 2);
          ctx.stroke();
        }
      }
    const actors = [
      ...t.caches.map((c) => ({
        ...c,
        kind: "cache",
      })),
      ...state.zombies.map((z) => ({
        ...z,
        kind: "zombie",
      })),
      {
        ...state.player,
        kind: "player",
      },
    ].sort((a, b) => a.x + a.y - (b.x + b.y));
    for (const a of actors) {
      const p = project(a.x, a.y);
      if (p.x < -tw || p.x > w + tw || p.y < -tw * 2 || p.y > h + tw) continue;
      ctx.fillStyle = "#101b1590";
      ctx.beginPath();
      ctx.ellipse(p.x, p.y + 2, tw * 0.22, th * 0.2, 0, 0, Math.PI * 2);
      ctx.fill();
      if (a.kind === "cache") {
        ctx.fillStyle = "#d8ad6a";
        ctx.fillRect(p.x - tw * 0.16, p.y - tw * 0.25, tw * 0.32, tw * 0.23);
        ctx.fillStyle = "#715438";
        ctx.fillRect(p.x - tw * 0.16, p.y - tw * 0.19, tw * 0.32, tw * 0.04);
        ctx.fillStyle = "#f5d997";
        ctx.fillRect(p.x - 2, p.y - tw * 0.17, 4, 4);
        ctx.strokeStyle = "#e9c785";
        ctx.beginPath();
        ctx.arc(p.x, p.y - tw * 0.16, tw * 0.29, 0, Math.PI * 2);
        ctx.stroke();
        continue;
      }
      const player = a.kind === "player";
      ctx.strokeStyle = player ? "#252e29" : "#293c2d";
      ctx.lineWidth = tw * 0.09;
      ctx.beginPath();
      ctx.moveTo(p.x - 4, p.y - 2);
      ctx.lineTo(p.x - 6, p.y - tw * 0.17);
      ctx.moveTo(p.x + 4, p.y - 2);
      ctx.lineTo(p.x + 6, p.y - tw * 0.17);
      ctx.stroke();
      ctx.fillStyle = player ? "#c68c55" : "#718c59";
      ctx.beginPath();
      ctx.moveTo(p.x - tw * 0.14, p.y - tw * 0.15);
      ctx.lineTo(p.x - tw * 0.12, p.y - tw * 0.42);
      ctx.lineTo(p.x + tw * 0.12, p.y - tw * 0.42);
      ctx.lineTo(p.x + tw * 0.15, p.y - tw * 0.15);
      ctx.closePath();
      ctx.fill();
      ctx.fillStyle = player ? "#e3c29b" : "#a0b47b";
      ctx.beginPath();
      ctx.arc(p.x, p.y - tw * 0.48, tw * 0.105, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = player ? "#3b4334" : "#516e47";
      ctx.fillRect(p.x - tw * 0.1, p.y - tw * 0.58, tw * 0.2, tw * 0.07);
      if (player) {
        ctx.strokeStyle = "#eee0b2";
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.ellipse(p.x, p.y + 2, tw * 0.27, th * 0.24, 0, 0, Math.PI * 2);
        ctx.stroke();
      }
    }
    effects = effects.filter((e) => now - e.start < 600);
    for (const e of effects) {
      const p = project(e.x, e.y),
        progress = (now - e.start) / 600;
      ctx.strokeStyle = `rgba(255,190,100,${1 - progress})`;
      ctx.lineWidth = 5 * (1 - progress) + 1;
      ctx.beginPath();
      ctx.arc(p.x, p.y, tw * (0.2 + progress), 0, Math.PI * 2);
      ctx.stroke();
      ctx.fillStyle = `rgba(255,220,160,${(1 - progress) * 0.5})`;
      ctx.beginPath();
      ctx.arc(p.x, p.y, tw * (1 - progress) * 0.4, 0, Math.PI * 2);
      ctx.fill();
    }
    if (noticeTimer && now > noticeTimer) {
      $("toast").textContent = "Stay sharp out there";
      noticeTimer = 0;
    }
  }
  $("start").addEventListener("click", () => {
    if (mode === "paused") {
      $("overlay").hidden = true;
      setBackgroundInert(false);
      running = true;
      mode = "playing";
      last = 0;
      finish();
    } else startRun();
  });
  $("help").addEventListener("click", () => {
    if (running) {
      mode = "paused";
      setModal(
        "How to<br><em>survive.</em>",
        "Scavenge the marked cache, then get close and blast the infected.",
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
      queue.push(held);
      b.classList.add("pressed");
      b.setPointerCapture(e.pointerId);
    });
    b.addEventListener("pointerup", release);
    b.addEventListener("pointercancel", release);
    b.addEventListener("lostpointercapture", release);
  }
  $("loot").addEventListener("click", () => {
    if (running) queue.push("L");
  });
  $("attack").addEventListener("click", () => {
    if (running) queue.push("A");
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
        queue.push(held);
      }
    } else if (!e.repeat && (k === " " || k === "l")) {
      e.preventDefault();
      queue.push(k === " " ? "A" : "L");
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
  requestAnimationFrame(frame);
})();
