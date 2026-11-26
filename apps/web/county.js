(() => {
  "use strict";
  const SIZE = 512;
  const $ = (id) => document.getElementById(id);
  const canvas = $("county-map"),
    ctx = canvas.getContext("2d", { alpha: false });
  const params = new URLSearchParams(location.search);
  const rawSeed = params.get("seed")?.trim() ?? "";
  const initialSeed = /^\d+$/.test(rawSeed) ? rawSeed : null;
  const initialActive = params.get("active") === "1";
  const pxParam = Number(params.get("px"));
  const pyParam = Number(params.get("py"));
  const pfParam = Number(params.get("pf") || 0);
  const activePlayer =
    initialActive &&
    params.has("px") &&
    params.has("py") &&
    Number.isFinite(pxParam) &&
    Number.isFinite(pyParam) &&
    Number.isFinite(pfParam)
      ? {
          x: Math.trunc(pxParam),
          y: Math.trunc(pyParam),
          floor: Math.max(0, Math.trunc(pfParam)),
        }
      : null;
  let firstLoad = true;
  const input = $("seed-input"),
    form = $("seed-form");
  const legend = {
    g: ["Grassland", "#526a4a"],
    f: ["Open field", "#a88a4e"],
    t: ["Forest", "#31513c"],
    m: ["Mountain", "#788073"],
    r: ["River", "#3e7881"],
    l: ["Lake", "#37717a"],
    A: ["Arterial", "#b7a477"],
    s: ["Street", "#9f9472"],
    d: ["Trail", "#917b56"],
    b: ["Bridge", "#bea478"],
    v: ["Gravel", "#847e61"],
    p: ["Pavement", "#858576"],
  };
  const districtName = {
    R: "Residential",
    C: "Downtown",
    H: "Hospital grounds",
    P: "Police precinct",
    A: "Army base",
    F: "Country",
    W: "Woods",
  };
  let county = null,
    selected = null,
    showIndex = true,
    view = { zoom: 1, x: 256, y: 256 },
    dragging = false,
    moved = false,
    last = null,
    walk = false,
    player = null,
    floor = 0,
    busy = false;
  let dpr = Math.min(devicePixelRatio || 1, 2),
    cssW = 0,
    cssH = 0;
  const base = document.createElement("canvas");
  base.width = base.height = SIZE;
  const ground = (x, y) => county?.ground?.[y * SIZE + x] || "g";
  const district = (x, y) => county?.district?.[y * SIZE + x] || "R";
  const elevation = (x, y) => county?.elevation?.charCodeAt(y * SIZE + x) || 33;
  const feat = (x, y) => county?.features?.[y * SIZE + x] || ".";
  const isSameActiveRun = () =>
    Boolean(
      activePlayer &&
        county &&
        String(county.seed) === String(initialSeed),
    );
  function buildBase() {
    CountyMap.drawBase(base, county);
  }
  function resize() {
    const r = canvas.getBoundingClientRect();
    if (!r.width || !r.height) return;
    cssW = r.width;
    cssH = r.height;
    dpr = Math.min(devicePixelRatio || 1, 2);
    canvas.width = Math.round(cssW * dpr);
    canvas.height = Math.round(cssH * dpr);
    render();
    if (selected) updateDetail();
  }
  function screenToWorld(clientX, clientY) {
    const r = canvas.getBoundingClientRect(),
      px = clientX - r.left,
      py = clientY - r.top,
      scale = (Math.min(cssW, cssH) * view.zoom) / SIZE,
      ox = (cssW - SIZE * scale) / 2 + (256 - view.x) * scale,
      oy = (cssH - SIZE * scale) / 2 + (256 - view.y) * scale;
    return [Math.floor((px - ox) / scale), Math.floor((py - oy) / scale)];
  }
  function render() {
    if (!county || !cssW || !cssH) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.fillStyle = "#26382d";
    ctx.fillRect(0, 0, cssW, cssH);
    const scale = (Math.min(cssW, cssH) * view.zoom) / SIZE,
      ox = (cssW - SIZE * scale) / 2 + (256 - view.x) * scale,
      oy = (cssH - SIZE * scale) / 2 + (256 - view.y) * scale;
    ctx.imageSmoothingEnabled = false;
    ctx.drawImage(base, ox, oy, SIZE * scale, SIZE * scale);
    for (const l of county.landmarks || []) {
      const X = ox + (l.x + 0.5) * scale,
        Y = oy + (l.y + 0.5) * scale,
        r = Math.max(2, Math.min(9, scale * 2.2));
      ctx.fillStyle = "#f1bc72";
      ctx.beginPath();
      ctx.arc(X, Y, r, 0, Math.PI * 2);
      ctx.fill();
      ctx.strokeStyle = "#2b281e";
      ctx.lineWidth = 1;
      ctx.stroke();
      if (scale > 1.8) {
        ctx.font = `${Math.min(11, scale * 1.8)}px IBM Plex Mono, monospace`;
        ctx.fillStyle = "#f2e9d0";
        ctx.fillText(l.name, X + r + 3, Y + 3);
      }
    }
    const left = (0 - ox) / scale,
      top = (0 - oy) / scale;
    const right = (cssW - ox) / scale,
      bottom = (cssH - oy) / scale;
    for (const b of county.buildings || []) {
      if (b.x + b.w < left || b.y + b.h < top || b.x > right || b.y > bottom)
        continue;
      if (scale > 2.2) {
        const X = ox + (b.x + b.w / 2) * scale,
          Y = oy + b.y * scale;
        ctx.fillStyle = "#e0d2aa";
        ctx.font = `${Math.min(12, scale * 1.3)}px ui-monospace, monospace`;
        ctx.fillText(b.name, X, Y - 2);
      }
    }
    // Project the selected upper floor into the main map, using the same
    // local cells as the cutaway and collision checks.
    const active = currentBuilding();
    if (active && floor > 0) {
      const upper = active.upper[floor - 1];
      if (upper) {
        for (let y = 0; y < active.h; y++)
          for (let x = 0; x < active.w; x++) {
            const code = upper[y * active.w + x];
            ctx.fillStyle = featureColors[code] || "#202a22";
            ctx.fillRect(
              ox + (active.x + x) * scale,
              oy + (active.y + y) * scale,
              Math.ceil(scale),
              Math.ceil(scale),
            );
          }
      }
    }
    for (const l of county.loot || []) {
      if (scale < 2 || l.floor !== floor) continue;
      ctx.fillStyle = "#e7d69c";
      ctx.fillRect(
        ox + (l.x + 0.25) * scale,
        oy + (l.y + 0.25) * scale,
        Math.max(1, scale * 0.5),
        Math.max(1, scale * 0.5),
      );
    }
    for (const z of county.infected || []) {
      if (scale < 2 || z.floor !== floor) continue;
      const X = ox + (z.x + 0.5) * scale,
        Y = oy + (z.y + 0.5) * scale;
      ctx.fillStyle = "#d86e59";
      ctx.beginPath();
      ctx.arc(X, Y, Math.max(1.4, scale * 0.45), 0, Math.PI * 2);
      ctx.fill();
    }
    if (selected) {
      const X = ox + selected.x * scale,
        Y = oy + selected.y * scale;
      ctx.strokeStyle = "#fff0b3";
      ctx.lineWidth = 1.5;
      ctx.strokeRect(X, Y, Math.max(3, scale), Math.max(3, scale));
    }
    if (isSameActiveRun()) {
      const X = ox + (activePlayer.x + 0.5) * scale,
        Y = oy + (activePlayer.y + 0.5) * scale,
        r = Math.max(4, Math.min(10, scale * 0.6));
      ctx.fillStyle = "#47d195";
      ctx.strokeStyle = "#111d16";
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.arc(X, Y, r, 0, Math.PI * 2);
      ctx.fill();
      ctx.stroke();
      ctx.fillStyle = "#ffffff";
      ctx.beginPath();
      ctx.arc(X, Y, Math.max(1.5, r * 0.38), 0, Math.PI * 2);
      ctx.fill();
      if (scale > 1.6) {
        ctx.font = `bold ${Math.max(10, Math.min(12, scale * 1.4))}px ui-monospace, monospace`;
        ctx.fillStyle = "#111d16";
        ctx.fillText("YOU ARE HERE", X + r + 5, Y + 4);
        ctx.fillStyle = "#47d195";
        ctx.fillText("YOU ARE HERE", X + r + 4, Y + 3);
      }
    }
    if (walk && player) {
      const X = ox + (player.x + 0.5) * scale,
        Y = oy + (player.y + 0.5) * scale;
      ctx.fillStyle = "#f3cd8a";
      ctx.strokeStyle = "#172119";
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.arc(X, Y, Math.max(3, scale * 0.43), 0, Math.PI * 2);
      ctx.fill();
      ctx.stroke();
    }
    $("coords").textContent = selected
      ? `${String(selected.x).padStart(3, "0")} / ${String(selected.y).padStart(3, "0")}`
      : "— / —";
  }
  function walkFeature(x, y, floorIndex = 0) {
    if (floorIndex === 0) return feat(x, y);
    const b = (county.buildings || []).find(
      (q) => x >= q.x && y >= q.y && x < q.x + q.w && y < q.y + q.h,
    );
    if (!b) return ".";
    return (
      (b.upper?.[floorIndex - 1] || "")[(y - b.y) * b.w + (x - b.x)] || "."
    );
  }
  function getDistrict(x, y) {
    return districtName[district(x, y)] || "Unsurveyed";
  }
  const at = (list, x, y, r = 0) =>
    list.filter((v) => Math.abs(v.x - x) <= r && Math.abs(v.y - y) <= r);
  function setSelection(x, y, keepIndex = false) {
    if (!county) return;
    if (!keepIndex) showIndex = false;
    x = Math.max(0, Math.min(SIZE - 1, x));
    y = Math.max(0, Math.min(SIZE - 1, y));
    selected = { x, y };
    updateDetail();
  }
  const inside = (b, x, y) =>
    x >= b.x && y >= b.y && x < b.x + b.w && y < b.y + b.h;
  function currentBuilding() {
    if (!selected || showIndex) return null;
    const { x, y } = selected;
    const onSite = county.buildings.find((b) => inside(b, x, y));
    if (onSite || walk) return onSite;
    // Selecting a public entrance opens the building it serves.
    const landmark = county.landmarks.find((l) => l.x === x && l.y === y);
    if (!landmark || ["Lakeshore", "Mountain pass"].includes(landmark.name))
      return null;
    const districtForSite = {
      Residential: "Residential",
      Downtown: "Downtown",
      Hospital: "Hospital",
      "Police station": "Police",
      "Army base": "Army base",
      "Woodland cabin": "Woods",
    };
    return county.buildings
      .filter((b) => b.district === districtForSite[landmark.name])
      .reduce((best, b) => {
        const d = (b.entry[0] - x) ** 2 + (b.entry[1] - y) ** 2;
        return !best || d < best.d ? { b, d } : best;
      }, null)?.b;
  }
  function ecology(items) {
    const counts = new Map();
    for (const { name } of items) counts.set(name, (counts.get(name) || 0) + 1);
    return (
      [...counts].map(([name, n]) => name + " ×" + n).join(" · ") || "None"
    );
  }
  function updateDetail() {
    if (!selected) return;
    const { x, y } = selected,
      dc = district(x, y),
      g = ground(x, y),
      elev = elevation(x, y) - 33;
    const building = currentBuilding();
    floor = walk ? player.floor : Math.min(floor, (building?.floors || 1) - 1);
    const nearLandmarks = at(county.landmarks, x, y, 12);
    const nearBuildings = county.buildings.filter(
      (b) =>
        b.x + b.w >= x - 12 &&
        b.x <= x + 12 &&
        b.y + b.h >= y - 12 &&
        b.y <= y + 12,
    );
    const local = (items) =>
      items.filter(
        (v) =>
          v.floor === floor &&
          (building
            ? inside(building, v.x, v.y)
            : Math.abs(v.x - x) <= 8 && Math.abs(v.y - y) <= 8),
      );
    const nearLoot = local(county.loot),
      nearInfected = local(county.infected);
    const atBuildings = building ? [building] : [];
    $("district-code").textContent = dc;
    $("tile-elevation").textContent = `ELEV ${elev}/63`;
    $("place-type").textContent = showIndex
      ? "LANDMARK INDEX"
      : walk
        ? "WALKER POSITION"
        : "LOCAL DISTRICT";
    $("place-name").textContent = showIndex
      ? "County overview"
      : building?.name || getDistrict(x, y);
    $("place-icon").textContent =
      dc === "H"
        ? "✚"
        : dc === "P"
          ? "⚑"
          : dc === "A"
            ? "⌖"
            : dc === "C"
              ? "▦"
              : "⌂";
    $("place-copy").textContent = showIndex
      ? "Select a landmark to see its approach and floor plan."
      : [getDistrict(x, y), legend[g]?.[0], building ? floor + 1 + "F" : null]
          .filter(Boolean)
          .join(" · ");
    $("building-count").textContent = showIndex
      ? county.buildings.length
      : nearBuildings.length;
    $("landmark-count").textContent = showIndex
      ? county.landmarks.length
      : nearLandmarks.length;
    $("infected-count").textContent = showIndex
      ? county.infected.length
      : nearInfected.length;
    $("ground-label").textContent = legend[g]?.[0] || g;
    $("feature-label").textContent = featureName(walkFeature(x, y, floor));
    $("loot-label").textContent = ecology(nearLoot);
    $("threat-label").textContent = ecology(nearInfected);
    $("site-title").textContent = showIndex ? "LANDMARKS" : "NEARBY PLACES";
    document.querySelector(".floor-card").hidden = !building;
    document.querySelector(".ecology-card").hidden = showIndex;
    const landmarks = showIndex
      ? (county.landmarks || []).map((v) => ({
          name: v.name,
          type: "LANDMARK",
          x: v.x,
          y: v.y,
        }))
      : nearLandmarks.map((v) => ({
          name: v.name,
          type: "LANDMARK",
          x: v.x,
          y: v.y,
        }));
    const sites = showIndex
      ? landmarks.sort((a, b) => distance(a, x, y) - distance(b, x, y))
      : [
          ...landmarks,
          ...nearBuildings.map((v) => ({
            name: v.name,
            type: v.district || "STRUCTURE",
            x: v.x + Math.floor(v.w / 2),
            y: v.y + Math.floor(v.h / 2),
          })),
          ...(atBuildings[0]?.rooms || [])
            .filter((v) => v.floor === floor)
            .map((v) => ({ name: v.name, type: "ROOM", x: v.x, y: v.y })),
          ...nearLoot.map((v) => ({
            name: v.name || "Supply cache",
            type: "SUPPLY",
            x: v.x,
            y: v.y,
          })),
          ...nearInfected.map((v) => ({
            name: v.name || "Infected",
            type: "THREAT",
            x: v.x,
            y: v.y,
          })),
        ]
          .sort((a, b) => distance(a, x, y) - distance(b, x, y))
          .slice(0, 8);
    $("site-radius").textContent = showIndex
      ? `${(county.landmarks || []).length} COUNTY LANDMARKS`
      : "FIELD RADIUS 12";
    $("site-list").innerHTML = sites.length
      ? sites
          .map(
            (v) =>
              `<li><button class="site-target" data-x="${v.x}" data-y="${v.y}" title="Go to ${escapeHtml(v.type)}">${escapeHtml(v.name)}</button><small>${escapeHtml(v.type)} · ${Math.round(distance(v, x, y))} TILES</small></li>`,
          )
          .join("")
      : '<li class="empty">No named places in field radius.</li>';
    $("floor-count").textContent = atBuildings.length
      ? `${atBuildings[0].floors || 1} LEVELS`
      : "NO STRUCTURE";
    renderFloor(atBuildings[0] || null);
    render();
    updateTeleportUI();
  }
  function distance(v, x, y) {
    return Math.hypot(v.x - x, v.y - y);
  }
  function escapeHtml(s) {
    return String(s).replace(
      /[&<>"']/g,
      (c) =>
        ({
          "&": "&amp;",
          "<": "&lt;",
          ">": "&gt;",
          '"': "&quot;",
          "'": "&#39;",
        })[c],
    );
  }
  function featureName(c) {
    return (
      {
        ".": "Open ground",
        _: "Interior floor",
        "#": "Exterior wall",
        "|": "Interior wall",
        w: "Window",
        "+": "Door",
        "^": "Stairs",
        o: "Furnishing",
        "=": "Fence",
        "!": "Streetlamp",
        T: "Tree",
      }[c] || `Feature ${c}`
    );
  }
  const featureColors = {
    "#": "#6f775e",
    "|": "#596950",
    w: "#77a8a1",
    "+": "#d9aa61",
    "^": "#bc925c",
    o: "#a17e55",
    "=": "#687356",
    _: "#293a2e",
    ".": "#202a22",
    T: "#476544",
    "!": "#e0ad5f",
  };

  function renderFloor(building) {
    const picker = $("floor-picker");
    const map = $("floor-map");
    const fctx = map.getContext("2d");
    picker.innerHTML = "";
    if (!building) {
      picker.disabled = true;
      picker.innerHTML = '<option value="0">Ground exterior</option>';
      fctx.clearRect(0, 0, map.width, map.height);
      fctx.fillStyle = "#111a15";
      fctx.fillRect(0, 0, map.width, map.height);
      fctx.fillStyle = "#748277";
      fctx.font = "10px monospace";
      fctx.textAlign = "center";
      fctx.fillText(
        "Select a structure to read its floor plan.",
        map.width / 2,
        map.height / 2,
      );
      return;
    }
    picker.disabled = walk;
    const floors = Math.max(1, building.floors || 1);
    for (let f = 0; f < floors; f++) {
      const option = document.createElement("option");
      option.value = f;
      option.textContent = f === 0 ? "Ground floor" : `${f + 1}F / upper level`;
      picker.append(option);
    }
    picker.value = String(Math.min(floor, floors - 1));
    floor = Number(picker.value);
    const cols = Math.max(1, building.w || 8);
    const rows = Math.max(1, building.h || 6);
    const rect = map.getBoundingClientRect();
    const ratio = Math.min(devicePixelRatio || 1, 2);
    map.width = Math.max(1, Math.floor(rect.width * ratio));
    map.height = Math.max(1, Math.floor(rect.height * ratio));
    fctx.fillStyle = "#111a15";
    fctx.fillRect(0, 0, map.width, map.height);
    const cw = Math.min(map.width / cols, map.height / rows),
      ch = cw;
    fctx.translate((map.width - cw * cols) / 2, (map.height - ch * rows) / 2);
    fctx.textAlign = "left";
    const upper = (building.upper || [])[floor - 1] || "";
    for (let y = 0; y < rows; y++)
      for (let x = 0; x < cols; x++) {
        const code =
          floor === 0
            ? feat(building.x + x, building.y + y)
            : upper[y * cols + x] || ".";
        fctx.fillStyle = featureColors[code] || "#202a22";
        fctx.fillRect(
          Math.floor(x * cw),
          Math.floor(y * ch),
          Math.ceil(cw),
          Math.ceil(ch),
        );
        if (code === "^") {
          fctx.strokeStyle = "#172119";
          fctx.lineWidth = 1;
          for (let step = 1; step < 4; step++) {
            fctx.beginPath();
            fctx.moveTo(x * cw, y * ch + (step * ch) / 4);
            fctx.lineTo((x + 1) * cw, y * ch + (step * ch) / 4);
            fctx.stroke();
          }
        }
      }
    for (const room of building.rooms || []) {
      if (room.floor !== floor) continue;
      const rx = room.x - building.x,
        ry = room.y - building.y;
      if (rx >= 0 && ry >= 0 && rx < cols && ry < rows) {
        fctx.fillStyle = "#e8e3cb";
        fctx.font = `${Math.max(7, Math.min(10, Math.floor(ch * 0.7)))}px monospace`;
        fctx.fillText(
          room.name,
          (rx + 0.15) * cw,
          (ry + 0.75) * ch,
          Math.max(8, room.w * cw - 2),
        );
      }
    }
    for (const item of county.loot || []) {
      if (
        (item.floor || 0) !== floor ||
        item.x < building.x ||
        item.y < building.y ||
        item.x >= building.x + cols ||
        item.y >= building.y + rows
      )
        continue;
      fctx.fillStyle = "#f0d481";
      fctx.fillRect(
        (item.x - building.x + 0.3) * cw,
        (item.y - building.y + 0.3) * ch,
        Math.max(3, cw * 0.4),
        Math.max(3, ch * 0.4),
      );
    }
    for (const item of county.infected || []) {
      if (
        (item.floor || 0) !== floor ||
        item.x < building.x ||
        item.y < building.y ||
        item.x >= building.x + cols ||
        item.y >= building.y + rows
      )
        continue;
      fctx.fillStyle = "#dc7160";
      fctx.beginPath();
      fctx.arc(
        (item.x - building.x + 0.5) * cw,
        (item.y - building.y + 0.5) * ch,
        Math.max(2, Math.min(cw, ch) * 0.35),
        0,
        Math.PI * 2,
      );
      fctx.fill();
    }
    if (selected && inside(building, selected.x, selected.y)) {
      fctx.strokeStyle = "#fff0b3";
      fctx.lineWidth = Math.max(1, ratio);
      fctx.strokeRect(
        (selected.x - building.x) * cw + 0.5,
        (selected.y - building.y) * ch + 0.5,
        Math.max(2, cw - 1),
        Math.max(2, ch - 1),
      );
    }
    if (
      isSameActiveRun() &&
      activePlayer.floor === floor &&
      inside(building, activePlayer.x, activePlayer.y)
    ) {
      const px = (activePlayer.x - building.x + 0.5) * cw;
      const py = (activePlayer.y - building.y + 0.5) * ch;
      const pr = Math.max(3, Math.min(cw, ch) * 0.42);
      fctx.fillStyle = "#47d195";
      fctx.strokeStyle = "#111d16";
      fctx.lineWidth = Math.max(1.5, ratio);
      fctx.beginPath();
      fctx.arc(px, py, pr, 0, Math.PI * 2);
      fctx.fill();
      fctx.stroke();
      fctx.fillStyle = "#ffffff";
      fctx.beginPath();
      fctx.arc(px, py, Math.max(1.2, pr * 0.38), 0, Math.PI * 2);
      fctx.fill();
    }
    fctx.strokeStyle = "#172119";
    fctx.lineWidth = Math.max(1, ratio);
    fctx.strokeRect(0, 0, map.width, map.height);
  }
  function traversable(x, y, f = 0) {
    if (x < 0 || x >= SIZE || y < 0 || y >= SIZE) return false;
    const g = ground(x, y),
      c = walkFeature(x, y, f);
    if (g === "r" || g === "l" || g === "m") return false;
    return f === 0
      ? [".", "_", "+", "^"].includes(c)
      : ["_", "+", "^"].includes(c);
  }
  function nearestTraversable(cx, cy, f, maxR) {
    let best = null;
    for (let dy = -maxR; dy <= maxR; dy++) {
      for (let dx = -maxR; dx <= maxR; dx++) {
        if (dx === 0 && dy === 0) continue;
        const x = cx + dx;
        const y = cy + dy;
        if (!traversable(x, y, f)) continue;
        const d = Math.hypot(dx, dy);
        if (!best || d < best.d) best = { x, y, d };
      }
    }
    return best;
  }
  function getTargetCell() {
    if (walk && player) {
      return { x: player.x, y: player.y, floor: player.floor };
    }
    if (selected) {
      const b = currentBuilding();
      if (b && floor > 0 && !inside(b, selected.x, selected.y)) {
        for (let y = b.y; y < b.y + b.h; y++) {
          for (let x = b.x; x < b.x + b.w; x++) {
            if (traversable(x, y, floor)) return { x, y, floor };
          }
        }
        return {
          x: b.x + Math.floor(b.w / 2),
          y: b.y + Math.floor(b.h / 2),
          floor,
        };
      }
      return { x: selected.x, y: selected.y, floor };
    }
    return {
      x: county?.spawn?.[0] ?? 256,
      y: county?.spawn?.[1] ?? 256,
      floor: 0,
    };
  }
  function resolveTeleport(tx, ty, tf) {
    if (traversable(tx, ty, tf)) {
      return { ok: true, snapped: false, x: tx, y: ty, floor: tf };
    }
    const b = (county?.buildings || []).find((q) => inside(q, tx, ty));
    if (b) {
      const f = Math.min(tf, Math.max(0, (b.floors || 1) - 1));
      let best = null;
      for (let y = b.y; y < b.y + b.h; y++) {
        for (let x = b.x; x < b.x + b.w; x++) {
          if (!traversable(x, y, f)) continue;
          const d = Math.hypot(x - tx, y - ty);
          if (!best || d < best.d) best = { x, y, d };
        }
      }
      if (best) {
        return { ok: true, snapped: true, x: best.x, y: best.y, floor: f };
      }
      if (Array.isArray(b.entry) && b.entry.length >= 2) {
        const [ex, ey] = b.entry;
        if (traversable(ex, ey, 0)) {
          return { ok: true, snapped: true, x: ex, y: ey, floor: 0 };
        }
        const nearEntry = nearestTraversable(ex, ey, 0, 4);
        if (nearEntry) {
          return {
            ok: true,
            snapped: true,
            x: nearEntry.x,
            y: nearEntry.y,
            floor: 0,
          };
        }
      }
    }
    const near = nearestTraversable(tx, ty, 0, 8);
    if (near) {
      return { ok: true, snapped: true, x: near.x, y: near.y, floor: 0 };
    }
    return {
      ok: false,
      reason:
        "Impassable water or mountain — select a road, shore, bridge, or building.",
    };
  }
  function updateTeleportUI() {
    const btn = $("teleport-btn");
    const chip = $("walk-teleport");
    const note = $("teleport-note");
    if (!county) {
      btn.disabled = true;
      chip.disabled = true;
      return;
    }
    const target = getTargetCell();
    const res = resolveTeleport(target.x, target.y, target.floor);
    if (!res.ok) {
      btn.disabled = true;
      chip.disabled = true;
      btn.textContent = "IMPASSABLE TERRAIN";
      chip.textContent = "IMPASSABLE";
      note.textContent = res.reason;
      return;
    }
    btn.disabled = false;
    chip.disabled = false;
    const sameRun = isSameActiveRun();
    btn.textContent = sameRun ? "TELEPORT HERE ↗" : "PLAY AT THIS CELL ↗";
    chip.textContent = sameRun ? "TELEPORT ↗" : "PLAY HERE ↗";
    const preserveText = sameRun
      ? "keeps gear & world edits"
      : `starts run on seed ${county.seed}`;
    note.textContent = res.snapped
      ? `Snaps to walkable (${res.x}, ${res.y}) · ${res.floor + 1}F · ${preserveText}`
      : `Deploy at (${res.x}, ${res.y}) · ${res.floor + 1}F · ${preserveText}`;
  }
  function dispatchTeleport() {
    if (!county) return;
    const target = getTargetCell();
    const res = resolveTeleport(target.x, target.y, target.floor);
    if (!res.ok) return;
    const seedStr = String(county.seed);
    if (
      window.ZombieboxAtlasBridge &&
      typeof window.ZombieboxAtlasBridge.teleport === "function"
    ) {
      window.ZombieboxAtlasBridge.teleport(seedStr, res.x, res.y, res.floor);
      return;
    }
    const q = new URLSearchParams({
      mode: "county",
      seed: seedStr,
      tx: String(res.x),
      ty: String(res.y),
      tf: String(res.floor),
    });
    location.href = `/?${q.toString()}`;
  }
  function move(dx, dy) {
    if (!walk || !player) return;
    const x = player.x + dx,
      y = player.y + dy;
    if (!traversable(x, y, player.floor)) return;
    player.x = x;
    player.y = y;
    view.x = x + 0.5;
    view.y = y + 0.5;
    setSelection(x, y);
  }
  function toggleWalk(force) {
    if (!county) return;
    walk = force ?? !walk;
    $("walk-toggle").textContent = walk ? "END WALK" : "START WALK";
    $("walk-toggle").setAttribute("aria-pressed", String(walk));
    $("walk-pad").classList.toggle("visible", walk);
    if (walk && !player) {
      player = {
        x: county.spawn?.[0] ?? 256,
        y: county.spawn?.[1] ?? 256,
        floor: 0,
      };
    }
    if (walk && player) {
      view.x = player.x + 0.5;
      view.y = player.y + 0.5;
      view.zoom = Math.max(
        view.zoom,
        Math.min(16, (10 * SIZE) / Math.max(1, Math.min(cssW, cssH))),
      );
      setSelection(player.x, player.y);
    } else overview();
    updateTeleportUI();
  }
  function overview() {
    if (!county) return;
    walk = false;
    floor = 0;
    view = { zoom: 1, x: SIZE / 2, y: SIZE / 2 };
    showIndex = true;
    $("walk-pad").classList.remove("visible");
    $("walk-toggle").setAttribute("aria-pressed", "false");
    $("walk-toggle").textContent = "START WALK";
    setSelection(...county.spawn, true);
  }
  $("overview").addEventListener("click", overview);
  function nearestBuilding() {
    if (!selected) return null;
    return (county.buildings || []).find(
      (b) =>
        selected.x >= b.x - 1 &&
        selected.y >= b.y - 1 &&
        selected.x <= b.x + b.w &&
        selected.y <= b.y + b.h,
    );
  }
  function changeFloor(delta) {
    if (
      !walk ||
      !player ||
      walkFeature(player.x, player.y, player.floor) !== "^"
    )
      return;
    const b = (county.buildings || []).find(
      (q) =>
        player.x >= q.x &&
        player.y >= q.y &&
        player.x < q.x + q.w &&
        player.y < q.y + q.h,
    );
    const levels = b?.floors || 1;
    player.floor = Math.max(0, Math.min(levels - 1, player.floor + delta));
    floor = player.floor;
    $("floor-picker").value = String(floor);
    updateDetail();
  }
  function activate() {
    if (!walk || !player) return;
    if (walkFeature(player.x, player.y, player.floor) === "^") {
      const max =
        (county.buildings || []).find(
          (b) =>
            player.x >= b.x &&
            player.y >= b.y &&
            player.x < b.x + b.w &&
            player.y < b.y + b.h,
        )?.floors || 1;
      player.floor = (player.floor + 1) % max;
      floor = player.floor;
      const picker = $("floor-picker");
      picker.value = String(floor);
      updateDetail();
      return;
    }
    const b = nearestBuilding();
    if (b) {
      selected = { x: player.x, y: player.y };
      updateDetail();
    }
  }
  async function load(seed) {
    if (busy) return;
    busy = true;
    $("map-status").textContent = "SURVEYING…";
    $("seed-note").textContent = "Fetching generated county…";
    try {
      const response = await fetch(`/api/county/${encodeURIComponent(seed)}`, {
        headers: { Accept: "application/json" },
      });
      if (!response.ok)
        throw new Error(`Survey unavailable (${response.status})`);
      const data = await response.json();
      if (data.size !== SIZE) throw new Error("Unexpected county dimensions");
      county = data;
      input.value = county.seed;
      params.set("seed", String(county.seed));
      if (String(county.seed) !== String(initialSeed)) {
        params.delete("active");
        params.delete("px");
        params.delete("py");
        params.delete("pf");
      }
      history.replaceState(
        null,
        "",
        `${location.pathname}?${params.toString()}`,
      );
      $("map-seed").textContent = `SEED ${county.seed}`;
      $("version").textContent = county.version ?? "1";
      $("map-status").textContent = "SURVEY COMPLETE";
      $("seed-note").textContent =
        `${(county.landmarks || []).length} landmarks · replay this seed`;
      showIndex = true;
      floor = 0;
      buildBase();
      player = {
        x: county.spawn?.[0] ?? 256,
        y: county.spawn?.[1] ?? 256,
        floor: 0,
      };
      const isInitialActive = firstLoad && isSameActiveRun();
      firstLoad = false;
      if (isInitialActive) {
        walk = false;
        showIndex = false;
        floor = activePlayer.floor;
        view.x = activePlayer.x + 0.5;
        view.y = activePlayer.y + 0.5;
        view.zoom = Math.max(
          2.5,
          Math.min(
            8,
            (14 * SIZE) / Math.max(1, Math.min(cssW || 400, cssH || 400)),
          ),
        );
        setSelection(activePlayer.x, activePlayer.y, false);
      } else {
        overview();
      }
      resize();
    } catch (error) {
      $("map-status").textContent = "SURVEY UNAVAILABLE";
      $("seed-note").textContent =
        error.message + " — try another decimal seed.";
    } finally {
      busy = false;
    }
  }
  form.addEventListener("submit", (e) => {
    e.preventDefault();
    const seed = input.value.trim();
    if (!/^\d+$/.test(seed)) {
      $("seed-note").textContent =
        "Enter a decimal number for the county seed.";
      return;
    }
    toggleWalk(false);
    load(seed);
  });
  $("seed-random").addEventListener("click", () => {
    if (busy) return;
    const seed = String(Math.floor(Math.random() * 1000000000) + 1);
    input.value = seed;
    toggleWalk(false);
    load(seed);
  });
  canvas.addEventListener("pointerdown", (e) => {
    if (!county) return;
    dragging = true;
    moved = false;
    last = { x: e.clientX, y: e.clientY, vx: view.x, vy: view.y };
    canvas.setPointerCapture(e.pointerId);
  });
  canvas.addEventListener("pointermove", (e) => {
    if (dragging && last) {
      const scale = (Math.min(cssW, cssH) * view.zoom) / SIZE;
      if (Math.abs(e.clientX - last.x) + Math.abs(e.clientY - last.y) > 4)
        moved = true;
      if (moved) {
        view.x = last.vx - (e.clientX - last.x) / scale;
        view.y = last.vy - (e.clientY - last.y) / scale;
        render();
      }
    }
  });
  canvas.addEventListener("pointerup", (e) => {
    if (!dragging) return;
    dragging = false;
    if (!moved) {
      const [x, y] = screenToWorld(e.clientX, e.clientY);
      if (x >= 0 && x < SIZE && y >= 0 && y < SIZE) {
        const tilePx = (Math.min(cssW, cssH) * view.zoom) / SIZE;
        const lm = (county.landmarks || [])
          .map((v) => ({ ...v, d: Math.hypot(v.x - x, v.y - y) }))
          .filter((v) => v.d <= Math.max(1, 5 / tilePx))
          .sort((a, b) => a.d - b.d)[0];
        if (lm && !walk) {
          floor = 0;
          setSelection(lm.x, lm.y);
        } else if (
          walk &&
          player &&
          Math.hypot(x - player.x, y - player.y) <= 2
        )
          activate();
        else if (!walk) {
          floor = 0;
          setSelection(x, y);
        }
      }
    }
  });
  canvas.addEventListener(
    "wheel",
    (e) => {
      e.preventDefault();
      if (!county) return;
      const [x, y] = screenToWorld(e.clientX, e.clientY);
      view.zoom = Math.max(
        1,
        Math.min(8, view.zoom * (e.deltaY < 0 ? 1.2 : 0.83)),
      );
      view.x = x;
      view.y = y;
      render();
    },
    { passive: false },
  );
  const directions = {
    w: [0, -1],
    a: [-1, 0],
    s: [0, 1],
    d: [1, 0],
    arrowup: [0, -1],
    arrowleft: [-1, 0],
    arrowdown: [0, 1],
    arrowright: [1, 0],
  };
  document.addEventListener("keydown", (e) => {
    if (!county || /INPUT|SELECT|TEXTAREA/.test(e.target?.tagName)) return;
    const key = e.key.toLowerCase(),
      dir = directions[key];
    if (walk && dir) {
      e.preventDefault();
      move(...dir);
    } else if (walk && key === "e") {
      e.preventDefault();
      activate();
    } else if (walk && (key === "[" || key === "]")) {
      e.preventDefault();
      changeFloor(key === "[" ? -1 : 1);
    } else if (e.target === canvas && dir) {
      e.preventDefault();
      setSelection(selected.x + dir[0], selected.y + dir[1]);
    } else if (
      e.target === canvas &&
      (key === "+" || key === "-" || key === "=")
    ) {
      e.preventDefault();
      view.zoom = Math.max(
        1,
        Math.min(16, view.zoom * (key === "-" ? 0.8 : 1.25)),
      );
      render();
    } else if (
      key === "enter" &&
      !/BUTTON|A/.test(e.target?.tagName) &&
      (walk || e.target === canvas || e.target === $("floor-map"))
    ) {
      e.preventDefault();
      dispatchTeleport();
    }
  });
  $("walk-toggle").addEventListener("click", () => toggleWalk());
  $("walk-pad").addEventListener("click", (e) => {
    const step = e.target.closest("[data-step]");
    const level = e.target.closest("[data-floor]");
    if (step) {
      const [dx, dy] = step.dataset.step.split(",").map(Number);
      move(dx, dy);
    } else if (level) {
      changeFloor(Number(level.dataset.floor));
    } else if (e.target.closest("[data-use]")) {
      activate();
    }
  });
  $("site-list").addEventListener("click", (e) => {
    const target = e.target.closest(".site-target");
    if (!target) return;
    if (walk) toggleWalk(false);
    floor = 0;
    const x = Number(target.dataset.x),
      y = Number(target.dataset.y);
    view.x = x + 0.5;
    view.y = y + 0.5;
    view.zoom = Math.max(
      view.zoom,
      Math.min(8, (5 * SIZE) / Math.max(1, Math.min(cssW, cssH))),
    );
    setSelection(x, y);
  });
  $("floor-map").addEventListener("click", (e) => {
    if (!county || walk) return;
    const building = currentBuilding();
    if (!building) return;
    const map = $("floor-map");
    if (!map.clientWidth || !map.clientHeight) return;
    const cols = Math.max(1, building.w || 8);
    const rows = Math.max(1, building.h || 6);
    const cw = Math.min(map.width / cols, map.height / rows);
    const ch = cw;
    const ox = (map.width - cw * cols) / 2;
    const oy = (map.height - ch * rows) / 2;
    const px = e.offsetX * (map.width / map.clientWidth);
    const py = e.offsetY * (map.height / map.clientHeight);
    const bx = Math.floor((px - ox) / cw);
    const by = Math.floor((py - oy) / ch);
    if (bx < 0 || by < 0 || bx >= cols || by >= rows) return;
    setSelection(building.x + bx, building.y + by, false);
  });
  $("floor-picker").addEventListener("change", (e) => {
    floor = Number(e.target.value);
    updateDetail();
  });
  $("teleport-btn").addEventListener("click", dispatchTeleport);
  $("walk-teleport").addEventListener("click", dispatchTeleport);
  for (const id of ["back-top", "back-bottom"]) {
    $(id)?.addEventListener("click", (e) => {
      if (
        window.ZombieboxAtlasBridge &&
        typeof window.ZombieboxAtlasBridge.close === "function"
      ) {
        e.preventDefault();
        window.ZombieboxAtlasBridge.close();
      }
    });
  }
  new ResizeObserver(resize).observe(canvas);
  window.addEventListener("resize", resize);
  const initial = initialSeed || "42";
  input.value = initial;
  load(initial);
})();
