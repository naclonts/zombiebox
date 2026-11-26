/* A deterministic, tile-derived field atlas. The caller caches this canvas. */
(() => {
  "use strict";
  const SIZE = 512,
    SCALE = 2,
    WIDTH = SIZE * SCALE;
  const palette = {
    g: [110, 137, 83],
    f: [179, 156, 91],
    t: [53, 95, 66],
    m: [123, 129, 113],
    r: [51, 132, 139],
    l: [44, 117, 130],
    A: [156, 132, 94],
    s: [69, 77, 70],
    d: [156, 128, 83],
    b: [180, 139, 86],
    v: [163, 152, 122],
    p: [149, 153, 137],
  };
  const isWater = (c) => c === "r" || c === "l";
  const hash = (x, y, salt) => {
    let h = Math.imul(x ^ salt, 0x1f123bb5) ^ Math.imul(y + salt, 0x5f356495);
    h = Math.imul(h ^ (h >>> 15), 0x2c1b3c6d);
    return (h ^ (h >>> 12)) >>> 0;
  };
  const clamp = (n) => Math.max(0, Math.min(255, n));

  function drawBase(canvas, county) {
    canvas.width = canvas.height = WIDTH;
    const ctx = canvas.getContext("2d", { alpha: false });
    const image = ctx.createImageData(WIDTH, WIDTH),
      pixels = image.data;
    const grounds = county.ground || "",
      districts = county.district || "";
    const features = county.features || "",
      heights = county.elevation || "";
    let salt = 2166136261;
    for (const c of String(county.seed ?? "0"))
      salt = Math.imul(salt ^ c.charCodeAt(0), 16777619);
    const groundAt = (x, y) =>
      x < 0 || y < 0 || x >= SIZE || y >= SIZE ? "" : grounds[y * SIZE + x];
    const elevation = (x, y) => {
      if (x < 0 || y < 0 || x >= SIZE || y >= SIZE) return 75;
      return (heights.charCodeAt(y * SIZE + x) || 108) - 33;
    };
    function pixel(x, y, r, g, b) {
      const i = (y * WIDTH + x) * 4;
      pixels[i] = clamp(r);
      pixels[i + 1] = clamp(g);
      pixels[i + 2] = clamp(b);
      pixels[i + 3] = 255;
    }
    for (let y = 0; y < SIZE; y++) {
      for (let x = 0; x < SIZE; x++) {
        const i = y * SIZE + x,
          code = grounds[i] || "g",
          district = districts[i] || "F";
        const feature = features[i] || ".",
          base = palette[code] || palette.g;
        const h = hash(x, y, salt),
          coarse = hash(x >> 2, y >> 2, salt);
        let tone = (h % 7) - 3,
          accent = 0;
        if (code === "g") {
          tone += (coarse % 13) - 6;
          if ((x + 2 * y + (coarse & 3)) % 23 === 0) accent = 8;
        } else if (code === "f") {
          tone += (coarse % 13) - 6;
          if ((x + y * 2) % 9 < 2) tone -= 12;
          if ((x + y * 2) % 9 === 3) accent = 9;
        } else if (code === "t") {
          // Offset small crowns create overlapping, clustered canopy rather
          // than individual randomly scattered pixels.
          const crown = hash((x + ((y >> 2) & 1) * 2) >> 2, y >> 2, salt + 7);
          tone += (crown % 24) - 12;
          const cx = (x + ((y >> 2) & 1) * 2) & 3,
            cy = y & 3;
          if ((cx === 0 || cy === 3) && crown & 1) tone -= 12;
          if (cx === 1 && cy === 1) accent = 16;
        } else if (code === "m") {
          const slope = elevation(x - 1, y - 1) - elevation(x + 1, y + 1);
          tone += Math.max(-24, Math.min(24, slope * 2)) + ((coarse % 15) - 7);
          if ((x + y * 2 + (coarse & 7)) % 17 === 0) accent = 21;
        } else if (isWater(code)) {
          tone += (coarse % 9) - 4;
          if (
            !isWater(groundAt(x - 1, y)) ||
            !isWater(groundAt(x, y - 1)) ||
            !isWater(groundAt(x + 1, y)) ||
            !isWater(groundAt(x, y + 1))
          )
            tone += 25;
          else if ((y + (h % 3)) % 12 === 0 && x % 13 < 5) accent = 22;
        } else if (code === "A") {
          tone += (h % 5) - 2;
          if (
            (x + y) % 12 < 5 &&
            groundAt(x - 2, y) === "A" &&
            groundAt(x + 2, y) === "A" &&
            x % 5 === 0
          )
            accent = 14;
        } else if (code === "s") {
          tone += (h % 5) - 2;
        } else if (code === "d" || code === "v") {
          tone += (h % 13) - 6;
        } else if (code === "b") {
          tone += (x + y) % 3 === 0 ? 20 : -5;
        } else if (code === "p") {
          tone += (h % 9) - 4;
        }
        let r = base[0] + tone,
          g = base[1] + tone,
          b = base[2] + tone;
        // These are actual ground-floor feature cells, including irregular
        // footprints and courtyards, rather than building bounding boxes.
        if (feature === "_" || feature === "o" || feature === "^") {
          const building =
            district === "H"
              ? [190, 188, 163]
              : district === "P"
                ? [145, 158, 151]
                : district === "A"
                  ? [143, 133, 103]
                  : district === "C"
                    ? [167, 131, 105]
                    : [174, 129, 94];
          r = building[0] + tone / 2;
          g = building[1] + tone / 2;
          b = building[2] + tone / 2;
          accent = feature === "^" ? 20 : 0;
        } else if (feature === "#" || feature === "|") {
          r = 55;
          g = 59;
          b = 50;
          accent = feature === "|" ? 12 : 0;
        } else if (feature === "w") {
          r = 116;
          g = 176;
          b = 166;
          accent = 8;
        } else if (feature === "+") {
          r = 228;
          g = 187;
          b = 112;
          accent = 0;
        } else if (feature === "=") {
          r = 109;
          g = 91;
          b = 67;
          accent = 20;
        } else if (feature === "!") {
          r = 238;
          g = 195;
          b = 107;
          accent = 18;
        } else if (feature === "T") {
          r = 41;
          g = 86;
          b = 54;
          accent = 29;
        }
        const px = x * SCALE,
          py = y * SCALE;
        pixel(px, py, r + accent, g + accent, b + accent);
        pixel(px + 1, py, r, g, b);
        pixel(px, py + 1, r, g, b);
        pixel(
          px + 1,
          py + 1,
          r - (accent > 0 ? 3 : 0),
          g - (accent > 0 ? 3 : 0),
          b - (accent > 0 ? 3 : 0),
        );
      }
    }
    ctx.putImageData(image, 0, 0);
  }
  globalThis.CountyMap = { drawBase };
})();
