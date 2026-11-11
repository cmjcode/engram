// Smoke test graf proyek (crates/memory-server/ui/graph.js) tanpa browser: DOM & kanvas tiruan.
// Menguji mode bola 3D dan 2D, uji klik, pilih/putar ke depan, filter, warna kelompok,
// filter aktivitas, gerak dikurangi, dan panel graf lengkap. Dijalankan juga oleh
// `cargo test -p memory-server ui_graph_smoke` bila `node` tersedia.
//   node scripts/ui-graph-smoke.mjs
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const SRC = readFileSync(join(here, "../crates/memory-server/ui/graph.js"), "utf8");
const W = 800, H = 600, DAY = 864e5;
let failures = 0, checks = 0;
const ok = (cond, msg) => { checks++; if (!cond) { failures++; console.error("FAIL:", msg); } };

// ---- DOM tiruan yang permisif: querySelector mengembalikan elemen stub per selektor.
function el(tag = "div") {
  const kids = new Map();
  const e = {
    tagName: tag.toUpperCase(), innerHTML: "", textContent: "", hidden: false, value: "", style: {}, dataset: {}, attrs: {},
    isConnected: true, width: 0, height: 0, listeners: {},
    classList: { _s: new Set(), toggle(c, on) { (on ?? !this._s.has(c)) ? this._s.add(c) : this._s.delete(c); }, add(c) { this._s.add(c); }, remove(c) { this._s.delete(c); }, contains(c) { return this._s.has(c); } },
    setAttribute(k, v) { this.attrs[k] = String(v); }, getAttribute(k) { return this.attrs[k] ?? null; },
    addEventListener(k, f) { (this.listeners[k] ||= []).push(f); }, removeEventListener() {},
    setPointerCapture() {}, releasePointerCapture() {}, focus() { this.focused = true; }, click() { if (this.onclick) this.onclick({ preventDefault() {} }); },
    querySelector(sel) { if (!kids.has(sel)) kids.set(sel, sel.includes("canvas") ? canvasEl() : el()); return kids.get(sel); },
    querySelectorAll() { return []; },
    getBoundingClientRect() { return { left: 0, top: 0, width: W, height: H }; },
  };
  return e;
}
function canvasEl() {
  const c = el("canvas");
  c.ctxCalls = 0; c.texts = [];
  const target = {};
  const ctx = new Proxy(target, {
    get(t, k) {
      if (k in t) return t[k];
      if (k === "createRadialGradient" || k === "createLinearGradient") return () => ({ addColorStop() {} });
      if (k === "fillText") return (s) => { c.texts.push(s); c.ctxCalls++; };
      if (k === "measureText") return (s) => ({ width: String(s).length * 6 });
      return () => { c.ctxCalls++; };
    },
    set(t, k, v) { t[k] = v; return true; },
  });
  c.getContext = () => ctx;
  return c;
}

// ---- jam & rAF yang dikendalikan tes.
let clock = 1_000_000, rafQ = [], rafId = 0;
const performance = { now: () => (clock += 0.25) };
const requestAnimationFrame = (f) => { rafQ.push([++rafId, f]); return rafId; };
const cancelAnimationFrame = (id) => { rafQ = rafQ.filter(([i]) => i !== id); };
const pump = (frames) => { for (let i = 0; i < frames && rafQ.length; i++) { clock += 16; const q = rafQ; rafQ = []; for (const [, f] of q) f(clock); } };

const store = () => { const m = new Map(); return { getItem: (k) => (m.has(k) ? m.get(k) : null), setItem: (k, v) => m.set(k, String(v)), removeItem: (k) => m.delete(k) }; };
let reducedMotion = false;
const toasts = [], pushed = [];
let graphData = null;
const ctx = {
  console, Math, Date, JSON, Map, Set, Promise, URLSearchParams, Number, String, Array, Object, Infinity, NaN, isFinite, parseFloat,
  performance, requestAnimationFrame, cancelAnimationFrame,
  window: { matchMedia: () => ({ matches: reducedMotion }), devicePixelRatio: 2 },
  matchMedia: () => ({ matches: reducedMotion }),
  document: { hidden: false, addEventListener() {}, removeEventListener() {}, createElement: el },
  localStorage: store(), sessionStorage: store(),
  ResizeObserver: class { constructor(cb) { this.cb = cb; this.on = true; } observe() { this.cb([]); } disconnect() { this.on = false; ctx.disconnects++; } },
  disconnects: 0,
  getComputedStyle: () => ({ getPropertyValue: () => "" }),
  location: { hash: "#graf" },
  history: { pushState: (s, ti, u) => pushed.push(u), replaceState: (s, ti, u) => pushed.push(u) },
  // Helper dari app.js / i18n.js.
  t: (s, vars) => (vars ? s.replace(/\{(\w+)\}/g, (m, k) => (k in vars ? String(vars[k]) : m)) : s),
  esc: (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c])),
  icon: (n) => `<svg data-i="${n}"></svg>`,
  $: (sel, root) => root.querySelector(sel),
  S: () => "s",
  api: async () => JSON.parse(JSON.stringify(graphData)),
  wikiLink: (x) => `<a>${x}</a>`, tagChip: (x) => `<a>#${x}</a>`, bindCites() {}, bindTagChips() {},
  showEntity() {}, projHref: (n) => `#proyek/${encodeURIComponent(n)}`, ago: () => "1 d ago",
  copyText() {}, toast: (m) => toasts.push(m), encodeURIComponent, decodeURIComponent,
};
vm.createContext(ctx);
vm.runInContext(SRC, ctx, { filename: "graph.js" });
const G = (name) => vm.runInContext(name, ctx);

// ---- graf contoh: 24 proyek, 4 tag, 5 entitas, keempat jenis sisi, 1 proyek "missing".
function sample(focus = null) {
  const now = Date.now(), nodes = [], edges = [];
  for (let i = 0; i < 24; i++) nodes.push({ id: `p:${i}`, label: `Project ${i}`, kind: "project", weight: 1 + (i % 7), has_meta: true, missing: false, degree: 0, last_at: new Date(now - (i < 12 ? 1 : 200) * DAY - i * 3600e3).toISOString() });
  nodes.push({ id: "p:ghost", label: "Ghost", kind: "project", weight: 0, has_meta: false, missing: true, degree: 0 });
  ["ops", "data", "web", "ml"].forEach((tg) => nodes.push({ id: `t:${tg}`, label: `#${tg}`, kind: "tag", weight: 0, has_meta: false, missing: false, degree: 0 }));
  for (let i = 0; i < 5; i++) nodes.push({ id: `e:${i}`, label: `Tool ${i}`, kind: "entity", entity_kind: "tool", entity_id: `ent${i}`, weight: 2, has_meta: false, missing: false, degree: 0 });
  for (let i = 0; i < 23; i++) edges.push({ from: `p:${i}`, to: `p:${i + 1}`, kind: "link", weight: 1 + (i % 3) });
  for (let i = 0; i < 24; i += 3) edges.push({ from: `p:${i}`, to: `p:${(i + 7) % 24}`, kind: "mention", weight: 2 });
  for (let i = 0; i < 24; i++) edges.push({ from: `p:${i}`, to: `t:${["ops", "data", "web", "ml"][i % 4]}`, kind: "tag", weight: 1 });
  edges.push({ from: "p:0", to: "t:data", kind: "tag", weight: 1 });
  for (let i = 0; i < 10; i++) edges.push({ from: `p:${i}`, to: `e:${i % 5}`, kind: "entity", weight: 3 });
  edges.push({ from: "p:3", to: "p:ghost", kind: "link", weight: 1 });
  for (const e of edges) for (const id of [e.from, e.to]) nodes.find((n) => n.id === id).degree++;
  return { nodes, edges, focus, truncated: false, total_nodes: nodes.length };
}
const prefs = (extra = {}) => ({ ...G("GRAPH_DEFAULTS"), ...extra });
const finiteOnScreen = (list) => list.every((n) => Number.isFinite(n.sx) && Number.isFinite(n.sy) && n.sx > -50 && n.sx < W + 50 && n.sy > -50 && n.sy < H + 50 && n.sr > 0);
const mount = (g, p, hooks = {}) => { const c = canvasEl(), tip = el(); return { c, tip, ctl: G("mountGraph")(c, g, p, tip, hooks) }; };

// 1. Mode bola 3D: tata letak di permukaan bola, terproyeksi di dalam kanvas.
{
  let selected = null;
  const p = prefs();
  const { c, ctl } = mount(sample(), p, { onSelect: (n) => { selected = n; } });
  pump(240);
  const v = ctl.view(), vis = ctl.nodes();
  ok(v.mode === "3d", "mode bawaan 3D");
  ok(vis.length === 34, `semua simpul terlihat (${vis.length})`);
  ok(vis.every((n) => Math.abs(Math.hypot(n.x, n.y, n.z) - v.R) < 1e-6 * v.R + 1e-6), "simpul tepat di permukaan bola");
  ok(finiteOnScreen(vis), "posisi layar finit dan di dalam kanvas (3D)");
  ok(vis.some((n) => n.depth < 0) && vis.some((n) => n.depth > 0), "ada sisi depan dan belakang bola");
  ok(c.ctxCalls > 100 && c.texts.length > 0, "kanvas digambar dengan label");
  ok(rafQ.length > 0, "putar otomatis menjaga animasi tetap jalan");
  const yaw0 = v.yaw; pump(5);
  ok(v.yaw !== yaw0, "bola berputar otomatis");
  // Simpul terdepan ditemukan uji klik.
  const front = vis.slice().sort((a, b) => a.depth - b.depth)[0];
  ok(ctl.hitTest(front.sx, front.sy) === front, "uji klik menemukan simpul terdepan");
  // Ketuk = pilih + putar ke depan; panel menerima simpul.
  const target = vis.filter((n) => n.kind === "project").sort((a, b) => b.depth - a.depth)[0];
  ctl.select(target.id);
  pump(60);
  ok(selected === target, "onSelect dipanggil dengan simpul terpilih");
  const minDepth = Math.min(...ctl.nodes().map((n) => n.depth));
  ok(Math.abs(target.depth - minDepth) < 1e-6 && Math.abs(target.sx - W / 2) < 2 && Math.abs(target.sy - H / 2) < 2, `simpul terpilih berputar ke depan tengah (depth ${target.depth.toFixed(1)}, min ${minDepth.toFixed(1)})`);
  // Pointer: ketuk latar membatalkan pilihan.
  c.onpointerdown({ pointerId: 1, clientX: 2, clientY: 2, button: 0 });
  c.onpointerup({ pointerId: 1, clientX: 2, clientY: 2 });
  ok(selected === null && ctl.selected() === null, "ketuk latar membatalkan pilihan");
  // Seret latar = orbit (yaw berubah), seret simpul tetap di permukaan bola.
  const yaw1 = v.yaw;
  c.onpointerdown({ pointerId: 1, clientX: 5, clientY: 300, button: 0 });
  c.onpointermove({ pointerId: 1, clientX: 60, clientY: 320 });
  c.onpointerup({ pointerId: 1, clientX: 60, clientY: 320 });
  ok(v.yaw !== yaw1, "seret latar memutar bola");
  const f2 = ctl.nodes().slice().sort((a, b) => a.depth - b.depth)[0];
  c.onpointerdown({ pointerId: 2, clientX: f2.sx, clientY: f2.sy, button: 0 });
  c.onpointermove({ pointerId: 2, clientX: f2.sx + 30, clientY: f2.sy + 10 });
  c.onpointerup({ pointerId: 2, clientX: f2.sx + 30, clientY: f2.sy + 10 });
  ok(Math.abs(Math.hypot(f2.x, f2.y, f2.z) - v.R) < 1e-6 * v.R + 1e-6 && !f2.fixed, "simpul diseret tetap di bola dan dilepas");
  // Ketuk ganda membuka simpul.
  let opened = null;
  const { c: c2, tip: tip2, ctl: ctl2 } = mount(sample(), prefs(), { onOpen: (n) => { opened = n; } });
  pump(240);
  const fr = ctl2.nodes().slice().sort((a, b) => a.depth - b.depth)[0];
  for (let i = 0; i < 2; i++) { c2.onpointerdown({ pointerId: 3, clientX: fr.sx, clientY: fr.sy, button: 0 }); c2.onpointerup({ pointerId: 3, clientX: fr.sx, clientY: fr.sy }); }
  ok(opened === fr, "ketuk ganda membuka simpul");
  // Keyboard: panah memilih simpul berderajat tertinggi, Escape menutup.
  let prevented = 0;
  const key = (k) => c2.onkeydown({ key: k, preventDefault() { prevented++; }, stopPropagation() {} });
  key("Escape");
  ok(ctl2.selected() === null && prevented === 1, "Escape menutup pilihan dari ketukan pertama");
  prevented = 0;
  key("ArrowRight");
  ok(ctl2.selected() && ctl2.selected().id === ctl2.nodes().slice().sort((a, b) => ctl2.model.neigh.get(b.id).size - ctl2.model.neigh.get(a.id).size || a.label.localeCompare(b.label))[0].id, "panah kanan memilih simpul pertama per derajat");
  key("Escape"); key("Escape");
  ok(ctl2.selected() === null, "Escape menutup pilihan");
  ok(prevented === 2, `Escape tanpa pilihan tidak ditelan (${prevented})`);
  // Tooltip sisi menjelaskan alasan tautan.
  const e0 = ctl2.model.visE.find((e) => e.kind === "mention" && e.s.depth < 0 && e.t.depth < 0) || ctl2.model.visE.find((e) => e.kind === "mention");
  const mx = (e0.s.sx + e0.t.sx) / 2, my = (e0.s.sy + e0.t.sy) / 2;
  if (!ctl2.hitTest(mx, my)) {
    c2.onpointermove({ pointerId: 9, clientX: mx, clientY: my, pointerType: "mouse" });
    ok(!tip2.hidden && tip2.innerHTML.includes("Mentioned in"), "hover sisi menampilkan alasan tautan");
  }
  ok(G("graphEdgeReason")(e0) === "Mentioned in 2 memories", "alasan sisi sebutan");

  // 2. Filter: tag mati -> simpul tag hilang; warna kelompok tetap ada.
  ctl.setFilters({ tag: false });
  ok(!ctl.nodes().some((n) => n.kind === "tag") && p.tag === false, "filter tag menyembunyikan simpul tag");
  ctl.setColor("tag");
  // p:1 dan p:5 bertag #data, p:2 bertag #web.
  const a = ctl.colorOf("p:1"), b = ctl.colorOf("p:2"), d = ctl.colorOf("p:5");
  ok(a.startsWith("hsl(") && a !== b, `warna kelompok berbeda per tag (${a} vs ${b})`);
  ok(a === d, "proyek satu kelompok berwarna sama");
  ok(ctl.colorOf("e:0") === ctl.style().entity, "entitas tetap berwarna jenisnya di mode tag");
  ok(ctl.model.groups.length === 4 && ctl.model.groupOf.get("p:0") === "#data", "proyek bertag ganda memakai tag terpopuler");
  ok(ctl.setGroup("#ops") === "#ops" && ctl.setGroup("#ops") === "", "sorot kelompok bisa dinyalakan & dimatikan");
  ctl.setFilters({ tag: true });

  // 3. Aktivitas: 30 hari menyembunyikan proyek basi dan proyek tanpa memori.
  ctl.setDays(30);
  const projs = ctl.nodes().filter((n) => n.kind === "project");
  ok(projs.length === 12 && !projs.some((n) => n.missing), `filter 30 hari (${projs.length} proyek)`);
  ok(Math.abs(Math.hypot(projs[0].x, projs[0].y, projs[0].z) - v.R) < 1e-6 * v.R + 1e-6, "bola diskalakan ulang setelah filter");
  ctl.setDays(0);
  ok(ctl.nodes().length === 34, "filter aktivitas bisa dikembalikan");

  // 4. Mode 2D: tetap finit, uji klik jalan, kembali ke 3D memulihkan posisi bola.
  const saved3d = ctl.nodes().map((n) => [n.id, n.x, n.y, n.z]);
  ctl.setMode("2d");
  pump(240);
  ok(ctl.view().mode === "2d" && p.mode === "2d", "pindah ke mode 2D");
  ok(ctl.nodes().every((n) => n.z === 0 && n.depth === 0 && n.fade === 1), "2D datar");
  ok(finiteOnScreen(ctl.nodes()), "posisi layar finit dan di dalam kanvas (2D)");
  const n2 = ctl.nodes()[5];
  ok(ctl.hitTest(n2.sx, n2.sy) !== null, "uji klik 2D");
  ctl.setMode("3d");
  const back = ctl.nodes().find((n) => n.id === saved3d[0][0]);
  ok(Math.abs(back.x - saved3d[0][1]) < 30 && Math.abs(back.z - saved3d[0][3]) < 30, "kembali ke 3D memulihkan posisi bola");

  // 5. Cari + Enter memilih simpul yang cocok.
  ctl.search("Project 7"); ctl.focusMatch();
  ok(ctl.selected() && ctl.selected().label === "Project 7", "cari lalu Enter memilih simpul");
  ctl.destroy();
  ok(ctx.disconnects >= 1, "destroy memutus ResizeObserver");
}

// 6. Graf lokal: fokus dipaku di depan, tidak berputar otomatis.
{
  rafQ = [];
  const { ctl } = mount(sample("p:5"), prefs());
  pump(240);
  const f = ctl.model.byId.get("p:5");
  ok(f.focus && f.depth < 0 && Math.abs(f.sx - W / 2) < 2, "fokus graf lokal di depan tengah");
  ok(rafQ.length === 0, "graf lokal tidak berputar otomatis (animasi berhenti setelah stabil)");
  ctl.destroy();
}

// 7. Gerak dikurangi: tata letak dihitung sinkron, tanpa rAF.
{
  rafQ = []; reducedMotion = true;
  const { ctl } = mount(sample(), prefs());
  ok(rafQ.length === 0, "prefers-reduced-motion: tanpa animasi");
  ok(finiteOnScreen(ctl.nodes()), "prefers-reduced-motion: posisi siap tanpa animasi");
  const n = ctl.nodes().find((x) => x.kind === "project");
  ctl.select(n.id);
  ok(Math.abs(n.sx - W / 2) < 2, "prefers-reduced-motion: putar ke simpul tanpa animasi");
  ctl.destroy(); reducedMotion = false;
}

// 8. Panel graf lengkap (toolbar, panel detail, legenda) tanpa galat.
{
  rafQ = []; graphData = sample();
  const box = el();
  await G("graphPanel")(box, { page: true });
  ok(box.innerHTML.includes("graph-canvas") && box.innerHTML.includes('data-gmode="3d"') && box.innerHTML.includes("data-gdays"), "panel graf dirender dengan kontrol 3D/2D & aktivitas");
  const box2 = el();
  await G("graphPanel")(box2, { project: "Project 5", depth: 2, page: true });
  ok(box2.innerHTML.includes("All projects") && !box2.innerHTML.includes("data-grotate"), "graf lokal: breadcrumb, tanpa tombol putar");
  // Panel detail simpul.
  const g = sample(), { ctl } = mount(g, prefs());
  pump(10);
  const aside = el();
  let focused = null;
  G("graphNodePanel")(aside, ctl.model.byId.get("p:3"), ctl, { onFocus: (name) => { focused = name; } });
  ok(!aside.hidden && aside.innerHTML.includes("Links to") && aside.innerHTML.includes("Ghost") && aside.innerHTML.includes("Focus here"), "panel detail berisi tetangga & aksi");
  aside.querySelector("[data-gfocus]").onclick();
  ok(focused === "Project 3", "Fokus di sini memanggil onFocus dengan nama proyek");
  const legend = el();
  for (const color of ["kind", "tag", "activity"]) { G("graphLegend")(legend, ctl, { color }); ok(legend.innerHTML.length > 20, `legenda mode ${color}`); }
  // Graf kosong menampilkan petunjuk.
  graphData = { nodes: [], edges: [], focus: null, truncated: false, total_nodes: 0 };
  const box3 = el();
  await G("graphPanel")(box3, {});
  ok(box3.innerHTML.includes("No links between projects yet."), "keadaan kosong");
  ctl.destroy();
}

if (failures) { console.error(`ui-graph-smoke: ${failures}/${checks} cek gagal`); process.exit(1); }
console.log(`ui-graph-smoke: ${checks} cek lolos`);
