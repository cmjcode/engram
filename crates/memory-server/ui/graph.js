// Graf proyek bergaya Obsidian: simpul proyek/tag/entitas, sisi tautan [[...]], sebutan, tag, entitas.
// Tampil sebagai bola 3D yang bisa diputar (bawaan) atau bidang 2D. Kanvas 2D + proyeksi perspektif
// dan layout gaya pegas buatan sendiri, tanpa pustaka: UI di-embed ke biner dan harus jalan offline.
// Berkas ini hanya berisi deklarasi (tanpa efek samping saat dimuat) dan memakai helper dari app.js
// (`$`, `api`, `esc`, `icon`, `S`, `wikiLink`, `tagChip`, `bindCites`, `bindTagChips`, `showEntity`,
// `projHref`, `ago`, `copyText`, `toast`) serta `t()` dari i18n.js saat fungsi dipanggil.
"use strict";

const GRAPH_KINDS = [["link", t("Links [[ ]]")], ["mention", t("Mentions")], ["tag", t("Tags")], ["entity", t("Entities")]];
const GRAPH_DEFAULTS = { link: true, mention: true, tag: true, entity: true, orphans: true, mode: "3d", rotate: true, color: "kind", days: 0 };
const GRAPH_DAY_MS = 864e5;

function graphPrefs() {
  try { return { ...GRAPH_DEFAULTS, ...JSON.parse(localStorage.getItem("graph-filters") || "{}") }; } catch (e) { return { ...GRAPH_DEFAULTS }; }
}
function saveGraphPrefs(p) { try { localStorage.setItem("graph-filters", JSON.stringify(p)); } catch (e) { /* storage diblokir */ } }

const graphRadius = (n) => (n.kind === "project" ? 5 + 2.2 * Math.sqrt(n.degree + n.weight / 8) : n.kind === "tag" ? 4 + 1.4 * Math.sqrt(n.degree) : 3 + Math.sqrt(n.degree));
const graphClamp = (v, a, b) => Math.max(a, Math.min(b, v));

// ---------------------------------------------------------------- model
// Simpul & sisi + filter. Posisi (x, y, z) diisi layout; cache layar (sx, sy, sr, depth, fade) diisi proyeksi.
function graphModel(g, prefs) {
  const nodes = g.nodes.map((n) => ({
    ...n, focus: n.id === g.focus, lastMs: n.last_at ? Date.parse(n.last_at) || 0 : 0,
    x: 0, y: 0, z: 0, vx: 0, vy: 0, vz: 0, fixed: false, sx: 0, sy: 0, sr: 0, depth: 0, fade: 1,
  }));
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const edges = g.edges.map((e) => ({ ...e, s: byId.get(e.from), t: byId.get(e.to) })).filter((e) => e.s && e.t);
  // Kelompok warna dari semua sisi tag (bukan yang terlihat), agar warna tetap saat filter tag dimatikan.
  // Proyek bertag banyak memakai tag terpopulernya agar kelompok tetap padu.
  const tagsOf = new Map(), tagSize = new Map();
  for (const e of edges) {
    if (e.kind !== "tag") continue;
    if (!tagsOf.has(e.from)) tagsOf.set(e.from, []);
    tagsOf.get(e.from).push(e.t.label);
    tagSize.set(e.t.label, (tagSize.get(e.t.label) || 0) + 1);
  }
  const bySize = (a, b) => (tagSize.get(b) || 0) - (tagSize.get(a) || 0) || a.localeCompare(b);
  const groupOf = new Map();
  for (const [id, list] of tagsOf) { list.sort(bySize); groupOf.set(id, list[0]); }
  const groups = [...new Set(groupOf.values())].sort(bySize);
  for (const n of nodes) if (n.kind === "tag" && groups.includes(n.label)) groupOf.set(n.id, n.label);

  const m = { nodes, edges, byId, groups, groupOf, tagsOf: (id) => tagsOf.get(id) || [], focus: nodes.find((n) => n.focus) || null, visN: [], visE: [], neigh: new Map(), rank: new Map() };
  m.apply = (p) => {
    const cut = p.days ? Date.now() - p.days * GRAPH_DAY_MS : 0;
    const alive = (n) => n.focus || !cut || n.lastMs >= cut;
    let visE = edges.filter((e) => p[e.kind] && alive(e.s) && alive(e.t));
    const deg = new Map();
    for (const e of visE) { deg.set(e.s.id, (deg.get(e.s.id) || 0) + 1); deg.set(e.t.id, (deg.get(e.t.id) || 0) + 1); }
    m.visN = nodes.filter((n) => {
      if (n.focus) return true;
      if (!alive(n)) return false;
      if (n.kind === "tag") return p.tag && deg.has(n.id);
      if (n.kind === "entity") return p.entity && deg.has(n.id);
      return p.orphans || deg.has(n.id);
    });
    const vis = new Set(m.visN.map((n) => n.id));
    m.visE = visE.filter((e) => vis.has(e.s.id) && vis.has(e.t.id));
    m.neigh = new Map(m.visN.map((n) => [n.id, new Set()]));
    for (const e of m.visE) { m.neigh.get(e.s.id).add(e.t.id); m.neigh.get(e.t.id).add(e.s.id); }
    // Peringkat proyek per derajat: label simpul penting tampil lebih dulu.
    const ranked = m.visN.filter((n) => n.kind === "project").sort((a, b) => m.neigh.get(b.id).size - m.neigh.get(a.id).size || b.weight - a.weight || a.label.localeCompare(b.label));
    m.rank = new Map(ranked.map((n, i) => [n.id, i]));
  };
  m.apply(prefs);
  return m;
}

// Tetangga per jenis sisi (semua sisi, bukan hanya yang terlihat). Hasil: [[judul, [{node, edge}]]].
function graphNeighbourGroups(m, n) {
  const out = (k) => m.edges.filter((e) => e.s === n && e.kind === k).map((e) => ({ node: e.t, edge: e }));
  const inc = (k) => m.edges.filter((e) => e.t === n && e.kind === k).map((e) => ({ node: e.s, edge: e }));
  const groups = n.kind === "project"
    ? [[t("Links to"), out("link")], [t("Linked from"), inc("link")], [t("Mentions"), out("mention")], [t("Mentioned by"), inc("mention")], [t("Tags"), out("tag")], [t("Entities"), out("entity")]]
    : n.kind === "tag" ? [[t("Projects"), inc("tag")]] : [[t("Used by"), inc("entity")]];
  return groups.filter(([, l]) => l.length).map(([h, l]) => [h, l.sort((a, b) => b.edge.weight - a.edge.weight || a.node.label.localeCompare(b.node.label))]);
}

// Alasan dua simpul terhubung, untuk tooltip sisi dan panel detail.
function graphEdgeReason(e) {
  if (e.kind === "link") return t("Linked with [[ ]] ({n}x)", { n: e.weight });
  if (e.kind === "mention") return t("Mentioned in {n} memories", { n: e.weight });
  if (e.kind === "tag") return t("Tagged {tag}", { tag: e.t.label });
  return t("{n} memories mention {name}", { n: e.weight, name: e.t.label });
}

// ---------------------------------------------------------------- gaya
function graphStyle(canvas) {
  const s = getComputedStyle(canvas);
  const v = (k, d) => (s.getPropertyValue(k) || "").trim() || d;
  return {
    project: v("--graph-project", "#0071e3"), tag: v("--graph-tag", "#0d9488"), entity: v("--graph-entity", "#8b5cf6"),
    missing: v("--graph-missing", "#94a3b8"), ungrouped: v("--graph-ungrouped", "#94a3b8"), edge: v("--graph-edge", "rgba(100,116,139,.45)"),
    text: v("--graph-text", "#1d1d1f"), halo: v("--graph-halo", "rgba(255,255,255,.85)"), accent: v("--accent", "#0071e3"),
    globe: v("--graph-globe", "rgba(0,113,227,.06)"), globeLine: v("--graph-globe-line", "rgba(100,116,139,.22)"),
    groupS: v("--graph-group-s", "58%"), groupL: v("--graph-group-l", "44%"),
  };
}
const graphHue = (i) => Math.round((210 + i * 137.508) % 360);
function graphGroupColor(m, c, label) {
  const i = m.groups.indexOf(label);
  return i < 0 ? c.ungrouped : `hsl(${graphHue(i)}, ${c.groupS}, ${c.groupL})`;
}
// Kecerahan per usia memori terakhir (mode warna "Aktivitas").
const GRAPH_AGE = [[7, 1], [30, 0.8], [90, 0.55]];
function graphAgeAlpha(n) {
  if (!n.lastMs) return 0.35;
  const days = (Date.now() - n.lastMs) / GRAPH_DAY_MS;
  for (const [d, a] of GRAPH_AGE) if (days < d) return a;
  return 0.35;
}
function graphNodeColor(m, c, n, mode) {
  if (n.missing) return c.missing;
  if (mode === "tag" && n.kind !== "entity") return graphGroupColor(m, c, m.groupOf.get(n.id));
  return c[n.kind] || c.entity;
}

// ---------------------------------------------------------------- fisika
// Tolak-menolak antar simpul + pegas pada sisi; mengakumulasi kecepatan (3D bila `three`).
function graphForces(m, alpha, three) {
  const N = m.visN, n = N.length;
  for (let i = 0; i < n; i++) {
    const a = N[i];
    for (let j = i + 1; j < n; j++) {
      const b = N[j];
      let dx = a.x - b.x, dy = a.y - b.y, dz = three ? a.z - b.z : 0, d2 = dx * dx + dy * dy + dz * dz;
      if (d2 < 0.01) { dx = (i - j) * 0.1; dy = 0.1; dz = 0; d2 = dx * dx + 0.01; }
      if (d2 > 250000) continue;
      const d = Math.sqrt(d2), f = (1800 * alpha) / d2 / d;
      a.vx += dx * f; a.vy += dy * f; a.vz += dz * f; b.vx -= dx * f; b.vy -= dy * f; b.vz -= dz * f;
    }
  }
  for (const e of m.visE) {
    const L = e.kind === "link" || e.kind === "mention" ? 90 : 55;
    const dx = e.t.x - e.s.x, dy = e.t.y - e.s.y, dz = three ? e.t.z - e.s.z : 0, d = Math.sqrt(dx * dx + dy * dy + dz * dz) || 0.1;
    const k = ((d - L) / d) * 0.06 * alpha * Math.min(3, 1 + Math.log(Math.max(1, e.weight)));
    e.s.vx += dx * k; e.s.vy += dy * k; e.s.vz += dz * k; e.t.vx -= dx * k; e.t.vy -= dy * k; e.t.vz -= dz * k;
  }
}

// Simpan/pulihkan posisi per mode agar berpindah 2D <-> 3D tidak mengacak tata letak.
function graphPositions(m) {
  let saved = null;
  return {
    save() { saved = new Map(m.nodes.map((n) => [n.id, [n.x, n.y, n.z]])); },
    restore() {
      if (!saved) return false;
      for (const n of m.nodes) { const p = saved.get(n.id); if (p) [n.x, n.y, n.z] = p; n.vx = n.vy = n.vz = 0; }
      return true;
    },
  };
}

// ---------------------------------------------------------------- layout 2D
// Bidang datar: layar = dunia * scale + (tx, ty). Semantik sama dengan graf 2D lama.
function layout2d(m, scr) {
  const pos = graphPositions(m);
  const v = { mode: "2d", scale: 1, tx: 0, ty: 0 };
  v.activate = () => {
    if (pos.restore()) return false;
    m.nodes.forEach((n, i) => {
      // Spiral deterministik agar tata letak stabil antar muat.
      const a = i * 2.399963, r = 30 * Math.sqrt(i + 1);
      n.x = n.focus ? 0 : Math.cos(a) * r; n.y = n.focus ? 0 : Math.sin(a) * r; n.z = 0; n.vx = n.vy = n.vz = 0;
    });
    return true;
  };
  v.deactivate = () => pos.save();
  v.refilter = () => {};
  v.tick = (alpha) => {
    graphForces(m, alpha, false);
    for (const a of m.visN) {
      a.vx -= a.x * 0.012 * alpha; a.vy -= a.y * 0.012 * alpha;
      if (a.fixed || a.focus) { a.vx = 0; a.vy = 0; if (!a.fixed) { a.x *= 0.9; a.y *= 0.9; } continue; }
      a.vx *= 0.6; a.vy *= 0.6;
      a.x += graphClamp(a.vx, -40, 40); a.y += graphClamp(a.vy, -40, 40);
    }
  };
  v.fit = () => {
    if (!m.visN.length || !scr.W) return;
    let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
    for (const n of m.visN) { x0 = Math.min(x0, n.x); y0 = Math.min(y0, n.y); x1 = Math.max(x1, n.x); y1 = Math.max(y1, n.y); }
    const pad = 60;
    v.scale = graphClamp(Math.min((scr.W - pad * 2) / Math.max(1, x1 - x0), (scr.H - pad * 2) / Math.max(1, y1 - y0)), 0.2, 2);
    v.tx = scr.W / 2 - ((x0 + x1) / 2) * v.scale; v.ty = scr.H / 2 - ((y0 + y1) / 2) * v.scale;
  };
  v.project = () => {
    for (const n of m.visN) { n.sx = n.x * v.scale + v.tx; n.sy = n.y * v.scale + v.ty; n.sr = graphRadius(n) * v.scale; n.depth = 0; n.fade = 1; }
    return m.visN;
  };
  v.drag = (n, dx, dy) => { n.x += dx / v.scale; n.y += dy / v.scale; };
  v.pan = (dx, dy) => { v.tx += dx; v.ty += dy; };
  v.orbit = v.pan;
  v.spin = () => false;
  v.zoomAt = (f, sx, sy) => {
    const ns = graphClamp(v.scale * f, 0.2, 4);
    v.tx = sx - ((sx - v.tx) * ns) / v.scale; v.ty = sy - ((sy - v.ty) * ns) / v.scale; v.scale = ns;
  };
  v.center = (n) => { v.scale = Math.max(v.scale, 1.2); v.tx = scr.W / 2 - n.x * v.scale; v.ty = scr.H / 2 - n.y * v.scale; };
  v.step = () => false;
  v.globe = () => null;
  return v;
}

// ---------------------------------------------------------------- layout 3D (bola)
// Simpul dikunci di permukaan bola radius R: gaya diproyeksikan ke bidang singgung lalu posisi
// dinormalkan kembali ke R. Kamera di sumbu -z sejauh D = 3R; rotasi yaw (sumbu y) lalu pitch (sumbu x).
function layout3d(m, scr) {
  const pos = graphPositions(m);
  const v = { mode: "3d", scale: 1, tx: 0, ty: 0, yaw: m.focus ? 0 : 0.6, pitch: m.focus ? 0 : -0.3, R: 160 };
  let anim = null;
  const radiusFor = (k) => graphClamp(120 + 9 * Math.sqrt(k), 140, 420);
  const toSphere = (n, R) => { const l = Math.hypot(n.x, n.y, n.z) || 1; n.x *= R / l; n.y *= R / l; n.z *= R / l; };
  v.activate = () => {
    const R = radiusFor(m.visN.length);
    if (pos.restore()) { for (const n of m.nodes) toSphere(n, R); v.R = R; return false; }
    v.R = R;
    // Fibonacci sphere atas simpul terurut id: deterministik dan merata.
    const list = m.nodes.filter((n) => !n.focus).sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
    list.forEach((n, i) => {
      const y = 1 - (2 * (i + 0.5)) / list.length, r = Math.sqrt(Math.max(0, 1 - y * y)), phi = i * 2.399963;
      n.x = Math.cos(phi) * r * R; n.y = y * R; n.z = Math.sin(phi) * r * R; n.vx = n.vy = n.vz = 0;
    });
    if (m.focus) { m.focus.x = 0; m.focus.y = 0; m.focus.z = -R; }
    return true;
  };
  v.deactivate = () => pos.save();
  // Jumlah simpul terlihat berubah: skala ulang bola, jangan acak ulang posisi.
  v.refilter = () => {
    const R = radiusFor(m.visN.length);
    if (Math.abs(R - v.R) < 1) return;
    for (const n of m.nodes) toSphere(n, R);
    v.R = R;
  };
  v.tick = (alpha) => {
    graphForces(m, alpha, true);
    for (const a of m.visN) {
      if (a.fixed || a.focus) { a.vx = a.vy = a.vz = 0; if (a.focus && !a.fixed) { a.x = 0; a.y = 0; a.z = -v.R; } continue; }
      const l = Math.hypot(a.x, a.y, a.z) || 1, nx = a.x / l, ny = a.y / l, nz = a.z / l;
      const dot = a.vx * nx + a.vy * ny + a.vz * nz;
      a.vx = (a.vx - dot * nx) * 0.6; a.vy = (a.vy - dot * ny) * 0.6; a.vz = (a.vz - dot * nz) * 0.6;
      a.x += graphClamp(a.vx, -40, 40); a.y += graphClamp(a.vy, -40, 40); a.z += graphClamp(a.vz, -40, 40);
      toSphere(a, v.R);
    }
  };
  // Siluet bola pada D = 3R kira-kira 1.06R; 14 px untuk simpul terbesar.
  v.fit = () => { if (!scr.W) return; v.tx = 0; v.ty = 0; v.scale = graphClamp((0.85 * Math.min(scr.W, scr.H)) / 2 / (1.06 * v.R + 14), 0.2, 2.5); };
  const rot = (x, y, z) => {
    const cy = Math.cos(v.yaw), sy = Math.sin(v.yaw), cp = Math.cos(v.pitch), sp = Math.sin(v.pitch);
    const x1 = x * cy + z * sy, z1 = -x * sy + z * cy;
    return [x1, y * cp - z1 * sp, y * sp + z1 * cp];
  };
  v.project = () => {
    const D = 3 * v.R, cx = scr.W / 2 + v.tx, cy = scr.H / 2 + v.ty;
    for (const n of m.visN) {
      const [x, y, z] = rot(n.x, n.y, n.z), f = D / (D + z);
      n.sx = cx + x * f * v.scale; n.sy = cy + y * f * v.scale; n.sr = graphRadius(n) * f * v.scale;
      n.depth = z; n.fade = graphClamp(0.3 + 0.7 * (1 - (z + v.R) / (2 * v.R)), 0.3, 1);
    }
    // Belakang ke depan: simpul depan digambar terakhir dan menang saat diuji klik.
    return m.visN.slice().sort((a, b) => b.depth - a.depth);
  };
  // Seret simpul di bidang singgung: sumbu layar dalam ruang dunia = rotasi balik (1,0,0) dan (0,1,0).
  v.drag = (n, dx, dy) => {
    const cy = Math.cos(v.yaw), sy = Math.sin(v.yaw), cp = Math.cos(v.pitch), sp = Math.sin(v.pitch);
    const D = 3 * v.R, k = (D + n.depth) / D / v.scale;
    n.x += (cy * dx + sp * sy * dy) * k; n.y += cp * dy * k; n.z += (sy * dx - sp * cy * dy) * k;
    toSphere(n, v.R);
  };
  v.pan = (dx, dy) => { v.tx += dx; v.ty += dy; };
  v.orbit = (dx, dy) => { anim = null; v.yaw -= dx * 0.006; v.pitch = graphClamp(v.pitch + dy * 0.006, -1.45, 1.45); };
  v.spin = () => { v.yaw -= 0.0025; return true; };
  v.zoomAt = (f, sx, sy) => {
    const ns = graphClamp(v.scale * f, 0.2, 4), ox = scr.W / 2 + v.tx, oy = scr.H / 2 + v.ty;
    v.tx = sx - scr.W / 2 - ((sx - ox) * ns) / v.scale; v.ty = sy - scr.H / 2 - ((sy - oy) * ns) / v.scale; v.scale = ns;
  };
  // Putar bola agar simpul menghadap kamera (kutub depan), beranimasi ~350 ms.
  v.center = (n, instant) => {
    const yaw1 = Math.atan2(n.x, -n.z), pitch1 = graphClamp(Math.atan2(-n.y, Math.hypot(n.x, n.z)), -1.45, 1.45);
    if (instant) { v.yaw = yaw1; v.pitch = pitch1; v.tx = 0; v.ty = 0; anim = null; return; }
    const dy = Math.atan2(Math.sin(yaw1 - v.yaw), Math.cos(yaw1 - v.yaw));
    anim = { t0: performance.now(), y0: v.yaw, dy, p0: v.pitch, dp: pitch1 - v.pitch, x0: v.tx, y0t: v.ty };
  };
  v.step = (now) => {
    if (!anim) return false;
    const k = graphClamp((now - anim.t0) / 350, 0, 1), e = k * k * (3 - 2 * k);
    v.yaw = anim.y0 + anim.dy * e; v.pitch = anim.p0 + anim.dp * e; v.tx = anim.x0 * (1 - e); v.ty = anim.y0t * (1 - e);
    if (k >= 1) anim = null;
    return true;
  };
  v.globe = () => ({ x: scr.W / 2 + v.tx, y: scr.H / 2 + v.ty, r: 1.06 * v.R * v.scale });
  return v;
}

// ---------------------------------------------------------------- render (ruang layar, kedua mode)
function drawGraph(ctx, m, v, order, c, ui, W, H) {
  ctx.clearRect(0, 0, W, H);
  const hot = ui.hover || ui.selected;
  const lit = hot ? m.neigh.get(hot.id) || new Set() : null;
  const matches = (n) => ui.query && n.label.toLowerCase().includes(ui.query);
  const dim = (n) => (lit && n !== hot && !lit.has(n.id)) || (ui.query && !matches(n) && n !== hot) || (ui.group && m.groupOf.get(n.id) !== ui.group && n !== hot);
  const g = v.globe();
  if (g && g.r > 0) {
    // Bola samar sebagai latar agar bentuk 3D terbaca.
    const grad = ctx.createRadialGradient(g.x - g.r * 0.35, g.y - g.r * 0.4, g.r * 0.1, g.x, g.y, g.r);
    grad.addColorStop(0, c.globe); grad.addColorStop(1, "rgba(0,0,0,0)");
    ctx.globalAlpha = 1; ctx.fillStyle = grad;
    ctx.beginPath(); ctx.arc(g.x, g.y, g.r, 0, Math.PI * 2); ctx.fill();
    ctx.lineWidth = 1; ctx.strokeStyle = c.globeLine; ctx.setLineDash([]); ctx.stroke();
  }
  const edges = v.mode === "3d" ? m.visE.slice().sort((a, b) => b.s.depth + b.t.depth - a.s.depth - a.t.depth) : m.visE;
  const zw = graphClamp(Math.sqrt(v.scale), 0.6, 1.6);
  for (const e of edges) {
    const on = (hot && (e.s === hot || e.t === hot)) || e === ui.hoverEdge;
    const base = (hot || ui.query || ui.group) && !on ? 0.12 : e.kind === "link" || e.kind === "mention" ? 0.9 : 0.55;
    ctx.globalAlpha = base * Math.min(e.s.fade, e.t.fade);
    ctx.strokeStyle = on ? c.accent : c.edge;
    ctx.lineWidth = Math.min(4, 0.8 + Math.log(1 + e.weight) * 0.7) * zw * (e === ui.hoverEdge ? 1.8 : 1);
    ctx.setLineDash(e.kind === "mention" ? [5, 4] : e.kind === "tag" ? [1.5, 3] : []);
    const dx = e.t.sx - e.s.sx, dy = e.t.sy - e.s.sy, d = Math.hypot(dx, dy) || 1, rt = e.t.sr + 2;
    if (d <= rt) continue;
    const ex = e.t.sx - (dx / d) * rt, ey = e.t.sy - (dy / d) * rt;
    ctx.beginPath(); ctx.moveTo(e.s.sx, e.s.sy); ctx.lineTo(ex, ey); ctx.stroke();
    if (e.kind === "link" || e.kind === "mention") {
      // Panah arah tautan, ukuran tetap di layar.
      const ah = 7, ang = Math.atan2(dy, dx);
      ctx.setLineDash([]); ctx.fillStyle = on ? c.accent : c.edge;
      ctx.beginPath(); ctx.moveTo(ex, ey);
      ctx.lineTo(ex - ah * Math.cos(ang - 0.4), ey - ah * Math.sin(ang - 0.4));
      ctx.lineTo(ex - ah * Math.cos(ang + 0.4), ey - ah * Math.sin(ang + 0.4));
      ctx.closePath(); ctx.fill();
    }
  }
  ctx.setLineDash([]);
  for (const n of order) {
    const r = n.sr, col = graphNodeColor(m, c, n, ui.color);
    ctx.globalAlpha = (dim(n) ? 0.18 : 1) * n.fade * (ui.color === "activity" ? graphAgeAlpha(n) : 1);
    ctx.beginPath(); ctx.arc(n.sx, n.sy, r, 0, Math.PI * 2);
    if (n.missing) { ctx.lineWidth = 1.5; ctx.strokeStyle = col; ctx.stroke(); }
    else {
      ctx.fillStyle = col; ctx.fill();
      if (v.mode === "3d" && r > 2.5) {
        // Kilau kecil di kiri atas agar simpul tampak seperti bola.
        const sh = ctx.createRadialGradient(n.sx - r * 0.35, n.sy - r * 0.35, r * 0.1, n.sx, n.sy, r);
        sh.addColorStop(0, "rgba(255,255,255,0.55)"); sh.addColorStop(1, "rgba(255,255,255,0)");
        ctx.fillStyle = sh; ctx.fill();
      }
    }
    if (n.focus || n === ui.hover || n === ui.selected || matches(n)) {
      ctx.globalAlpha = Math.max(0.6, n.fade);
      ctx.lineWidth = n === ui.selected ? 2.5 : 2; ctx.strokeStyle = c.accent;
      ctx.beginPath(); ctx.arc(n.sx, n.sy, r + 3, 0, Math.PI * 2); ctx.stroke();
    }
  }
  // Label: yang disorot selalu tampil; selain itu proyek penting di sisi depan, dengan peredam tumpukan.
  ctx.font = "12px -apple-system, BlinkMacSystemFont, system-ui, sans-serif";
  ctx.textAlign = "center"; ctx.textBaseline = "top"; ctx.lineJoin = "round";
  const K = Math.round(12 + 30 * v.scale), taken = new Set();
  const forced = (n) => n === hot || (lit && lit.has(n.id)) || matches(n) || n.focus;
  const front = (n) => v.mode === "2d" || n.depth < 0;
  const wanted = (n) => n.kind === "project" ? (v.mode === "2d" ? v.scale >= 0.45 : front(n) && (m.rank.get(n.id) ?? 1e9) < K) : v.scale >= 1.4 && front(n);
  const labels = order.filter((n) => forced(n) || wanted(n)).sort((a, b) => (forced(b) ? 1 : 0) - (forced(a) ? 1 : 0) || a.depth - b.depth || (m.rank.get(a.id) ?? 1e9) - (m.rank.get(b.id) ?? 1e9));
  for (const n of labels) {
    const sx = n.sx, sy = n.sy + n.sr + 4;
    if (sx < -100 || sx > W + 100 || sy < -20 || sy > H + 20) continue;
    const key = `${Math.round(sx / 70)}:${Math.round(sy / 16)}`;
    if (taken.has(key) && !forced(n)) continue;
    taken.add(key);
    ctx.globalAlpha = dim(n) ? 0.25 : Math.max(0.55, n.fade);
    const label = n.label.length > 32 ? n.label.slice(0, 31) + "..." : n.label;
    ctx.lineWidth = 3; ctx.strokeStyle = c.halo; ctx.strokeText(label, sx, sy);
    ctx.fillStyle = n.missing ? c.missing : c.text; ctx.fillText(label, sx, sy);
  }
  ctx.globalAlpha = 1;
}

// Uji klik ruang layar: urutan belakang->depan, jadi cek dari akhir (depan) dulu.
function hitNode(order, px, py) {
  for (let i = order.length - 1; i >= 0; i--) { const n = order[i]; if (Math.hypot(px - n.sx, py - n.sy) < n.sr + 4) return n; }
  return null;
}
function hitEdge(m, px, py) {
  let best = null, bd = 5;
  for (const e of m.visE) {
    const ax = e.s.sx, ay = e.s.sy, bx = e.t.sx, by = e.t.sy, l2 = (bx - ax) ** 2 + (by - ay) ** 2;
    if (!l2) continue;
    const k = graphClamp(((px - ax) * (bx - ax) + (py - ay) * (by - ay)) / l2, 0, 1);
    const d = Math.hypot(px - ax - k * (bx - ax), py - ay - k * (by - ay));
    if (d < bd) { bd = d; best = e; }
  }
  return best;
}

// ---------------------------------------------------------------- input
// Seret latar: putar (3D) atau geser (2D); Shift/tombol tengah/dua jari: geser; roda/cubit: zoom;
// ketuk: pilih; ketuk ganda: buka. Penangan `h` disediakan mountGraph.
function bindGraphInput(canvas, h) {
  const pts = new Map();
  let drag = null, pinch = null, lastTap = { t: 0, id: null };
  const pos = (e) => { const r = canvas.getBoundingClientRect(); return [e.clientX - r.left, e.clientY - r.top]; };
  const pair = () => { const [a, b] = [...pts.values()]; return { d: Math.hypot(a.x - b.x, a.y - b.y) || 1, mx: (a.x + b.x) / 2, my: (a.y + b.y) / 2 }; };
  canvas.onpointerdown = (e) => {
    const [x, y] = pos(e);
    pts.set(e.pointerId, { x, y });
    try { canvas.setPointerCapture(e.pointerId); } catch (err) { /* pointer sudah lepas */ }
    h.touched();
    if (pts.size === 2) { if (drag && drag.n) h.dragEnd(drag.n); drag = null; pinch = pair(); return; }
    const n = h.hit(x, y);
    drag = { n, x0: x, y0: y, x, y, moved: false, pan: e.shiftKey || e.button === 1 };
    if (n) h.dragStart(n);
  };
  canvas.onpointermove = (e) => {
    const [x, y] = pos(e);
    if (pts.has(e.pointerId)) pts.set(e.pointerId, { x, y });
    if (pinch && pts.size >= 2) {
      const p = pair();
      h.zoom(p.d / pinch.d, p.mx, p.my); h.pan(p.mx - pinch.mx, p.my - pinch.my);
      pinch = p;
      return;
    }
    if (drag) {
      if (!drag.moved && Math.hypot(x - drag.x0, y - drag.y0) > 4) drag.moved = true;
      if (drag.moved) {
        const dx = x - drag.x, dy = y - drag.y;
        if (drag.n) h.drag(drag.n, dx, dy); else if (drag.pan) h.pan(dx, dy); else h.orbit(dx, dy);
        h.hover(null, null, x, y);
      }
      drag.x = x; drag.y = y;
      return;
    }
    if (e.pointerType === "touch") return;
    const n = h.hit(x, y);
    canvas.style.cursor = n ? "pointer" : "grab";
    h.hover(n, n ? null : h.hitEdge(x, y), x, y);
  };
  canvas.onpointerup = (e) => {
    pts.delete(e.pointerId);
    try { canvas.releasePointerCapture(e.pointerId); } catch (err) { /* abaikan */ }
    if (pinch) { if (pts.size < 2) pinch = null; return; }
    if (!drag) return;
    const d = drag; drag = null;
    if (d.n) h.dragEnd(d.n);
    if (d.moved) return;
    const now = Date.now(), dbl = d.n && lastTap.id === d.n.id && now - lastTap.t < 350;
    lastTap = { t: dbl ? 0 : now, id: d.n ? d.n.id : null };
    if (dbl) h.open(d.n); else h.tap(d.n);
  };
  canvas.onpointercancel = (e) => { pts.delete(e.pointerId); if (drag && drag.n) h.dragEnd(drag.n); drag = null; pinch = null; };
  canvas.onpointerleave = () => { if (!drag) h.hover(null, null); };
  canvas.addEventListener("wheel", (e) => { e.preventDefault(); const [x, y] = pos(e); h.touched(); h.zoom(Math.exp(-e.deltaY * 0.0015), x, y); }, { passive: false });
  // WebKit (Tauri macOS/iOS): cegah zoom halaman saat mencubit kanvas.
  canvas.addEventListener("gesturestart", (e) => e.preventDefault());
  canvas.onkeydown = (e) => h.key(e);
}

// ---------------------------------------------------------------- orkestrator
// Simulasi + render + interaksi. `prefs` dibagi dengan panel (filter, mode, warna, aktivitas).
// `hooks`: onSelect(node|null), onOpen(node), onKey(key) untuk tombol pintas yang mengubah toolbar.
function mountGraph(canvas, g, prefs, tip, hooks = {}) {
  const reduce = !!(window.matchMedia && matchMedia("(prefers-reduced-motion: reduce)").matches);
  const m = graphModel(g, prefs);
  const scr = { W: 0, H: 0, dpr: 1 };
  const layouts = { "2d": layout2d(m, scr), "3d": layout3d(m, scr) };
  let v = layouts[prefs.mode === "2d" ? "2d" : "3d"];
  v.activate();
  const ui = { hover: null, hoverEdge: null, selected: null, query: "", group: "", color: prefs.color };
  let alpha = 1, raf = 0, everConnected = false, userMoved = false, interacted = false, destroyed = false, order = m.visN;
  let style = graphStyle(canvas);

  const label = () => canvas.setAttribute("aria-label", t("Graph of {projects} projects and {edges} edges. A text list is available below.", { projects: m.visN.filter((n) => n.kind === "project").length, edges: m.visE.length }));
  label();
  const draw = () => {
    if (!scr.W || destroyed) return;
    style = graphStyle(canvas);
    order = v.project();
    const ctx = canvas.getContext("2d");
    ctx.setTransform(scr.dpr, 0, 0, scr.dpr, 0, 0);
    drawGraph(ctx, m, v, order, style, ui, scr.W, scr.H);
  };
  const rotating = () => prefs.rotate && v.mode === "3d" && !m.focus && !interacted && !reduce && !document.hidden;
  const settleTicks = (limit) => { let i = 0; while (alpha > 0.02 && i++ < limit) { v.tick(alpha); alpha *= 0.985; } };
  const loop = () => {
    raf = 0;
    if (destroyed) return;
    if (!canvas.isConnected) { if (everConnected) { api_.destroy(); return; } raf = requestAnimationFrame(loop); return; }
    everConnected = true;
    let more = false;
    if (alpha > 0.02) {
      const t0 = performance.now();
      do { v.tick(alpha); alpha *= 0.985; } while (alpha > 0.02 && performance.now() - t0 < 10);
      if (!userMoved) v.fit();
      more = true;
    }
    if (rotating() && v.spin()) more = true;
    if (v.step(performance.now())) more = true;
    draw();
    if (more) raf = requestAnimationFrame(loop);
  };
  const request = () => { if (!raf && !destroyed) raf = requestAnimationFrame(loop); };
  const kick = (a = 0.5) => {
    alpha = Math.max(alpha, a);
    if (reduce) { settleTicks(600); if (!userMoved) v.fit(); draw(); } else request();
  };
  const resize = () => {
    const r = canvas.getBoundingClientRect();
    if (!r.width) return;
    const first = !scr.W;
    scr.W = r.width; scr.H = r.height; scr.dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(scr.W * scr.dpr); canvas.height = Math.round(scr.H * scr.dpr);
    if (first || !userMoved) v.fit();
    draw();
  };
  const ro = new ResizeObserver(resize);
  ro.observe(canvas);
  const onVisible = () => { if (!document.hidden) request(); };
  document.addEventListener("visibilitychange", onVisible);
  if (reduce) settleTicks(600); else request();

  // --- interaksi
  const kindLabel = { project: t("project"), tag: t("tag"), entity: t("entity") };
  const showTip = (n, e, sx, sy) => {
    if (!n && !e) { tip.hidden = true; return; }
    if (n) {
      const extra = n.kind === "project"
        ? (n.missing ? t("no memory yet") : t("{n} memories · {neighbors} neighbors", { n: n.weight, neighbors: m.neigh.get(n.id)?.size || 0 }))
        : `${n.entity_kind ? n.entity_kind + " · " : ""}${t("{n} projects", { n: n.weight })}`;
      const when = n.kind === "project" && n.last_at ? ` · ${esc(ago(n.last_at))}` : "";
      tip.innerHTML = `<b>${esc(n.label)}</b><br><span class="muted">${esc(kindLabel[n.kind] || n.kind)} · ${esc(extra)}${when}</span>`;
    } else {
      tip.innerHTML = `<b>${esc(e.s.label)}</b> &rarr; <b>${esc(e.t.label)}</b><br><span class="muted">${esc(graphEdgeReason(e))}</span>`;
    }
    tip.style.left = `${Math.min(sx + 14, scr.W - 220)}px`; tip.style.top = `${Math.min(sy + 14, scr.H - 50)}px`; tip.hidden = false;
  };
  const open = (n) => {
    if (hooks.onOpen) return hooks.onOpen(n);
    if (n.kind === "project") location.hash = projHref(n.label);
    else if (n.kind === "tag") { localStorage.setItem("proj-tag", n.label.replace(/^#/, "")); location.hash = "#proyek"; }
    else if (n.entity_id) showEntity(n.entity_id);
  };
  const select = (n, center = true) => {
    if (n && !m.neigh.has(n.id)) n = null;
    ui.selected = n;
    if (n && center) { interacted = true; userMoved = true; v.center(n, reduce); }
    if (hooks.onSelect) hooks.onSelect(n);
    draw(); request();
  };
  const cycleList = () => m.visN.slice().sort((a, b) => m.neigh.get(b.id).size - m.neigh.get(a.id).size || a.label.localeCompare(b.label));
  const cycle = (dir) => {
    const list = cycleList();
    if (!list.length) return;
    const i = ui.selected ? list.indexOf(ui.selected) : -1;
    select(list[(i + dir + list.length) % list.length]);
  };
  const zoomAt = (f, sx = scr.W / 2, sy = scr.H / 2) => { v.zoomAt(f, sx, sy); userMoved = true; draw(); };
  const refit = () => { userMoved = false; v.fit(); draw(); };
  bindGraphInput(canvas, {
    touched: () => { interacted = true; },
    hit: (x, y) => hitNode(order, x, y),
    hitEdge: (x, y) => hitEdge(m, x, y),
    hover: (n, e, x, y) => {
      if (n !== ui.hover || e !== ui.hoverEdge) { ui.hover = n; ui.hoverEdge = e; draw(); }
      showTip(n, e, x, y);
    },
    dragStart: (n) => { n.fixed = true; },
    drag: (n, dx, dy) => { userMoved = true; v.drag(n, dx, dy); kick(0.3); draw(); },
    dragEnd: (n) => { n.fixed = false; },
    orbit: (dx, dy) => { userMoved = v.mode === "2d" || userMoved; v.orbit(dx, dy); draw(); },
    pan: (dx, dy) => { userMoved = true; v.pan(dx, dy); draw(); },
    zoom: (f, x, y) => zoomAt(f, x, y),
    tap: (n) => select(n, v.mode === "3d"),
    open,
    key: (e) => {
      const k = e.key;
      if (k === "+" || k === "=") zoomAt(1.25);
      else if (k === "-") zoomAt(0.8);
      else if (k === "0") refit();
      else if (k === "ArrowRight" || k === "ArrowDown") cycle(1);
      else if (k === "ArrowLeft" || k === "ArrowUp") cycle(-1);
      else if (k === "Enter") { if (ui.selected) open(ui.selected); else cycle(1); }
      else if (k === "Escape") { if (!ui.selected) return; select(null); }
      else if ((k === "/" || k === "r" || k === "m") && hooks.onKey) hooks.onKey(k);
      else return;
      e.preventDefault(); e.stopPropagation();
    },
  });

  const refilter = () => {
    m.apply(prefs);
    v.refilter();
    if (ui.hover && !m.neigh.has(ui.hover.id)) ui.hover = null;
    if (ui.selected && !m.neigh.has(ui.selected.id)) select(null, false);
    label();
    kick(0.6);
    draw();
  };
  const api_ = {
    model: m,
    view: () => v,
    nodes: () => m.visN,
    setFilters(f) { Object.assign(prefs, f); refilter(); },
    setDays(d) { prefs.days = d; refilter(); },
    setMode(mode) {
      if (mode !== "2d" && mode !== "3d") return;
      prefs.mode = mode;
      if (v.mode === mode) return;
      v.deactivate();
      v = layouts[mode];
      const fresh = v.activate();
      userMoved = false; v.fit();
      kick(fresh ? 1 : 0.2);
      if (ui.selected) v.center(ui.selected, true);
      draw();
    },
    setRotate(b) { prefs.rotate = !!b; if (b) interacted = false; request(); },
    setColor(c) { prefs.color = c; ui.color = c; draw(); },
    setGroup(gr) { ui.group = ui.group === gr ? "" : gr || ""; draw(); return ui.group; },
    zoom(k) { if (k === "in") zoomAt(1.25); else if (k === "out") zoomAt(0.8); else refit(); },
    search(s) { ui.query = s.trim().toLowerCase(); draw(); },
    focusMatch() { const n = m.visN.find((x) => x.label.toLowerCase().includes(ui.query)); if (ui.query && n) select(n); },
    select(id) { const n = typeof id === "string" ? m.byId.get(id) : id; if (n && !m.neigh.has(n.id)) return false; select(n || null); return true; },
    selected: () => ui.selected,
    cycle,
    open,
    hitTest: (x, y) => hitNode(order, x, y),
    colorOf: (id) => { const n = m.byId.get(id); return n ? graphNodeColor(m, style, n, ui.color) : ""; },
    groupColor: (gr) => graphGroupColor(m, style, gr),
    style: () => style,
    redraw: draw,
    destroy() {
      if (destroyed) return;
      destroyed = true;
      if (raf) cancelAnimationFrame(raf);
      raf = 0;
      ro.disconnect();
      document.removeEventListener("visibilitychange", onVisible);
    },
  };
  return api_;
}

// ---------------------------------------------------------------- panel detail & legenda
function graphNodeLink(n) {
  return n.kind === "project" ? wikiLink(n.label) : n.kind === "tag" ? tagChip(n.label.replace(/^#/, "")) : `<a class="cite" data-entity="${esc(n.entity_id)}">${esc(n.label)}</a>`;
}

function graphNodePanel(aside, n, ctl, ctx) {
  if (!n) { aside.hidden = true; aside.innerHTML = ""; return; }
  const m = ctl.model;
  const kindLabel = { project: t("Project"), tag: t("Tag"), entity: t("Entity") }[n.kind] || n.kind;
  let facts;
  if (n.kind === "project") {
    facts = n.missing ? t("No memory for this project yet")
      : [t("{n} memories", { n: n.weight }), t("{n} neighbors", { n: m.neigh.get(n.id)?.size || 0 }), n.last_at ? t("last active {when}", { when: ago(n.last_at) }) : ""].filter(Boolean).join(" · ");
  } else if (n.kind === "tag") facts = t("{n} projects", { n: n.weight });
  else facts = `${n.entity_kind ? esc(n.entity_kind) + " · " : ""}${t("used by {n} projects", { n: n.weight })}`;
  const tags = n.kind === "project" ? m.tagsOf(n.id) : [];
  const groups = graphNeighbourGroups(m, n).map(([h, list]) => `<div class="graph-side-group"><div class="small muted">${esc(h)} (${list.length})</div><ul>${list.slice(0, 40).map(({ node, edge }) => `<li><button type="button" class="graph-node-link" data-gnode="${esc(node.id)}"><span class="graph-dot" style="background:${ctl.colorOf(node.id)}" aria-hidden="true"></span><span>${esc(node.label)}</span></button> <span class="small muted">${esc(graphEdgeReason(edge))}</span></li>`).join("")}${list.length > 40 ? `<li class="small muted">${t("+{n} more", { n: list.length - 40 })}</li>` : ""}</ul></div>`).join("");
  const openLabel = n.kind === "project" ? t("Open project") : n.kind === "tag" ? t("Show projects") : t("Open entity");
  const canFocus = n.kind === "project" && !n.focus && ctx.onFocus;
  aside.innerHTML = `<div class="graph-side-head"><span class="badge">${esc(kindLabel)}</span><b class="graph-side-title">${esc(n.label)}</b>
      <button type="button" class="ghost" data-gclose title="${t("Close")}" aria-label="${t("Close")}">${icon("x")}</button></div>
    <p class="small muted">${facts}</p>
    ${tags.length ? `<p class="graph-side-tags">${tags.map((tg) => tagChip(tg.replace(/^#/, ""))).join(" ")}</p>` : ""}
    <div class="graph-side-actions">
      <button type="button" class="primary" data-gopen>${icon("external")} ${openLabel}</button>
      ${canFocus ? `<button type="button" data-gfocus title="${t("Show the local graph of this project")}">${icon("graph")} ${t("Focus here")}</button>` : ""}
      <button type="button" class="ghost" data-gcopy title="${t("Copy name")}" aria-label="${t("Copy name")}">${icon("download")}</button>
    </div>
    ${groups || `<p class="small muted">${t("no links")}</p>`}`;
  aside.hidden = false;
  $("[data-gclose]", aside).onclick = () => ctl.select(null);
  $("[data-gopen]", aside).onclick = () => ctl.open(n);
  $("[data-gcopy]", aside).onclick = () => copyText(n.label);
  const f = $("[data-gfocus]", aside);
  if (f) f.onclick = () => ctx.onFocus(n.label);
  bindTagChips(aside);
  aside.querySelectorAll("[data-gnode]").forEach((b) => {
    b.onclick = () => { if (!ctl.select(b.dataset.gnode)) toast(t("Hidden by the current filters")); };
  });
}

function graphLegend(el, ctl, prefs) {
  const m = ctl.model, c = ctl.style();
  const dot = (color, extra = "") => `<span class="graph-dot${extra}" style="background:${color}" aria-hidden="true"></span>`;
  if (prefs.color === "tag") {
    if (!m.groups.length) { el.innerHTML = `<span class="small muted">${t("No group tags yet. Add tags on a project page to color groups.")}</span>`; return; }
    const shown = m.groups.slice(0, 16);
    el.innerHTML = `${shown.map((gr) => `<button type="button" class="graph-legend-item" data-group="${esc(gr)}" aria-pressed="false">${dot(ctl.groupColor(gr))}${esc(gr)}</button>`).join("")}
      <span class="graph-legend-item static">${dot(c.ungrouped)}${t("No tag")}</span>${m.groups.length > shown.length ? `<span class="small muted">${t("+{n} more", { n: m.groups.length - shown.length })}</span>` : ""}`;
    el.querySelectorAll("[data-group]").forEach((b) => {
      b.onclick = () => {
        const on = ctl.setGroup(b.dataset.group);
        el.querySelectorAll("[data-group]").forEach((x) => { const a = x.dataset.group === on; x.classList.toggle("active", a); x.setAttribute("aria-pressed", String(a)); });
      };
    });
  } else if (prefs.color === "activity") {
    el.innerHTML = [[1, t("under 7 days")], [0.8, t("under 30 days")], [0.55, t("under 90 days")], [0.35, t("older or unknown")]]
      .map(([a, l]) => `<span class="graph-legend-item static"><span class="graph-dot" style="background:${c.project};opacity:${a}" aria-hidden="true"></span>${l}</span>`).join("");
  } else {
    el.innerHTML = [[c.project, t("Project")], [c.tag, t("Tag")], [c.entity, t("Entity")]].map(([col, l]) => `<span class="graph-legend-item static">${dot(col)}${l}</span>`).join("")
      + `<span class="graph-legend-item static"><span class="graph-dot hollow" style="border-color:${c.missing}" aria-hidden="true"></span>${t("Linked, no memory yet")}</span>`;
  }
}

// ---------------------------------------------------------------- panel graf
// `project` kosong = graf global. `page` = halaman #graf (URL & "Fokus di sini" tanpa pindah halaman).
async function graphPanel(box, { project = "", depth = 1, page = false } = {}) {
  const q = new URLSearchParams();
  if (project) { q.set("project", project); q.set("depth", String(depth)); }
  box.innerHTML = `<div class="empty">${t("Loading graph...")}</div>`;
  let g;
  try { g = await api(`/api/subjects/${S()}/projects/graph?${q}`); }
  catch (e) { box.innerHTML = `<div class="card">${t("Could not load graph:")} ${esc(e.message)}</div>`; return; }
  const crumb = page && project ? `<p class="small graph-crumb"><a href="#graf">${t("All projects")}</a> / <b>${esc(project)}</b></p>` : "";
  const projects = g.nodes.filter((n) => n.kind === "project");
  if (!projects.length || (!project && !g.edges.length)) {
    box.innerHTML = `${crumb}<div class="card empty-state"><p><b>${project ? t("This project has no neighbors in the graph yet.") : t("No links between projects yet.")}</b></p>
      <p class="small">${t("The graph is built from three sources:")}</p><ol class="small">
      <li>${t("Project notes: open a project page, <b>Edit project info</b>, then type <code>[[</code> to link another project.")}</li>
      <li>${t("Agent memory: <code>remember</code> with text containing <code>[[Project-Name]]</code>.")}</li>
      <li>${t("Automatic: episodes that mention another project, shared group tags, and tools/organizations used by several projects.")}</li></ol></div>`;
    return;
  }
  const prefs = graphPrefs();
  const pairs = g.edges.filter((e) => e.kind === "link" || e.kind === "mention").length;
  const counts = { tag: g.nodes.filter((n) => n.kind === "tag").length, entity: g.nodes.filter((n) => n.kind === "entity").length };
  const opt = (v, l, cur) => `<option value="${v}" ${String(v) === String(cur) ? "selected" : ""}>${l}</option>`;
  box.innerHTML = `<div class="graph-wrap">
    ${crumb}
    <div class="graph-tools" role="toolbar" aria-label="${t("Graph controls")}">
      <input type="search" class="graph-search" placeholder="${t("Search nodes...")}" aria-label="${t("Search nodes in the graph")}" autocomplete="off">
      <span class="mode graph-mode" role="group" aria-label="${t("View")}">
        <button type="button" data-gmode="3d" class="${prefs.mode !== "2d" ? "active" : ""}" aria-pressed="${prefs.mode !== "2d"}" title="${t("Globe: drag to rotate")}">3D</button>
        <button type="button" data-gmode="2d" class="${prefs.mode === "2d" ? "active" : ""}" aria-pressed="${prefs.mode === "2d"}" title="${t("Flat: drag to pan")}">2D</button>
      </span>
      ${project ? "" : `<button type="button" class="ghost graph-rotate${prefs.rotate ? " active" : ""}" data-grotate aria-pressed="${!!prefs.rotate}" title="${t("Auto-rotate (R)")}" aria-label="${t("Auto-rotate")}">${icon("orbit")}</button>`}
      <label class="small graph-select">${t("Color")} <select data-gcolor>${opt("kind", t("Node kind"), prefs.color)}${opt("tag", t("Group tag"), prefs.color)}${opt("activity", t("Activity"), prefs.color)}</select></label>
      <label class="small graph-select">${t("Active within")} <select data-gdays>${opt(0, t("All time"), prefs.days)}${opt(7, t("7 days"), prefs.days)}${opt(30, t("30 days"), prefs.days)}${opt(90, t("90 days"), prefs.days)}</select></label>
      ${project ? `<label class="small graph-select">${t("Depth")} <select data-depth>${[1, 2, 3].map((d) => opt(d, d, depth)).join("")}</select></label>` : ""}
      <div class="graph-zoom">
        <button type="button" class="ghost" data-zoom="in" title="${t("Zoom in")}" aria-label="${t("Zoom in")}">${icon("plus")}</button>
        <button type="button" class="ghost" data-zoom="out" title="${t("Zoom out")}" aria-label="${t("Zoom out")}">${icon("minus")}</button>
        <button type="button" class="ghost" data-zoom="fit" title="${t("Fit all nodes")}" aria-label="${t("Fit all nodes")}">${icon("fit")}</button>
      </div>
    </div>
    <div class="tag-filter graph-filter">
      ${GRAPH_KINDS.map(([k, l]) => `<button type="button" data-gk="${k}" class="${prefs[k] ? "active" : ""}" aria-pressed="${!!prefs[k]}"><span class="graph-key gk-${k}" aria-hidden="true"></span>${l}</button>`).join("")}
      <button type="button" data-gk="orphans" class="${prefs.orphans ? "active" : ""}" aria-pressed="${!!prefs.orphans}">${t("Unlinked projects")}</button>
    </div>
    <div class="graph-main">
      <div class="graph-stage"><canvas class="graph-canvas" tabindex="0" role="img"></canvas><div class="graph-tip" hidden></div></div>
      <aside class="graph-side card" hidden aria-label="${t("Node details")}"></aside>
    </div>
    <div class="graph-legend"></div>
    <div class="small muted graph-stats">${t("{projects} projects · {pairs} links between projects · {tags} tags · {entities} entities", { projects: projects.length, pairs, tags: counts.tag, entities: counts.entity })}${g.truncated ? ` · ${t("trimmed from {n} nodes", { n: g.total_nodes })}` : ""}. <span class="graph-hint"></span></div>
    <div class="sr graph-live" aria-live="polite"></div>
    <details class="graph-list"><summary>${t("Link list (text)")}</summary><div class="graph-list-body"></div></details>
  </div>`;

  const aside = $(".graph-side", box), live = $(".graph-live", box), hint = $(".graph-hint", box), legend = $(".graph-legend", box);
  const search = $(".graph-search", box);
  const onFocus = (name) => {
    if (page) {
      history.pushState(null, "", `#graf/${encodeURIComponent(name)}`);
      graphPanel(box, { project: name, depth: 1, page: true });
    } else location.hash = `#graf/${encodeURIComponent(name)}`;
  };
  const setHint = () => {
    hint.textContent = prefs.mode === "2d"
      ? t("Drag to pan, scroll or pinch to zoom. Click a node for details, double-click to open.")
      : t("Drag to rotate, Shift+drag to pan, scroll or pinch to zoom. Click a node for details, double-click to open. Arrow keys step through nodes.");
  };
  let view = null;
  const setToggle = (sel, on) => { const b = $(sel, box); if (b) { b.classList.toggle("active", on); b.setAttribute("aria-pressed", String(on)); } };
  view = mountGraph($(".graph-canvas", box), g, prefs, $(".graph-tip", box), {
    onSelect: (n) => {
      graphNodePanel(aside, n, view, { onFocus });
      live.textContent = n ? t("Selected: {name}", { name: n.label }) : "";
    },
    onKey: (k) => {
      if (k === "/") search.focus();
      else if (k === "r" && !project) { view.setRotate(!prefs.rotate); saveGraphPrefs(prefs); setToggle("[data-grotate]", prefs.rotate); }
      else if (k === "m") $(`[data-gmode="${prefs.mode === "2d" ? "3d" : "2d"}"]`, box).click();
    },
  });
  setHint();
  graphLegend(legend, view, prefs);

  // Daftar ketetanggaan sebagai alternatif teks untuk kanvas.
  const m = view.model;
  const rows = m.nodes.filter((n) => n.kind === "project").sort((a, b) => a.label.localeCompare(b.label)).map((p) => {
    const parts = graphNeighbourGroups(m, p).map(([h, l]) => `<span class="muted">${esc(h)}:</span> ${l.map((x) => graphNodeLink(x.node)).join(" ")}`);
    return `<li><b>${wikiLink(p.label)}</b>${parts.length ? " " + parts.join(" · ") : ` <span class="muted">${t("no links")}</span>`}</li>`;
  });
  const list = $(".graph-list-body", box);
  list.innerHTML = `<ul>${rows.join("")}</ul>`;
  bindCites(list);
  bindTagChips(list);

  box.querySelectorAll("[data-gk]").forEach((b) => {
    b.onclick = () => {
      const k = b.dataset.gk;
      view.setFilters({ [k]: !prefs[k] });
      setToggle(`[data-gk="${k}"]`, prefs[k]);
      saveGraphPrefs(prefs);
    };
  });
  box.querySelectorAll("[data-gmode]").forEach((b) => {
    b.onclick = () => {
      view.setMode(b.dataset.gmode);
      box.querySelectorAll("[data-gmode]").forEach((x) => setToggle(`[data-gmode="${x.dataset.gmode}"]`, x.dataset.gmode === prefs.mode));
      const rb = $("[data-grotate]", box);
      if (rb) rb.hidden = prefs.mode === "2d";
      saveGraphPrefs(prefs); setHint();
    };
  });
  const rb = $("[data-grotate]", box);
  if (rb) {
    rb.hidden = prefs.mode === "2d";
    rb.onclick = () => { view.setRotate(!prefs.rotate); setToggle("[data-grotate]", prefs.rotate); saveGraphPrefs(prefs); };
  }
  $("[data-gcolor]", box).onchange = (e) => { view.setColor(e.target.value); saveGraphPrefs(prefs); graphLegend(legend, view, prefs); };
  $("[data-gdays]", box).onchange = (e) => { view.setDays(+e.target.value || 0); saveGraphPrefs(prefs); };
  box.querySelectorAll("[data-zoom]").forEach((b) => { b.onclick = () => view.zoom(b.dataset.zoom); });
  search.oninput = () => view.search(search.value);
  search.onkeydown = (e) => { if (e.key === "Enter") { e.preventDefault(); view.focusMatch(); } };
  const ds = $("[data-depth]", box);
  if (ds) {
    ds.onchange = () => {
      sessionStorage.setItem("proj-graph-depth", ds.value);
      if (page) history.replaceState(null, "", `#graf/${encodeURIComponent(project)}?depth=${ds.value}`);
      graphPanel(box, { project, depth: +ds.value, page });
    };
  }
}
