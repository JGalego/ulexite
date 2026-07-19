#!/usr/bin/env node
// Regenerates src/components/CrystalMesh/mesh.json — a low-poly triangle
// mesh in the logo's own cyan/indigo/purple sweep, used as the hero's
// faceted-crystal background. Not run at build time; its output is
// checked in like any other static asset. Re-run this (and re-check-in
// mesh.json) if you want a different mesh — same seed always reproduces
// today's exact mesh, so this script is documentation as much as a tool.
//
//   node website/scripts/gen-crystal-mesh.mjs > website/src/components/CrystalMesh/mesh.json

function mulberry32(seed) {
  return function () {
    seed |= 0;
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
const rand = mulberry32(1337);

const COLS = 13;
const ROWS = 7;
const W = 1400;
const H = 760;
const cellW = W / COLS;
const cellH = H / ROWS;
const JITTER = 0.34; // fraction of cell size — how far a vertex can wander off-grid

function lerp(a, b, t) {
  return a + (b - a) * t;
}
function hexLerp(c1, c2, t) {
  const p = (h) => [parseInt(h.slice(1, 3), 16), parseInt(h.slice(3, 5), 16), parseInt(h.slice(5, 7), 16)];
  const [r1, g1, b1] = p(c1);
  const [r2, g2, b2] = p(c2);
  const r = Math.round(lerp(r1, r2, t));
  const g = Math.round(lerp(g1, g2, t));
  const b = Math.round(lerp(b1, b2, t));
  return `#${[r, g, b].map((x) => x.toString(16).padStart(2, '0')).join('')}`;
}
// The logo's own sweep: cyan (top-left) -> indigo -> purple (center) -> blue.
const STOPS = ['#67e8f9', '#4f46e5', '#9333ea', '#4338ca', '#0ea5e9'];
function colorAt(u, v) {
  const t = Math.max(0, Math.min(1, u * 0.6 + v * 0.4));
  const seg = t * (STOPS.length - 1);
  const i = Math.min(STOPS.length - 2, Math.floor(seg));
  return hexLerp(STOPS[i], STOPS[i + 1], seg - i);
}

// Shared jittered vertex grid — border vertices stay unjittered so the
// mesh's outer edge is a clean rectangle (no gaps at the hero's own edges).
const pts = [];
for (let r = 0; r <= ROWS; r++) {
  const row = [];
  for (let c = 0; c <= COLS; c++) {
    const jx = (rand() - 0.5) * 2 * JITTER * cellW;
    const jy = (rand() - 0.5) * 2 * JITTER * cellH;
    const border = r === 0 || r === ROWS || c === 0 || c === COLS;
    row.push([
      Math.round((c * cellW + (border ? 0 : jx)) * 10) / 10,
      Math.round((r * cellH + (border ? 0 : jy)) * 10) / 10,
    ]);
  }
  pts.push(row);
}

// Split each grid cell into two triangles, alternating the diagonal
// direction per cell (the standard "low poly" tiling trick) so the mesh
// doesn't read as an obviously repeating pattern.
const triangles = [];
for (let r = 0; r < ROWS; r++) {
  for (let c = 0; c < COLS; c++) {
    const A = pts[r][c];
    const B = pts[r][c + 1];
    const C = pts[r + 1][c];
    const D = pts[r + 1][c + 1];
    const flip = (r + c) % 2 === 0;
    const tris = flip ? [[A, B, C], [B, D, C]] : [[A, B, D], [A, D, C]];
    for (const tri of tris) {
      const cx = (tri[0][0] + tri[1][0] + tri[2][0]) / 3;
      const cy = (tri[0][1] + tri[1][1] + tri[2][1]) / 3;
      triangles.push({pts: tri, u: cx / W, v: cy / H});
    }
  }
}

const data = triangles.map((t) => {
  const points = t.pts.map((p) => `${p[0]},${p[1]}`).join(' ');
  const fill = colorAt(t.u, t.v);
  return [points, fill];
});

process.stdout.write(JSON.stringify({viewBox: `0 0 ${W} ${H}`, triangles: data}));
