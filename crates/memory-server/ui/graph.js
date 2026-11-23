// Graf proyek bergaya Obsidian: simpul proyek/tag/entitas, sisi tautan [[...]], sebutan, tag, entitas.
// Tampil sebagai bola 3D yang bisa diputar (bawaan) atau bidang 2D. Kanvas 2D + proyeksi perspektif
// dan layout gaya pegas buatan sendiri, tanpa pustaka: UI di-embed ke biner dan harus jalan offline.
// Berkas ini hanya berisi deklarasi (tanpa efek samping saat dimuat) dan memakai helper dari app.js
// (`$`, `api`, `esc`, `icon`, `S`, `wikiLink`, `tagChip`, `bindCites`, `bindTagChips`, `showEntity`,
// `projHref`, `ago`, `copyText`, `toast`) serta `t()` dari i18n.js saat fungsi dipanggil.
"use strict";

const GRAPH_KINDS = [["link", t("Links [[ ]]")], ["mention", t("Mentions")], ["tag", t("Tags")], ["entity", t("Entities")]];
const GRAPH_DEFAULTS = { v: 2, link: true, mention: true, tag: true, entity: true, orphans: true, mode: "3d", rotate: true, color: "cluster", days: 0 };
const GRAPH_DAY_MS = 864e5;
const GRAPH_CLUSTER_HUES = [202, 352, 265, 28, 145, 322, 178, 48, 228, 95, 12, 290, 162, 72];

function graphPrefs() {
  try {
    const raw = JSON.parse(localStorage.getItem("graph-filters") || "{}");
    if (!raw.v && raw.color === "kind") delete raw.color;
    return { ...GRAPH_DEFAULTS, ...raw, v: GRAPH_DEFAULTS.v };
  } catch (e) { return { ...GRAPH_DEFAULTS }; }
}
function saveGraphPrefs(p) { try { localStorage.setItem("graph-filters", JSON.stringify({ ...p, v: GRAPH_DEFAULTS.v })); } catch (e) { /* storage diblokir */ } }

const graphRadius = (n) => (n.kind === "project" ? 5 + 2.2 * Math.sqrt(n.degree + n.weight / 8) : n.kind === "tag" ? 4 + 1.4 * Math.sqrt(n.degree) : 3 + Math.sqrt(n.degree));
const graphClamp = (v, a, b) => Math.max(a, Math.min(b, v));

// Prefiks bermakna nama proyek (mis. "api-master-kandang" -> "api-master", "BACKEND/foo" -> "backend").
function graphPrefix(label) {
  const s = String(label || "").trim().toLowerCase();
  const slash = s.indexOf("/");
  if (slash > 1) return s.slice(0, slash);
  const parts = s.split(/[-_\s]+/).filter(Boolean);
  if (parts.length >= 3) return `${parts[0]}-${parts[1]}`;
  if (parts.length === 2 && parts[0].length >= 3) return parts[0];
  return "";
}

// Pengelompokan klaster otomatis berbasis hub proyek + kedekatan graf, agar proyek besar
// menyebar ke klaster berbeda dan satelit (tag/entitas) mengikuti klaster proyek induknya.
function graphClusters(nodes, edges, groupOf) {
  const projects = nodes.filter((n) => n.kind === "project");
  const clusterOf = new Map();
  if (!projects.length) return { clusters: [], clusterOf };

  // Afinitas antar proyek: tautan langsung kuat, tag/entitas bersama moderat (abaikan hub terlalu umum).
  const aff = new Map(projects.map((p) => [p.id, new Map()]));
  const addAff = (a, b, w) => {
    if (a === b) return;
    const ma = aff.get(a), mb = aff.get(b);
    if (!ma || !mb) return;
    ma.set(b, (ma.get(b) || 0) + w);
    mb.set(a, (mb.get(a) || 0) + w);
  };
  const satToProj = new Map();
  for (const e of edges) {
    if (e.s.kind === "project" && e.t.kind === "project") {
      addAff(e.s.id, e.t.id, 3 + Math.log(1 + Math.max(1, e.weight)));
    } else if (e.s.kind === "project" && e.t.kind !== "project") {
      if (!satToProj.has(e.t.id)) satToProj.set(e.t.id, []);
      satToProj.get(e.t.id).push({ id: e.s.id, w: Math.max(1, e.weight) });
    } else if (e.t.kind === "project" && e.s.kind !== "project") {
      if (!satToProj.has(e.s.id)) satToProj.set(e.s.id, []);
      satToProj.get(e.s.id).push({ id: e.t.id, w: Math.max(1, e.weight) });
    }
  }
  for (const [, list] of satToProj) {
    if (list.length < 2 || list.length > 14) continue;
    const share = 1.2 / list.length;
    for (let i = 0; i < list.length; i++) for (let j = i + 1; j < list.length; j++) addAff(list[i].id, list[j].id, share);
  }

  // Pilih K jangkar (proyek hub paling menonjol yang saling berjauhan di graf).
  const K = projects.length <= 6 ? projects.length : graphClamp(Math.round(Math.sqrt(projects.length) * 1.6), 4, 14);
  const ranked = projects.slice().sort((a, b) => b.degree - a.degree || b.weight - a.weight || (a.id < b.id ? -1 : 1));
  const anchors = [ranked[0]];
  const anchorIds = new Set([ranked[0].id]);
  while (anchors.length < K) {
    let best = null, bestScore = -1;
    for (const p of ranked) {
      if (anchorIds.has(p.id)) continue;
      const base = 1 + p.degree + Math.log(1 + Math.max(0, p.weight));
      const ma = aff.get(p.id);
      let pen = 0;
      const pg = groupOf.get(p.id), pp = graphPrefix(p.label);
      for (const a of anchors) {
        if (ma && ma.has(a.id)) pen += 1.6 + Math.min(3, ma.get(a.id) * 0.4);
        if (pg && pg === groupOf.get(a.id)) pen += 0.35;
        if (pp && pp === graphPrefix(a.label)) pen += 0.45;
      }
      const sc = base / (1 + pen);
      if (sc > bestScore) { bestScore = sc; best = p; }
    }
    if (!best) break;
    anchors.push(best);
    anchorIds.add(best.id);
  }

  const members = anchors.map((a) => [a]);
  const anchorIdx = new Map(anchors.map((a, i) => [a.id, i]));
  anchors.forEach((a, i) => clusterOf.set(a.id, i));

  // Masukkan proyek lain ke jangkar terdekat (afinitas 1-hop + 2-hop + prefiks + keseimbangan ukuran).
  for (const p of ranked) {
    if (anchorIdx.has(p.id)) continue;
    const ma = aff.get(p.id) || new Map();
    const pg = groupOf.get(p.id), pp = graphPrefix(p.label);
    let bestI = 0, bestS = -1;
    for (let i = 0; i < anchors.length; i++) {
      const a = anchors[i];
      let s = ma.get(a.id) || 0;
      // 2-hop melalui tetangga yang sudah masuk klaster i.
      for (const [nid, w] of ma) if (clusterOf.get(nid) === i) s += w * 0.35;
      if (pg && pg === groupOf.get(a.id)) s += 0.5;
      if (pp && pp === graphPrefix(a.label)) s += 0.85;
      const balanced = (s + 0.01) / (1 + 0.16 * members[i].length);
      if (balanced > bestS) { bestS = balanced; bestI = i; }
    }
    clusterOf.set(p.id, bestI);
    members[bestI].push(p);
  }

  // Satelit (tag & entitas) ikut klaster proyek yang paling kuat terhubung dengannya.
  for (const n of nodes) {
    if (n.kind === "project") continue;
    const list = satToProj.get(n.id) || [];
    const votes = new Map();
    for (const { id, w } of list) {
      const ci = clusterOf.get(id);
      if (ci != null) votes.set(ci, (votes.get(ci) || 0) + w);
    }
    let bestI = 0, bestW = -1;
    for (const [ci, w] of votes) if (w > bestW) { bestW = w; bestI = ci; }
    if (bestW >= 0) clusterOf.set(n.id, bestI);
  }

  // Urutkan klaster dari terbesar agar warna & legenda stabil dan rapi.
  const counts = anchors.map((_, i) => 0);
  for (const ci of clusterOf.values()) counts[ci]++;
  const order = anchors.map((a, i) => ({ old: i, anchor: a, size: counts[i] }))
    .sort((x, y) => y.size - x.size || x.anchor.label.localeCompare(y.anchor.label));
  const remap = new Map(order.map((o, i) => [o.old, i]));
  for (const [id, ci] of clusterOf) clusterOf.set(id, remap.get(ci));
  const clusters = order.map((o, i) => ({ id: i, label: o.anchor.label, anchor: o.anchor, size: o.size }));
  return { clusters, clusterOf };
}

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
  const { clusters, clusterOf } = graphClusters(nodes, edges, groupOf);
  const sortedIds = nodes.slice().sort((a, b) => (a.kind === b.kind ? (a.id < b.id ? -1 : 1) : a.kind === "project" ? -1 : b.kind === "project" ? 1 : a.kind < b.kind ? -1 : 1));
  const nodeIndex = new Map(sortedIds.map((n, i) => [n.id, i]));

  const m = { nodes, edges, byId, groups, groupOf, clusters, clusterOf, nodeIndex, tagsOf: (id) => tagsOf.get(id) || [], focus: nodes.find((n) => n.focus) || null, visN: [], visE: [], neigh: new Map(), rank: new Map() };
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
    text: v("--graph-text", "#1d1d1f"), halo: v("--graph-halo", "rgba(255,255,255,.85)"),
    labelBg: v("--graph-label-bg", "rgba(248,250,252,.86)"), grid: v("--graph-grid", "rgba(100,116,139,.09)"),
    accent: v("--accent", "#0071e3"),
    globe: v("--graph-globe", "rgba(0,113,227,.06)"), globeLine: v("--graph-globe-line", "rgba(100,116,139,.22)"),
    groupS: v("--graph-group-s", "58%"), groupL: v("--graph-group-l", "44%"),
  };
}
const graphHue = (i) => Math.round((210 + i * 137.508) % 360);
const graphClusterHue = (ci) => (ci < GRAPH_CLUSTER_HUES.length ? GRAPH_CLUSTER_HUES[ci] : graphHue(ci));
function graphClusterColor(c, ci, variant = null) {
  if (ci == null || ci < 0) return c.ungrouped;
  const baseH = graphClusterHue(ci);
  const baseS = parseFloat(c.groupS) || 60;
  const baseL = parseFloat(c.groupL) || 56;
  if (variant == null) return `hsl(${baseH}, ${Math.round(baseS)}%, ${Math.round(baseL)}%)`;
  const dh = ((variant % 5) - 2) * 5;
  const dl = ((variant % 3) - 1) * 6;
  const h = (baseH + dh + 360) % 360;
  const l = graphClamp(Math.round(baseL + dl), 28, 78);
  return `hsl(${h}, ${Math.round(baseS)}%, ${l}%)`;
}
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
  if (mode === "cluster") return graphClusterColor(c, m.clusterOf.get(n.id), m.nodeIndex.get(n.id) || 0);
  if (mode === "node") return `hsl(${graphHue(m.nodeIndex.get(n.id) || 0)}, ${c.groupS}, ${c.groupL})`;
  if (mode === "tag" && n.kind !== "entity") return graphGroupColor(m, c, m.groupOf.get(n.id));
  return c[n.kind] || c.entity;
}

// ---------------------------------------------------------------- fisika
// Tolak-menolak sadar-klaster & ukuran + pegas ternormalisasi derajat agar hub besar menyebar
// merata di seluruh bola (3D) atau membentuk pulau klaster rapi (2D).
function graphForces(m, alpha, three, R = 220) {
  const N = m.visN, n = N.length;
  const repBase = three ? (R * R * 1.45) / Math.max(8, n) : 2200;
  const maxD2 = three ? Infinity : 640000;
  for (let i = 0; i < n; i++) {
    const a = N[i], ra = graphRadius(a), ca = m.clusterOf.get(a.id);
    for (let j = i + 1; j < n; j++) {
      const b = N[j], rb = graphRadius(b), cb = m.clusterOf.get(b.id);
      let dx = a.x - b.x, dy = a.y - b.y, dz = three ? a.z - b.z : 0, d2 = dx * dx + dy * dy + dz * dz;
      if (d2 < 0.01) { dx = (i - j) * 0.1; dy = 0.1; dz = three ? 0.05 : 0; d2 = dx * dx + dy * dy + dz * dz; }
      if (d2 > maxD2) continue;
      const d = Math.sqrt(d2);
      const sameC = ca != null && ca === cb;
      const bothProj = a.kind === "project" && b.kind === "project";
      const mult = sameC ? (bothProj ? 0.85 : 0.55) : (bothProj ? 3.4 : 2.2);
      let f = (repBase * mult * alpha) / d2 / d;
      // Hindari tumpang-tindih simpul besar.
      const minD = (ra + rb) * (bothProj ? 2.5 : 1.9);
      if (d < minD) f += ((minD - d) / d) * 0.42 * alpha;
      a.vx += dx * f; a.vy += dy * f; a.vz += dz * f; b.vx -= dx * f; b.vy -= dy * f; b.vz -= dz * f;
    }
  }
  for (const e of m.visE) {
    const a = e.s, b = e.t;
    const sameC = m.clusterOf.get(a.id) != null && m.clusterOf.get(a.id) === m.clusterOf.get(b.id);
    const ra = graphRadius(a), rb = graphRadius(b);
    const L = sameC
      ? Math.max((ra + rb) * 2.3, three ? R * 0.15 : 52)
      : (three ? R * 0.88 : 240);
    const dx = b.x - a.x, dy = b.y - a.y, dz = three ? b.z - a.z : 0, d = Math.sqrt(dx * dx + dy * dy + dz * dz) || 0.1;
    const degA = m.neigh.get(a.id)?.size || 1, degB = m.neigh.get(b.id)?.size || 1;
    const degScale = 1 / Math.sqrt(Math.max(1, Math.min(degA, degB)));
    const baseK = sameC ? 0.048 : 0.008;
    const k = ((d - L) / d) * baseK * alpha * degScale * Math.min(2.5, 1 + Math.log(Math.max(1, e.weight)) * 0.5);
    // Satelit (tag/entitas) lebih ringan agar tidak menyedot semua proyek ke satu titik.
    const wa = a.kind === "project" && b.kind !== "project" ? 0.25 : b.kind === "project" && a.kind !== "project" ? 0.75 : 0.5;
    const wb = 1 - wa;
    a.vx += dx * k * wa * 2; a.vy += dy * k * wa * 2; a.vz += dz * k * wa * 2;
    b.vx -= dx * k * wb * 2; b.vy -= dy * k * wb * 2; b.vz -= dz * k * wb * 2;
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
// Bidang datar: layar = dunia * scale + (tx, ty). Klaster tersebar sebagai pulau terpisah.
function layout2d(m, scr) {
  const pos = graphPositions(m);
  const v = { mode: "2d", scale: 1, tx: 0, ty: 0 };
  v.activate = () => {
    if (pos.restore()) return false;
    const K = Math.max(1, m.clusters.length);
    const centers = [];
    for (let ci = 0; ci < K; ci++) {
      const a = ci * 2.399963, r = K <= 1 ? 0 : 175 * Math.sqrt(ci + 0.6);
      centers.push([Math.cos(a) * r, Math.sin(a) * r]);
    }
    const localIdx = new Map();
    const list = m.nodes.slice().sort((a, b) => (a.kind === "project" ? 0 : 1) - (b.kind === "project" ? 0 : 1) || (a.id < b.id ? -1 : 1));
    for (const n of list) {
      if (n.focus) { n.x = 0; n.y = 0; n.z = 0; n.vx = n.vy = n.vz = 0; continue; }
      const ci = m.clusterOf.get(n.id) ?? 0;
      const [cx, cy] = centers[ci] || [0, 0];
      const idx = localIdx.get(ci) || 0;
      localIdx.set(ci, idx + 1);
      const a = idx * 2.399963, r = idx === 0 ? 0 : 20 * Math.sqrt(idx);
      n.x = cx + Math.cos(a) * r; n.y = cy + Math.sin(a) * r; n.z = 0; n.vx = n.vy = n.vz = 0;
    }
    return true;
  };
  v.deactivate = () => pos.save();
  v.refilter = () => {};
  v.tick = (alpha) => {
    graphForces(m, alpha, false, 220);
    for (const a of m.visN) {
      a.vx -= a.x * 0.008 * alpha; a.vy -= a.y * 0.008 * alpha;
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
    v.scale = graphClamp(Math.min((scr.W - pad * 2) / Math.max(1, x1 - x0), (scr.H - pad * 2) / Math.max(1, y1 - y0)), 0.15, 2);
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
    const ns = graphClamp(v.scale * f, 0.15, 4);
    v.tx = sx - ((sx - v.tx) * ns) / v.scale; v.ty = sy - ((sy - v.ty) * ns) / v.scale; v.scale = ns;
  };
  v.center = (n) => { v.scale = Math.max(v.scale, 1.2); v.tx = scr.W / 2 - n.x * v.scale; v.ty = scr.H / 2 - n.y * v.scale; };
  v.step = () => false;
  v.globe = () => null;
  return v;
}

// ---------------------------------------------------------------- layout 3D (bola)
// Simpul dikunci di permukaan bola radius R: pusat klaster disebar merata dengan Fibonacci sphere,
// lalu gaya tolak-menolak + koreksi pusat massa mencegah klaster mengumpul ke satu sisi bola.
function layout3d(m, scr) {
  const pos = graphPositions(m);
  const v = { mode: "3d", scale: 1, tx: 0, ty: 0, yaw: m.focus ? 0 : 0.6, pitch: m.focus ? 0 : -0.3, R: 180 };
  let anim = null;
  const radiusFor = (k) => graphClamp(150 + 11 * Math.sqrt(k), 170, 440);
  const toSphere = (n, R) => { const l = Math.hypot(n.x, n.y, n.z) || 1; n.x *= R / l; n.y *= R / l; n.z *= R / l; };
  v.activate = () => {
    const R = radiusFor(m.visN.length);
    if (pos.restore()) { for (const n of m.nodes) toSphere(n, R); v.R = R; return false; }
    v.R = R;
    const K = Math.max(1, m.clusters.length);
    // Pusat klaster disebar merata di seluruh bola (Fibonacci sphere).
    const centers = [];
    for (let ci = 0; ci < K; ci++) {
      const y = K === 1 ? 0 : 1 - (2 * (ci + 0.5)) / K;
      const r = Math.sqrt(Math.max(0, 1 - y * y)), phi = ci * 2.399963;
      centers.push([Math.cos(phi) * r, y, Math.sin(phi) * r]);
    }
    const localIdx = new Map();
    const list = m.nodes.filter((n) => !n.focus).sort((a, b) => (a.kind === "project" ? 0 : 1) - (b.kind === "project" ? 0 : 1) || b.degree - a.degree || (a.id < b.id ? -1 : 1));
    for (const n of list) {
      const ci = m.clusterOf.get(n.id) ?? 0;
      const [ux, uy, uz] = centers[ci] || [0, 1, 0];
      // Basis ortonormal di bidang singgung pusat klaster.
      const ax = Math.abs(uy) < 0.9 ? 0 : 1, ay = Math.abs(uy) < 0.9 ? 1 : 0, az = 0;
      let tx = uy * az - uz * ay, ty = uz * ax - ux * az, tz = ux * ay - uy * ax;
      const tl = Math.hypot(tx, ty, tz) || 1; tx /= tl; ty /= tl; tz /= tl;
      const bx = uy * tz - uz * ty, by = uz * tx - ux * tz, bz = ux * ty - uy * tx;
      const idx = localIdx.get(ci) || 0;
      localIdx.set(ci, idx + 1);
      const ang = idx * 2.399963, rad = idx === 0 ? 0 : Math.min(0.32, 0.065 * Math.sqrt(idx));
      const ox = Math.cos(ang) * rad, oy = Math.sin(ang) * rad;
      n.x = (ux + tx * ox + bx * oy) * R;
      n.y = (uy + ty * ox + by * oy) * R;
      n.z = (uz + tz * ox + bz * oy) * R;
      toSphere(n, R);
      n.vx = n.vy = n.vz = 0;
    }
    if (m.focus) { m.focus.x = 0; m.focus.y = 0; m.focus.z = -R; m.focus.vx = m.focus.vy = m.focus.vz = 0; }
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
    graphForces(m, alpha, true, v.R);
    // Koreksi pusat massa agar simpul tidak mengumpul di satu sisi bola.
    let mx = 0, my = 0, mz = 0, mw = 0;
    if (!m.focus) {
      for (const a of m.visN) { const w = a.kind === "project" ? 2 : 1; mx += a.x * w; my += a.y * w; mz += a.z * w; mw += w; }
      if (mw > 0) { mx /= mw; my /= mw; mz /= mw; }
    }
    const recenter = !m.focus && Math.hypot(mx, my, mz) > 0.04 * v.R ? 0.14 * alpha : 0;
    for (const a of m.visN) {
      if (a.fixed || a.focus) { a.vx = a.vy = a.vz = 0; if (a.focus && !a.fixed) { a.x = 0; a.y = 0; a.z = -v.R; } continue; }
      if (recenter) { a.vx -= mx * recenter; a.vy -= my * recenter; a.vz -= mz * recenter; }
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
  // Kisi latar halus untuk membantu pembacaan ruang & klaster.
  const step = 48;
  ctx.globalAlpha = 1; ctx.lineWidth = 1; ctx.strokeStyle = c.grid; ctx.setLineDash([]);
  ctx.beginPath();
  for (let x = (W / 2) % step; x < W; x += step) { ctx.moveTo(x, 0); ctx.lineTo(x, H); }
  for (let y = (H / 2) % step; y < H; y += step) { ctx.moveTo(0, y); ctx.lineTo(W, y); }
  ctx.stroke();

  const hot = ui.hover || ui.selected;
  const lit = hot ? m.neigh.get(hot.id) || new Set() : null;
  const matches = (n) => ui.query && n.label.toLowerCase().includes(ui.query);
  const dim = (n) => (lit && n !== hot && !lit.has(n.id))
    || (ui.query && !matches(n) && n !== hot)
    || (ui.group && m.groupOf.get(n.id) !== ui.group && n !== hot)
    || (ui.cluster !== null && m.clusterOf.get(n.id) !== ui.cluster && n !== hot);
  for (const n of order) n._col = graphNodeColor(m, c, n, ui.color);

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
  // Pusat layar tiap klaster untuk mengikat (edge bundling) sisi antar-klaster menjadi berkas kurva rapi.
  const cSum = new Map();
  for (const n of order) {
    const ci = m.clusterOf.get(n.id);
    if (ci == null) continue;
    let st = cSum.get(ci);
    if (!st) { st = { x: 0, y: 0, w: 0 }; cSum.set(ci, st); }
    const w = n.kind === "project" ? 2 : 1;
    st.x += n.sx * w; st.y += n.sy * w; st.w += w;
  }
  const cPos = new Map();
  for (const [ci, st] of cSum) cPos.set(ci, { x: st.x / st.w, y: st.y / st.w });

  const zw = graphClamp(Math.sqrt(v.scale), 0.6, 1.5);
  const coloredEdges = ui.color === "cluster" || ui.color === "node" || ui.color === "tag";
  const hasFilter = !!(hot || ui.query || ui.group || ui.cluster !== null);
  for (const e of edges) {
    const on = (hot && (e.s === hot || e.t === hot)) || e === ui.hoverEdge;
    const ca = m.clusterOf.get(e.s.id), cb = m.clusterOf.get(e.t.id);
    const sameC = ca != null && ca === cb;
    const base = hasFilter && !on ? 0.08 : on ? 0.92 : sameC ? (e.kind === "link" || e.kind === "mention" ? 0.55 : 0.32) : (e.kind === "link" || e.kind === "mention" ? 0.48 : 0.24);
    ctx.globalAlpha = base * Math.min(e.s.fade, e.t.fade);
    ctx.lineWidth = Math.min(3.5, 0.65 + Math.log(1 + e.weight) * 0.55) * zw * (e === ui.hoverEdge ? 1.8 : on ? 1.35 : 1);
    ctx.setLineDash(e.kind === "mention" ? [5, 4] : e.kind === "tag" ? [1.5, 3] : []);
    const dx0 = e.t.sx - e.s.sx, dy0 = e.t.sy - e.s.sy, d = Math.hypot(dx0, dy0) || 1, rt = e.t.sr + 2;
    if (d <= rt) { e._q = null; continue; }
    // Kurva Bezier terikat untuk sisi antar-klaster yang cukup panjang.
    let qx = null, qy = null;
    if (!sameC && d > 36 && cPos.has(ca) && cPos.has(cb)) {
      const pA = cPos.get(ca), pB = cPos.get(cb);
      const mx = (pA.x + pB.x) * 0.5, my = (pA.y + pB.y) * 0.5;
      const exm = (e.s.sx + e.t.sx) * 0.5, eym = (e.s.sy + e.t.sy) * 0.5;
      qx = exm * 0.35 + mx * 0.65; qy = eym * 0.35 + my * 0.65;
    }
    e._q = qx !== null ? [qx, qy] : null;
    const tx = qx !== null ? e.t.sx - qx : dx0, ty = qy !== null ? e.t.sy - qy : dy0, td = Math.hypot(tx, ty) || 1;
    const ex = e.t.sx - (tx / td) * rt, ey = e.t.sy - (ty / td) * rt;
    let stroke = on ? c.accent : c.edge;
    if (!on && coloredEdges && e.s._col && e.t._col) {
      if (sameC || d < 28) stroke = e.s._col;
      else {
        const lg = ctx.createLinearGradient(e.s.sx, e.s.sy, ex, ey);
        lg.addColorStop(0, e.s._col); lg.addColorStop(1, e.t._col);
        stroke = lg;
      }
    }
    ctx.strokeStyle = stroke;
    ctx.beginPath(); ctx.moveTo(e.s.sx, e.s.sy);
    if (qx !== null) ctx.quadraticCurveTo(qx, qy, ex, ey); else ctx.lineTo(ex, ey);
    ctx.stroke();
    if (e.kind === "link" || e.kind === "mention") {
      // Panah arah tautan, ukuran tetap di layar.
      const ah = 6.5, ang = Math.atan2(ty, tx);
      ctx.setLineDash([]); ctx.fillStyle = on ? c.accent : (coloredEdges && e.t._col ? e.t._col : c.edge);
      ctx.beginPath(); ctx.moveTo(ex, ey);
      ctx.lineTo(ex - ah * Math.cos(ang - 0.4), ey - ah * Math.sin(ang - 0.4));
      ctx.lineTo(ex - ah * Math.cos(ang + 0.4), ey - ah * Math.sin(ang + 0.4));
      ctx.closePath(); ctx.fill();
    }
  }
  ctx.setLineDash([]);
  for (const n of order) {
    const r = n.sr, col = n._col;
    ctx.globalAlpha = (dim(n) ? 0.16 : 1) * n.fade * (ui.color === "activity" ? graphAgeAlpha(n) : 1);
    ctx.beginPath(); ctx.arc(n.sx, n.sy, r, 0, Math.PI * 2);
    if (n.missing) { ctx.lineWidth = 1.5; ctx.strokeStyle = col; ctx.stroke(); }
    else {
      ctx.fillStyle = col; ctx.fill();
      if (v.mode === "3d" && r > 2.5) {
        // Kilau kecil di kiri atas agar simpul tampak seperti bola.
        const sh = ctx.createRadialGradient(n.sx - r * 0.35, n.sy - r * 0.35, r * 0.1, n.sx, n.sy, r);
        sh.addColorStop(0, "rgba(255,255,255,0.52)"); sh.addColorStop(1, "rgba(255,255,255,0)");
        ctx.fillStyle = sh; ctx.fill();
      }
    }
    if (n.focus || n === ui.hover || n === ui.selected || matches(n)) {
      ctx.globalAlpha = Math.max(0.6, n.fade);
      ctx.lineWidth = n === ui.selected ? 2.5 : 2; ctx.strokeStyle = c.accent;
      ctx.beginPath(); ctx.arc(n.sx, n.sy, r + 3, 0, Math.PI * 2); ctx.stroke();
    }
  }
  // Label: yang disorot selalu tampil; selain itu proyek penting di sisi depan, dengan lencana latar kontras.
  ctx.font = "11.5px -apple-system, BlinkMacSystemFont, system-ui, sans-serif";
  ctx.textAlign = "center"; ctx.textBaseline = "top"; ctx.lineJoin = "round";
  const K = Math.round(14 + 28 * v.scale), taken = new Set();
  const forced = (n) => n === hot || (lit && lit.has(n.id)) || matches(n) || n.focus;
  const front = (n) => v.mode === "2d" || n.depth < 0;
  const wanted = (n) => n.kind === "project" ? (v.mode === "2d" ? v.scale >= 0.4 : front(n) && (m.rank.get(n.id) ?? 1e9) < K) : v.scale >= 1.4 && front(n);
  const labels = order.filter((n) => forced(n) || wanted(n)).sort((a, b) => (forced(b) ? 1 : 0) - (forced(a) ? 1 : 0) || a.depth - b.depth || (m.rank.get(a.id) ?? 1e9) - (m.rank.get(b.id) ?? 1e9));
  for (const n of labels) {
    const sx = n.sx, sy = n.sy + n.sr + 4;
    if (sx < -100 || sx > W + 100 || sy < -20 || sy > H + 20) continue;
    const key = `${Math.round(sx / 84)}:${Math.round(sy / 20)}`;
    if (taken.has(key) && !forced(n)) continue;
    taken.add(key);
    ctx.globalAlpha = dim(n) ? 0.22 : Math.max(0.65, n.fade);
    const label = n.label.length > 30 ? n.label.slice(0, 29) + "..." : n.label;
    const tw = ctx.measureText ? ctx.measureText(label).width : label.length * 6.5;
    const px = 5, py = 2;
    ctx.fillStyle = c.labelBg;
    ctx.fillRect(sx - tw / 2 - px, sy - py, tw + px * 2, 15 + py);
    ctx.fillStyle = n.missing ? c.missing : c.text;
    ctx.fillText(label, sx, sy);
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
  const segDist = (ax, ay, bx, by) => {
    const l2 = (bx - ax) ** 2 + (by - ay) ** 2;
    if (!l2) return Infinity;
    const k = graphClamp(((px - ax) * (bx - ax) + (py - ay) * (by - ay)) / l2, 0, 1);
    return Math.hypot(px - ax - k * (bx - ax), py - ay - k * (by - ay));
  };
  for (const e of m.visE) {
    const ax = e.s.sx, ay = e.s.sy, bx = e.t.sx, by = e.t.sy;
    let d;
    if (e._q) {
      const mx = 0.25 * ax + 0.5 * e._q[0] + 0.25 * bx, my = 0.25 * ay + 0.5 * e._q[1] + 0.25 * by;
      d = Math.min(segDist(ax, ay, mx, my), segDist(mx, my, bx, by));
    } else d = segDist(ax, ay, bx, by);
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
  const ui = { hover: null, hoverEdge: null, selected: null, query: "", group: "", cluster: null, color: prefs.color };
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
    setColor(c) { prefs.color = c; ui.color = c; ui.group = ""; ui.cluster = null; draw(); },
    setGroup(gr) { ui.cluster = null; ui.group = ui.group === gr ? "" : gr || ""; draw(); return ui.group; },
    setCluster(ci) { ui.group = ""; ui.cluster = ui.cluster === ci ? null : (ci ?? null); draw(); return ui.cluster; },
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
    clusterColor: (ci) => graphClusterColor(style, ci, null),
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
  if (prefs.color === "cluster") {
    const shown = m.clusters.slice(0, 14);
    el.innerHTML = shown.map((cl) => `<button type="button" class="graph-legend-item" data-cluster="${cl.id}" aria-pressed="false">${dot(ctl.clusterColor(cl.id))}${esc(cl.label)}</button>`).join("")
      + `<span class="graph-legend-item static"><span class="graph-dot hollow" style="border-color:${c.missing}" aria-hidden="true"></span>${t("Linked, no memory yet")}</span>`;
    el.querySelectorAll("[data-cluster]").forEach((b) => {
      b.onclick = () => {
        const ci = Number(b.dataset.cluster);
        const on = ctl.setCluster(ci);
        el.querySelectorAll("[data-cluster]").forEach((x) => { const a = Number(x.dataset.cluster) === on; x.classList.toggle("active", a); x.setAttribute("aria-pressed", String(a)); });
      };
    });
  } else if (prefs.color === "node") {
    el.innerHTML = `<span class="graph-legend-item static">${t("Each node has its own color")}</span>`
      + `<span class="graph-legend-item static"><span class="graph-dot hollow" style="border-color:${c.missing}" aria-hidden="true"></span>${t("Linked, no memory yet")}</span>`;
  } else if (prefs.color === "tag") {
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
      <label class="small graph-select">${t("Color")} <select data-gcolor>${opt("cluster", t("Cluster"), prefs.color)}${opt("node", t("Each node"), prefs.color)}${opt("kind", t("Node kind"), prefs.color)}${opt("tag", t("Group tag"), prefs.color)}${opt("activity", t("Activity"), prefs.color)}</select></label>
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
