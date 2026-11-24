(() => {
  "use strict";

  const GROUND = {
    r: ["#555d53", "#60665a"],
    g: ["#435e48", "#4a674b"],
    p: ["#85887a", "#929383"],
    f: ["#6a6c4d", "#747554"],
    v: ["#6e7869", "#798273"],
  };
  const hash = (x, y) => Math.abs((x * 73 + y * 137 + x * y * 17) % 997);

  function draw(canvas, state, now, effects = []) {
    const ctx = canvas.getContext("2d");
    const rect = canvas.getBoundingClientRect();
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const width = rect.width;
    const height = rect.height;
    const pixelWidth = Math.round(width * dpr);
    const pixelHeight = Math.round(height * dpr);
    if (canvas.width !== pixelWidth || canvas.height !== pixelHeight) {
      canvas.width = pixelWidth;
      canvas.height = pixelHeight;
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.imageSmoothingEnabled = false;
    ctx.fillStyle = "#354b3e";
    ctx.fillRect(0, 0, width, height);
    if (!state || !state.tiles || !state.player) return;

    const tiles = state.tiles;
    const tileWidth = Math.max(32, Math.min(52, width / 8.3));
    const tileHeight = tileWidth / 2;
    const scale = tileWidth / 48;
    const centerX = width / 2;
    const centerY = height * 0.49;
    const house = state.house;
    const houseX = house ? house.x + (house.width - 1) / 2 : state.player.x;
    const houseY = house ? house.y + (house.height - 1) / 2 : state.player.y;
    const distance =
      Math.abs(houseX - state.player.x) + Math.abs(houseY - state.player.y);
    const houseBias = house ? Math.max(0, 1 - distance / 14) * 0.55 : 0;
    const cameraX = state.player.x + (houseX - state.player.x) * houseBias;
    const cameraY = state.player.y + (houseY - state.player.y) * houseBias;
    const project = (x, y) => ({
      x: centerX + ((x - cameraX - (y - cameraY)) * tileWidth) / 2,
      y: centerY + ((x - cameraX + (y - cameraY)) * tileHeight) / 2,
    });
    const visible = (p, margin = tileWidth * 3) =>
      p.x > -margin &&
      p.x < width + margin &&
      p.y > -margin &&
      p.y < height + margin;
    const poly = (points, fill, stroke) => {
      ctx.beginPath();
      ctx.moveTo(points[0].x, points[0].y);
      for (const p of points.slice(1)) ctx.lineTo(p.x, p.y);
      ctx.closePath();
      ctx.fillStyle = fill;
      ctx.fill();
      if (stroke) {
        ctx.strokeStyle = stroke;
        ctx.lineWidth = 1;
        ctx.stroke();
      }
    };
    const diamond = (p, color, inset = 0) =>
      poly(
        [
          { x: p.x, y: p.y - tileHeight / 2 + inset },
          { x: p.x + tileWidth / 2 - inset, y: p.y },
          { x: p.x, y: p.y + tileHeight / 2 - inset },
          { x: p.x - tileWidth / 2 + inset, y: p.y },
        ],
        color,
      );
    const insideHouse =
      house &&
      state.player.x >= house.x &&
      state.player.x < house.x + house.width &&
      state.player.y >= house.y &&
      state.player.y < house.y + house.height;
    const inHouse = (x, y) =>
      house &&
      x >= house.x &&
      x < house.x + house.width &&
      y >= house.y &&
      y < house.y + house.height;

    // Ground is deterministic decoration only; the host owns every traversable tile.
    for (let y = tiles.y; y < tiles.y + tiles.height; y++) {
      for (let x = tiles.x; x < tiles.x + tiles.width; x++) {
        const p = project(x, y);
        if (!visible(p, tileWidth)) continue;
        const type = tiles.ground[(y - tiles.y) * tiles.width + x - tiles.x];
        const variation = hash(x, y);
        const palette = GROUND[type] || GROUND.g;
        diamond(p, palette[variation % 2]);
        if (inHouse(x, y)) {
          diamond(p, (x + y) % 2 ? "#766e5b" : "#807864", 1);
          ctx.strokeStyle = "#544f42";
          ctx.lineWidth = 0.7;
          ctx.beginPath();
          ctx.moveTo(p.x - tileWidth * 0.25, p.y - tileHeight * 0.25);
          ctx.lineTo(p.x + tileWidth * 0.25, p.y + tileHeight * 0.25);
          ctx.stroke();
        } else if (type === "g" || type === "f") {
          ctx.fillStyle = variation % 3 ? "#78916a" : "#9b9c70";
          const dx = ((variation % 11) - 5) * scale;
          ctx.fillRect(p.x + dx, p.y - 2 * scale, scale, 3 * scale);
          ctx.fillRect(p.x + dx + 3 * scale, p.y - scale, scale, 2 * scale);
        } else if (type === "r" && (x + y) % 4 === 0) {
          ctx.strokeStyle = "#c5b99488";
          ctx.lineWidth = 2 * scale;
          ctx.beginPath();
          ctx.moveTo(p.x - 5 * scale, p.y - 2.5 * scale);
          ctx.lineTo(p.x + 5 * scale, p.y + 2.5 * scale);
          ctx.stroke();
        } else if (type === "p" || type === "v") {
          ctx.fillStyle = "#c4c1a64a";
          ctx.fillRect(
            p.x + ((variation % 7) - 3) * scale,
            p.y,
            2 * scale,
            scale,
          );
        }
      }
    }

    if (state.block && state.block.min && state.block.max) {
      const min = state.block.min;
      const max = state.block.max;
      const corners = [
        project(min.x - 0.5, min.y - 0.5),
        project(max.x + 0.5, min.y - 0.5),
        project(max.x + 0.5, max.y + 0.5),
        project(min.x - 0.5, max.y + 0.5),
      ];
      ctx.strokeStyle = "#d3c6a080";
      ctx.lineWidth = 2 * scale;
      ctx.setLineDash([7 * scale, 4 * scale]);
      ctx.beginPath();
      ctx.moveTo(corners[0].x, corners[0].y);
      for (const corner of corners.slice(1)) ctx.lineTo(corner.x, corner.y);
      ctx.closePath();
      ctx.stroke();
      ctx.setLineDash([]);
    }

    function cache(p) {
      ctx.save();
      ctx.translate(p.x, p.y);
      ctx.scale(scale, scale);
      ctx.fillStyle = "#19251b99";
      ctx.beginPath();
      ctx.ellipse(0, 2, 13, 5, 0, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = "#ae7542";
      ctx.fillRect(-9, -17, 18, 15);
      ctx.fillStyle = "#e0b77a";
      ctx.fillRect(-9, -19, 18, 5);
      ctx.fillStyle = "#684b35";
      ctx.fillRect(-2, -19, 4, 17);
      ctx.fillStyle = "#f4deb0";
      ctx.fillRect(-1, -12, 2, 4);
      ctx.strokeStyle = "#e6c481aa";
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.ellipse(0, 0, 16, 8, 0, 0, Math.PI * 2);
      ctx.stroke();
      ctx.restore();
    }

    function actor(p, zombie, armed, index) {
      ctx.save();
      ctx.translate(Math.round(p.x), Math.round(p.y));
      ctx.scale(scale, scale);
      ctx.fillStyle = "#111c1699";
      ctx.beginPath();
      ctx.ellipse(0, 1, 12, 5, 0, 0, Math.PI * 2);
      ctx.fill();
      // Feet and legs anchor each silhouette to the tile.
      ctx.fillStyle = zombie ? "#313e34" : "#252f2c";
      ctx.fillRect(-7, -14, 6, 12);
      ctx.fillRect(2, -14, 6, 12);
      ctx.fillStyle = "#1c2824";
      ctx.fillRect(-9, -4, 8, 4);
      ctx.fillRect(2, -4, 9, 4);
      if (zombie) {
        ctx.fillStyle = index % 2 ? "#71855d" : "#6c8060";
        ctx.fillRect(-9, -29, 18, 17);
        ctx.fillStyle = "#50644b";
        ctx.fillRect(-11, -26, 4, 15);
        ctx.fillRect(7, -26, 5, 13);
        // Uneven reaching arms and ragged hems keep the infected distinct.
        ctx.fillRect(10, -22, 9, 4);
        ctx.fillRect(-13, -19, 4, 7);
        ctx.fillStyle = "#8a9b71";
        ctx.fillRect(17, -22, 4, 4);
        ctx.fillStyle = "#3c5140";
        ctx.fillRect(-8, -14, 4, 4);
        ctx.fillRect(1, -14, 3, 3);
        ctx.fillStyle = "#b0bb8c";
        ctx.fillRect(-7, -40, 14, 12);
        ctx.fillRect(-10, -14, 4, 4);
        ctx.fillStyle = "#3d5441";
        ctx.fillRect(-8, -41, 15, 5);
        ctx.fillStyle = "#d7c49a";
        ctx.fillRect(3, -35, 2, 2);
        ctx.fillStyle = "#455942";
        ctx.fillRect(-4, -23, 3, 8);
      } else {
        ctx.fillStyle = "#c88e58";
        ctx.fillRect(-9, -30, 18, 18);
        ctx.fillStyle = "#8c5c3e";
        ctx.fillRect(-10, -28, 4, 13);
        ctx.fillRect(7, -27, 5, 10);
        ctx.fillStyle = "#e7c4a0";
        ctx.fillRect(-7, -40, 14, 12);
        ctx.fillStyle = "#303d32";
        ctx.fillRect(-8, -42, 15, 5);
        ctx.fillRect(3, -38, 7, 3);
        ctx.fillStyle = "#e7c4a0";
        ctx.fillRect(10, -20, 5, 4);
        ctx.fillStyle = "#e0b77b";
        ctx.fillRect(-7, -27, 3, 12);
        ctx.fillStyle = "#5f4c39";
        ctx.fillRect(-9, -16, 18, 3);
        if (armed) {
          ctx.fillStyle = "#202927";
          ctx.fillRect(13, -23, 14, 5);
          ctx.fillRect(15, -19, 3, 5);
          ctx.fillStyle = "#a3a99a";
          ctx.fillRect(14, -24, 12, 2);
        }
      }
      ctx.restore();
    }

    // Back walls sit behind actors. The door is a gap through the appropriate perimeter tile.
    if (house) {
      const wallHeight = 27 * scale;
      const door = house.door;
      for (let y = house.y; y < house.y + house.height; y++) {
        for (let x = house.x; x < house.x + house.width; x++) {
          if (x !== house.x && y !== house.y) continue;
          const p = project(x, y);
          if (!visible(p)) continue;
          const isDoor = door && x === door.x && y === door.y;
          if (isDoor) {
            diamond(p, "#d4b274", 3 * scale);
            continue;
          }
          const top = { x: p.x, y: p.y - tileHeight / 2 };
          if (y === house.y)
            poly(
              [
                top,
                { x: p.x + tileWidth / 2, y: p.y },
                { x: p.x + tileWidth / 2, y: p.y - wallHeight },
                { x: top.x, y: top.y - wallHeight },
              ],
              "#9a937a",
              "#676653",
            );
          if (x === house.x)
            poly(
              [
                top,
                { x: p.x - tileWidth / 2, y: p.y },
                { x: p.x - tileWidth / 2, y: p.y - wallHeight },
                { x: top.x, y: top.y - wallHeight },
              ],
              "#b0a58a",
              "#79745f",
            );
        }
      }
      if (door) {
        const p = project(door.x, door.y);
        diamond(p, "#dcbd7f", 4 * scale);
        ctx.fillStyle = "#ffe0a655";
        ctx.beginPath();
        ctx.ellipse(
          p.x,
          p.y,
          tileWidth * 0.34,
          tileHeight * 0.27,
          0,
          0,
          Math.PI * 2,
        );
        ctx.fill();
      }
    }

    if (house && insideHouse) {
      // Furniture sits on solid perimeter tiles, leaving the interior path clear.
      const shelf = project(house.x + 2, house.y);
      ctx.fillStyle = "#584b3d";
      ctx.fillRect(
        shelf.x - 11 * scale,
        shelf.y - 25 * scale,
        22 * scale,
        19 * scale,
      );
      ctx.fillStyle = "#a38a67";
      ctx.fillRect(
        shelf.x - 9 * scale,
        shelf.y - 20 * scale,
        18 * scale,
        2 * scale,
      );
      ctx.fillRect(
        shelf.x - 9 * scale,
        shelf.y - 13 * scale,
        18 * scale,
        2 * scale,
      );
      ctx.fillStyle = "#c5ad80";
      ctx.fillRect(
        shelf.x - 6 * scale,
        shelf.y - 25 * scale,
        3 * scale,
        5 * scale,
      );
      ctx.fillRect(
        shelf.x + 2 * scale,
        shelf.y - 18 * scale,
        4 * scale,
        5 * scale,
      );
      const cabinet = project(house.x, house.y + 3);
      ctx.fillStyle = "#6d5c48";
      ctx.fillRect(
        cabinet.x - 10 * scale,
        cabinet.y - 18 * scale,
        20 * scale,
        13 * scale,
      );
      ctx.fillStyle = "#b39d7c";
      ctx.fillRect(
        cabinet.x - 11 * scale,
        cabinet.y - 20 * scale,
        22 * scale,
        3 * scale,
      );
      ctx.fillStyle = "#ded2ad";
      ctx.fillRect(
        cabinet.x - 4 * scale,
        cabinet.y - 24 * scale,
        7 * scale,
        4 * scale,
      );
    }

    const items = [
      ...(tiles.caches || []).map((item) => ({ ...item, kind: "cache" })),
      ...(state.zombies || []).map((item, index) => ({
        ...item,
        kind: "zombie",
        index,
      })),
      { ...state.player, kind: "player" },
    ].sort((a, b) => a.x + a.y - b.x - b.y || a.x - b.x);
    function drawItem(item) {
      const p = project(item.x, item.y);
      if (!visible(p)) return;
      if (item.kind === "cache") cache(p);
      else
        actor(
          p,
          item.kind === "zombie",
          item.kind === "player" && state.armed,
          item.index,
        );
    }

    // Interior actors are hidden by the roof; outside actors stay visible in front.
    for (const item of items) {
      if (insideHouse || inHouse(item.x, item.y)) drawItem(item);
    }

    if (state.aim) {
      const target = project(state.aim.x, state.aim.y);
      ctx.strokeStyle = "#f1c27dcc";
      ctx.lineWidth = 1.5 * scale;
      ctx.beginPath();
      ctx.ellipse(
        target.x,
        target.y + 2 * scale,
        14 * scale,
        7 * scale,
        0,
        0,
        Math.PI * 2,
      );
      ctx.stroke();
    }

    if (house && !insideHouse) {
      // The two front facades leave a full-height opening at the door.
      const wallHeight = 27 * scale;
      for (let y = house.y; y < house.y + house.height; y++) {
        for (let x = house.x; x < house.x + house.width; x++) {
          if (
            x !== house.x + house.width - 1 &&
            y !== house.y + house.height - 1
          )
            continue;
          const p = project(x, y);
          if (!visible(p)) continue;
          const door = house.door && x === house.door.x && y === house.door.y;
          if (door) {
            ctx.strokeStyle = "#e4bc78";
            ctx.lineWidth = 3 * scale;
            ctx.beginPath();
            ctx.moveTo(p.x - tileWidth / 2, p.y - wallHeight);
            ctx.lineTo(p.x - tileWidth / 2, p.y);
            ctx.moveTo(p.x + tileWidth / 2, p.y - wallHeight);
            ctx.lineTo(p.x + tileWidth / 2, p.y);
            ctx.stroke();
            continue;
          }
          const bottom = { x: p.x, y: p.y + tileHeight / 2 };
          if (y === house.y + house.height - 1)
            poly(
              [
                bottom,
                { x: p.x + tileWidth / 2, y: p.y },
                { x: p.x + tileWidth / 2, y: p.y - wallHeight },
                { x: bottom.x, y: bottom.y - wallHeight },
              ],
              "#8c8c78",
              "#666958",
            );
          if (x === house.x + house.width - 1)
            poly(
              [
                bottom,
                { x: p.x - tileWidth / 2, y: p.y },
                { x: p.x - tileWidth / 2, y: p.y - wallHeight },
                { x: bottom.x, y: bottom.y - wallHeight },
              ],
              "#aaa187",
              "#77715d",
            );
          // Small framed windows are painted on solid exterior wall tiles.
          if ((x + y) % 3 === 0) {
            const wx =
              p.x + (y === house.y + house.height - 1 ? 5 : -9) * scale;
            const wy = p.y - 21 * scale;
            ctx.fillStyle = "#d1c5a3";
            ctx.fillRect(wx, wy, 9 * scale, 12 * scale);
            ctx.fillStyle = "#536d67";
            ctx.fillRect(wx + 2 * scale, wy + 2 * scale, 5 * scale, 8 * scale);
            ctx.fillStyle = "#c5b998";
            ctx.fillRect(wx + 4 * scale, wy + 2 * scale, scale, 8 * scale);
          }
        }
      }
      const a = project(house.x - 0.5, house.y - 0.5);
      const b = project(house.x + house.width - 0.5, house.y - 0.5);
      const c = project(
        house.x + house.width - 0.5,
        house.y + house.height - 0.5,
      );
      const d = project(house.x - 0.5, house.y + house.height - 0.5);
      const roofHeight = 28 * scale;
      for (const p of [a, b, c, d]) p.y -= roofHeight;
      const ridge = { x: (a.x + c.x) / 2, y: (a.y + c.y) / 2 - 19 * scale };
      poly([a, b, ridge, d], "#4f5b4e", "#303d35");
      poly([b, c, d, ridge], "#3c4d42", "#303d35");
      // Clipped rows suggest weathered shingles without adding geometry to the map.
      ctx.save();
      ctx.beginPath();
      ctx.moveTo(a.x, a.y);
      ctx.lineTo(b.x, b.y);
      ctx.lineTo(c.x, c.y);
      ctx.lineTo(d.x, d.y);
      ctx.closePath();
      ctx.clip();
      ctx.strokeStyle = "#a1ad9450";
      ctx.lineWidth = 0.8 * scale;
      ctx.beginPath();
      for (let row = -10; row < 28; row++) {
        const yy = ridge.y + row * 6 * scale;
        ctx.moveTo(a.x - tileWidth, yy);
        ctx.lineTo(c.x + tileWidth, yy);
        for (let col = -12; col < 16; col++) {
          const xx = ridge.x + (col * 13 + (row % 2) * 6) * scale;
          ctx.moveTo(xx, yy);
          ctx.lineTo(xx, yy + 3 * scale);
        }
      }
      ctx.stroke();
      ctx.restore();
      ctx.strokeStyle = "#73807188";
      ctx.lineWidth = scale;
      ctx.beginPath();
      ctx.moveTo(a.x, a.y);
      ctx.lineTo(ridge.x, ridge.y);
      ctx.lineTo(c.x, c.y);
      ctx.stroke();
      // A chimney gives the house a recognizable residential silhouette.
      const chimney = project(
        house.x + house.width * 0.35,
        house.y + house.height * 0.27,
      );
      ctx.fillStyle = "#756c5c";
      ctx.fillRect(
        chimney.x - 5 * scale,
        chimney.y - 61 * scale,
        10 * scale,
        18 * scale,
      );
      ctx.fillStyle = "#3c4036";
      ctx.fillRect(
        chimney.x - 6 * scale,
        chimney.y - 63 * scale,
        12 * scale,
        4 * scale,
      );
    }

    if (!insideHouse) {
      for (const item of items) {
        if (!inHouse(item.x, item.y)) drawItem(item);
      }
    }

    for (const effect of effects) {
      const age = now - effect.start;
      if (age < 0 || age > 500) continue;
      if (effect.type === "gunshot" && effect.from && effect.at) {
        const from = project(effect.from.x, effect.from.y);
        const to = project(effect.at.x, effect.at.y);
        const alpha = Math.max(0, 1 - age / 300);
        ctx.strokeStyle = `rgba(255,219,142,${alpha})`;
        ctx.lineWidth = 2 * scale;
        ctx.beginPath();
        ctx.moveTo(from.x + 18 * scale, from.y - 20 * scale);
        ctx.lineTo(to.x, to.y - 18 * scale);
        ctx.stroke();
        ctx.fillStyle = `rgba(255,226,160,${alpha})`;
        ctx.beginPath();
        ctx.arc(
          from.x + 23 * scale,
          from.y - 20 * scale,
          (3 + alpha * 4) * scale,
          0,
          Math.PI * 2,
        );
        ctx.fill();
      } else if (effect.type === "hurt") {
        ctx.fillStyle = `rgba(170,58,44,${(1 - age / 500) * 0.18})`;
        ctx.fillRect(0, 0, width, height);
      }
    }
    const vignette = ctx.createRadialGradient(
      width / 2,
      height / 2,
      height * 0.15,
      width / 2,
      height / 2,
      Math.max(width, height) * 0.7,
    );
    vignette.addColorStop(0, "#12201600");
    vignette.addColorStop(1, "#101c1677");
    ctx.fillStyle = vignette;
    ctx.fillRect(0, 0, width, height);
  }

  window.ZombieboxScene = { draw };
})();
