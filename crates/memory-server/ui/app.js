// UI web lokal engram. Tanpa framework/build step; data selalu di-escape.
"use strict";

// Ikon SVG inline (stroke). Jangan pakai glyph Unicode sebagai ikon.
const ICONS = {
  book: '<path d="M4 4h10a4 4 0 0 1 4 4v12H8a4 4 0 0 1-4-4z"/><path d="M4 16a4 4 0 0 1 4-4h10"/>',
  clock: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
  users: '<circle cx="9" cy="8" r="3.5"/><path d="M2.5 20a6.5 6.5 0 0 1 13 0"/><circle cx="17" cy="9" r="2.5"/><path d="M15.5 14.5A5 5 0 0 1 21.5 19"/>',
  compass: '<circle cx="12" cy="12" r="9"/><path d="m15.5 8.5-2 5-5 2 2-5z"/>',
  search: '<circle cx="11" cy="11" r="6.5"/><path d="m20 20-4.2-4.2"/>',
  check: '<path d="m5 12.5 4.5 4.5L19 7.5"/>',
  x: '<path d="M6 6l12 12M18 6 6 18"/>',
  inbox: '<path d="M4 13h4l2 3h4l2-3h4"/><path d="M5 5h14l1 8v6H4v-6z"/>',
  mic: '<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  upload: '<path d="M12 16V4M7 9l5-5 5 5"/><path d="M4 16v4h16v-4"/>',
  flag: '<path d="M5 21V4h11l-1.5 4L16 12H5"/>',
  home: '<path d="M4 11 12 4l8 7v9H4z"/><path d="M10 20v-5h4v5"/>',
  plug: '<path d="M9 3v5M15 3v5"/><path d="M6 8h12v3a6 6 0 0 1-12 0z"/><path d="M12 17v4"/>',
  play: '<path d="M8 5v14l11-7z"/>',
  cloud: '<path d="M7 18a4.5 4.5 0 0 1-.6-8.96A6 6 0 0 1 18 8.5a4.5 4.5 0 0 1-.5 9.5z"/>',
  folder: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
  alert: '<path d="M12 4 2.5 20h19z"/><path d="M12 10v4M12 17v.5"/>',
  refresh: '<path d="M20 11a8 8 0 0 0-14.5-4.5L4 8M4 4v4h4"/><path d="M4 13a8 8 0 0 0 14.5 4.5L20 16M20 20v-4h-4"/>',
  settings: '<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 0 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 0 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 0 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 0 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z"/>',
  download: '<path d="M12 4v12M7 11l5 5 5-5"/><path d="M4 20h16"/>',
  external: '<path d="M14 4h6v6M20 4l-9 9"/><path d="M18 14v6H4V6h6"/>',
};
const icon = (n) => `<svg class="i" viewBox="0 0 24 24" aria-hidden="true">${ICONS[n] || ""}</svg>`;
const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const $ = (sel, el = document) => el.querySelector(sel);

const state = { subject: localStorage.getItem("subject") || "", subjects: [] };

function toast(msg) {
  const t = $("#toast");
  t.textContent = msg;
  t.classList.add("show");
  clearTimeout(t._h);
  t._h = setTimeout(() => t.classList.remove("show"), 2600);
}

async function api(path, opts = {}) {
  const headers = Object.assign({}, opts.headers || {});
  const token = localStorage.getItem("token");
  if (token) headers["Authorization"] = "Bearer " + token;
  if (opts.json !== undefined) {
    headers["Content-Type"] = "application/json";
    opts.body = JSON.stringify(opts.json);
  }
  const res = await fetch(path, Object.assign({}, opts, { headers }));
  if (res.status === 401) {
    askToken();
    throw new Error("token akses dibutuhkan");
  }
  const body = res.headers.get("content-type")?.includes("json") ? await res.json() : await res.text();
  if (!res.ok) throw new Error(body?.error || res.statusText);
  return body;
}
const S = () => encodeURIComponent(state.subject);

const ROUTES = [
  ["ringkasan", "home", "Ringkasan", viewHome],
  ["timeline", "clock", "Timeline", viewTimeline],
  ["orang", "users", "Orang & Silsilah", viewPeople],
  ["berpikir", "compass", "Cara berpikir", viewThinking],
  ["cari", "search", "Cari & tanya", viewSearch],
  ["wawancara", "mic", "Wawancara", viewInterview],
  ["review", "flag", "Review", viewReview],
  ["sumber", "inbox", "Sumber & import", viewSources],
];

function renderNav() {
  const cur = (location.hash.slice(1).split("/")[0]) || "ringkasan";
  $("#nav").innerHTML = ROUTES.map(([k, ic, label]) => `<a href="#${k}" class="${k === cur ? "active" : ""}">${icon(ic)}<span>${label}</span></a>`).join("");
}

function hydrateIcons(root = document) {
  root.querySelectorAll("[data-icon]").forEach((el) => { el.innerHTML = icon(el.dataset.icon); });
}

async function route() {
  renderNav();
  const [name, arg] = location.hash.slice(1).split("/");
  const r = ROUTES.find((x) => x[0] === (name || "ringkasan")) || ROUTES[0];
  const view = $("#view");
  if (!state.subject) {
    view.innerHTML = `<div class="empty"><h1>Selamat datang</h1><p>Mulai dengan menambahkan orang yang memorinya ingin disimpan.</p>
      <button class="primary" id="first-subject">${icon("plus")} Tambah subjek</button></div>`;
    $("#first-subject").onclick = newSubjectForm;
    return;
  }
  view.innerHTML = `<div class="empty">Memuat...</div>`;
  try { await r[3](view, arg && decodeURIComponent(arg)); }
  catch (e) { view.innerHTML = `<div class="card">Gagal memuat: ${esc(e.message)}</div>`; }
  view.focus();
}

async function loadSubjects() {
  const r = await api("/api/subjects");
  state.subjects = r.subjects;
  if (!state.subject || !r.subjects.some((s) => s.id === state.subject)) state.subject = r.subjects[0]?.id || "";
  $("#subject-select").hidden = r.subjects.length === 0;
  $("#subject-select").innerHTML = r.subjects.map((s) => `<option value="${esc(s.id)}" ${s.id === state.subject ? "selected" : ""}>${esc(s.name)}</option>`).join("");
}

// ---------------------------------------------------------------- drawer
function openDrawer(title, html) {
  $("#drawer-title").textContent = title;
  $("#drawer-body").innerHTML = html;
  $("#drawer").hidden = false;
  bindCites($("#drawer-body"));
}
$("#drawer-close").onclick = () => { $("#drawer").hidden = true; };
document.addEventListener("keydown", (e) => { if (e.key === "Escape") $("#drawer").hidden = true; });

function citeLinks(text) {
  return esc(text).replace(/\[ep:([a-z0-9]+)\]/g, (_, id) => `<a class="cite" data-ep="${id}">[ep:${id.slice(0, 6)}]</a>`);
}
function bindCites(root) {
  root.querySelectorAll("[data-ep]").forEach((a) => { a.onclick = (e) => { e.stopPropagation(); showEpisode(a.dataset.ep); }; });
  root.querySelectorAll("[data-entity]").forEach((a) => { a.onclick = (e) => { e.stopPropagation(); showEntity(a.dataset.entity); }; });
  root.querySelectorAll("[data-source]").forEach((a) => { a.onclick = (e) => { e.stopPropagation(); showSource(a.dataset.source); }; });
}

function factBadge(f) {
  if (f.status === "conflict") return `<span class="badge conflict">bertentangan</span>`;
  if (f.current) return `<span class="badge now">berlaku sekarang</span>`;
  if (f.valid_to) return `<span class="badge past">dulu</span>`;
  return "";
}
const yr = (d) => (d ? d.slice(0, 4) : "?");
function factLine(f) {
  const span = f.valid_to ? `${yr(f.valid_from)}–${yr(f.valid_to)}` : `sejak ${yr(f.valid_from)}`;
  const ev = (f.evidence || []).map((e) => `<a class="cite" data-ep="${esc(e.episode_id || e)}">bukti</a>`).join(" ");
  const statement = f.statement || `${f.predicate} ${f.object_text}`;
  return `<div class="fact">${factBadge(f)} ${esc(statement)} <span class="muted small">(${esc(span)}; ${esc(f.time?.kind ? "" : f.time || "")})</span> ${ev}</div>`;
}

async function showEpisode(id) {
  const r = await api(`/api/subjects/${S()}/episodes/${encodeURIComponent(id)}`);
  const e = r.episode;
  const ents = r.entities.map((x) => `<a class="cite" data-entity="${esc(x.id)}">${esc(x.name)}</a>`).join(", ");
  const facts = r.facts.map((f) => `<div class="fact">${f.valid_to ? '<span class="badge past">dulu</span>' : '<span class="badge now">berlaku</span>'} ${esc(f.predicate)} ${esc(f.object_text)} <span class="muted small">${esc(f.valid_from || "?")} – ${esc(f.valid_to || "")}</span><div class="quote">${esc(f.evidence?.[0]?.quote || "")}</div></div>`).join("");
  const dec = (e.decisions || []).map((d) => `<li>${esc(d.situation)}: <b>${esc(d.action)}</b>${d.reason ? " — " + esc(d.reason) : ""}</li>`).join("");
  const emo = (e.emotions || []).map((m) => `<span class="badge">${esc(m.label)}</span>`).join("");
  const retold = r.retellings.length ? `<p class="small muted">Diceritakan ulang ${r.retellings.length}x: ${r.retellings.map((x) => `<a class="cite" data-ep="${esc(x.id)}">${esc(x.created_at.slice(0, 10))}</a>`).join(", ")}</p>` : "";
  openDrawer(e.title || "Cerita", `
    <p class="muted small">${esc(labelTime(e))} · ${esc(e.life_phase || "fase tidak diketahui")} · bahasa: ${esc((e.languages || []).join(", "))}</p>
    ${e.retelling_of ? `<p class="small">Ini penceritaan ulang dari <a class="cite" data-ep="${esc(e.retelling_of)}">cerita asli</a>.</p>` : ""}
    <p>${esc(e.summary)}</p>${emo}
    ${dec ? `<h3>Keputusan</h3><ul>${dec}</ul>` : ""}
    ${facts ? `<h3>Fakta dari cerita ini</h3>${facts}` : ""}
    ${ents ? `<p><b>Disebut:</b> ${ents}</p>` : ""}${retold}
    <h3>Teks asli</h3><pre class="src">${esc(e.text)}</pre>
    ${r.source ? `<p class="small">Sumber: <a class="cite" data-source="${esc(r.source.id)}">${esc(r.source.title)}</a></p>` : ""}`);
}

function labelTime(e) {
  const t = e.time || {};
  switch (t.kind) {
    case "exact": return t.date;
    case "approx": return "sekitar " + (t.around || "").slice(0, 4);
    case "range": return `${yr(t.start)}–${t.ongoing ? "sekarang" : yr(t.end)}`;
    case "age": return t.to && t.to !== t.from ? `umur ${t.from}–${t.to}` : `umur ${t.from}`;
    case "relative": return `${t.offset_years} th setelah ${t.anchor}`;
    case "life_phase": return "masa " + t.phase;
    case "current": return "sekarang";
    case "past": return "dulu";
    default: return e.time_start ? e.time_start.slice(0, 4) : "waktu tidak diketahui";
  }
}

async function showEntity(id) {
  const w = await api(`/api/subjects/${S()}/entities/${encodeURIComponent(id)}`);
  const e = w.entity;
  const kin = w.kin.map((k) => `<li>${esc(k.relation)}: <a class="cite" data-entity="${esc(k.person_id)}">${esc(k.name)}</a>${k.inferred ? ' <span class="muted small">(turunan)</span>' : ""}</li>`).join("");
  openDrawer(e.name, `
    <p>${w.relation_to_subject ? `<span class="badge">${esc(w.relation_to_subject)}</span>` : ""}<span class="badge">${esc(e.kind)}</span><span class="badge ${e.status === "provisional" ? "warn" : ""}">${esc(e.status)}</span></p>
    ${e.aliases.length ? `<p class="small">Alias: ${esc(e.aliases.join(", "))}</p>` : ""}
    ${e.description ? `<p>${esc(e.description)}</p>` : ""}
    ${w.current_facts.length ? `<h3>Berlaku sekarang</h3>${w.current_facts.map(factLine).join("")}` : ""}
    ${w.history.length ? `<h3>Riwayat</h3>${w.history.map(factLine).join("")}` : ""}
    ${kin ? `<h3>Kekerabatan</h3><ul>${kin}</ul>` : ""}
    <h3>Cerita (${w.episodes.length})</h3>
    ${w.episodes.map((ep) => `<div class="tl-item" data-ep="${esc(ep.id)}"><span class="tl-time">${esc(ep.time)}</span><span>${esc(ep.title || ep.excerpt)}</span></div>`).join("") || '<p class="muted">Belum ada.</p>'}`);
}

async function showSource(id) {
  const r = await api(`/api/sources/${encodeURIComponent(id)}`);
  let text = esc(r.text);
  for (const q of [...new Set(r.quotes)].filter((q) => q && q.length > 3)) {
    const eq = esc(q);
    text = text.split(eq).join(`<mark>${eq}</mark>`);
  }
  const s = r.source;
  openDrawer(s.title, `
    <p class="small muted">${esc(s.kind)} · ${esc(s.status)} · ${esc((s.languages || []).join(", "))} · ${esc(s.created_at.slice(0, 10))}</p>
    <p class="small">Episode ${s.stats.episodes} · working memory ${s.stats.stream_items} · dibuang (noise) ${s.stats.dropped} · bukan tentang subjek ${s.stats.not_about_subject} · fakta ${s.stats.facts} · kutipan ditolak ${s.stats.facts_rejected}</p>
    ${s.error ? `<p class="badge conflict">${esc(s.error)}</p>` : ""}
    ${s.status === "failed" ? `<div class="row"><button id="src-retry">${icon("refresh")} Proses ulang</button><span class="muted small">Otomatis dicoba lagi saat server LLM &amp; embedding hidup.</span></div>` : ""}
    <h3>Episode dari sumber ini</h3>
    ${r.episodes.map((ep) => `<div class="tl-item" data-ep="${esc(ep.id)}"><span class="tl-time">${esc(labelTime(ep))}</span><span>${esc(ep.title)}</span></div>`).join("") || '<p class="muted">Belum ada.</p>'}
    <h3>Teks asli <span class="muted small">(sorotan = kutipan yang dipakai sebagai bukti)</span></h3><pre class="src">${text}</pre>`);
  const rb = $("#src-retry");
  if (rb) rb.onclick = async () => {
    rb.disabled = true;
    try { await api(`/api/sources/${encodeURIComponent(id)}/retry`, { method: "POST" }); toast("Masuk antrean"); showSource(id); }
    catch (e) { toast(e.message); rb.disabled = false; }
  };
}

// ---------------------------------------------------------------- views
async function viewHome(el) {
  const r = await api(`/api/subjects/${S()}/stats`);
  const st = r.stats;
  const items = [["Cerita", st.episodes], ["Diceritakan ulang", st.retellings], ["Working memory", st.stream_items], ["Orang & tempat", st.entities],
    ["Fakta", st.facts], ["Relasi keluarga", st.kin], ["Prinsip", st.principles], ["Sifat & nilai", st.dispositions], ["Menunggu review", st.open_reviews], ["Sumber", st.sources]];
  el.innerHTML = `<h1>${esc(r.subject?.name || state.subject)}</h1>
    <p class="muted">${r.subject?.anchors?.birth ? "Lahir " + esc(r.subject.anchors.birth) + " · " : ""}${r.pending_jobs} pekerjaan dalam antrean</p>
    <div class="grid">${items.map(([k, v]) => `<div class="card stat"><b>${v}</b><span>${k}</span></div>`).join("")}</div>
    <h2>Perawatan</h2>
    <div class="row">
      <button data-m="consolidate">${icon("refresh")} Konsolidasi</button>
      <button data-m="distill">${icon("compass")} Distilasi prinsip</button>
      <button data-m="voice">${icon("mic")} Profil gaya tutur</button>
      <button id="retry-failed">${icon("refresh")} Proses ulang yang gagal</button>
    </div>
    <h2>Data subjek</h2>
    <div class="card"><div class="row">
      <label>Tanggal lahir <input type="date" id="birth" value="${esc(r.subject?.anchors?.birth || "")}"></label>
      <button class="primary" id="save-birth">Simpan</button>
      <span class="muted small">Dipakai untuk menerjemahkan "umur 20" dan "waktu SD" menjadi tahun.</span>
    </div></div>`;
  el.querySelectorAll("[data-m]").forEach((b) => { b.onclick = async () => { await api(`/api/subjects/${S()}/maintenance/${b.dataset.m}`, { method: "POST" }); toast("Masuk antrean"); }; });
  $("#retry-failed").onclick = async () => {
    try {
      const r = await api("/api/jobs/retry-failed", { method: "POST" });
      toast(r.backend_error ? `${r.failed} gagal, server belum bisa dihubungi` : r.failed ? `${r.requeued} sumber diantre ulang` : "Tidak ada sumber yang gagal");
    } catch (e) { toast(e.message); }
  };
  $("#save-birth").onclick = async () => {
    await api("/api/subjects", { method: "POST", json: { id: state.subject, name: r.subject.name, birth: $("#birth").value } });
    toast("Tersimpan");
  };
}

async function viewTimeline(el) {
  const t = await api(`/api/subjects/${S()}/timeline?limit=2000`);
  const groups = new Map();
  for (const it of t.items) {
    const k = it.life_phase || "tanpa fase";
    if (!groups.has(k)) groups.set(k, []);
    groups.get(k).push(it);
  }
  const phaseSummary = new Map(t.phases.map((p) => [p.name, p.summary]));
  const fuzzy = (time) => !/^\d{2}-\d{2}-\d{4}$/.test(time);
  el.innerHTML = `<h1>Timeline</h1>
    <p class="muted small">Waktu bergaris putus-putus = perkiraan ("umur 20", "waktu SD", "sekitar 2015").</p>
    ${[...groups].map(([phase, items]) => `<section class="phase"><h2>${esc(phase)}</h2>
      ${phaseSummary.get(phase) ? `<p class="muted">${esc(phaseSummary.get(phase))}</p>` : ""}
      ${items.map((it) => `<div class="tl-item" ${it.kind === "episode" ? `data-ep="${esc(it.id)}"` : ""}><span class="tl-time ${fuzzy(it.time) ? "fuzzy" : ""}">${esc(it.time)}<br><span class="small">${esc(it.date || "")}</span></span><span>${esc(it.label)}</span></div>`).join("")}
    </section>`).join("") || '<div class="empty">Belum ada cerita bertanggal.</div>'}
    ${t.undated.length ? `<h2>Waktu belum diketahui (${t.undated.length})</h2>${t.undated.map((e) => `<div class="tl-item" data-ep="${esc(e.id)}"><span class="tl-time">?</span><span>${esc(e.title || e.excerpt)}</span></div>`).join("")}` : ""}`;
  bindCites(el);
}

async function viewPeople(el) {
  const [tree, ents] = await Promise.all([api(`/api/subjects/${S()}/family`), api(`/api/subjects/${S()}/entities?limit=500`)]);
  const self = tree.nodes.find((n) => n.is_self);
  const rel = new Map();
  if (self) for (const e of tree.edges) if (e.to === self.id && !rel.has(e.from)) rel.set(e.from, e);
  const gen = { 2: ["buyut"], 1: ["kakek", "nenek", "kakek/nenek"], 0.5: ["ayah", "ibu", "orang tua", "pakde", "bude", "paman", "bibi", "paman/bibi", "mertua", "orang tua angkat"],
    0: ["suami", "istri", "pasangan", "kakak", "adik", "kakak laki-laki", "kakak perempuan", "saudara kandung", "sepupu", "ipar", "besan"], "-1": ["anak", "menantu", "keponakan", "anak angkat"], "-2": ["cucu", "cicit"] };
  const level = (label) => Number(Object.keys(gen).find((k) => gen[k].includes(label)) ?? 0);
  const rows = {};
  for (const n of tree.nodes) {
    if (n.is_self) continue;
    const e = rel.get(n.id);
    if (!e) continue;
    const l = level(e.label);
    (rows[l] ||= []).push({ n, e });
  }
  const order = Object.keys(rows).map(Number).sort((a, b) => b - a);
  const person = ({ n, e }) => `<div class="person ${e.inferred ? "inferred" : ""}" data-entity="${esc(n.id)}"><b>${esc(n.name)}</b><small>${esc(e.label)}</small></div>`;
  const kinds = { person: "Orang", place: "Tempat", org: "Organisasi", event: "Peristiwa", object: "Benda", concept: "Konsep" };
  el.innerHTML = `<h1>Orang & Silsilah</h1>
    <div class="card">
      ${order.filter((l) => l > 0).map((l) => `<div class="tree-gen">${rows[l].map(person).join("")}</div>`).join("")}
      ${self ? `<div class="tree-gen"><div class="person self" data-entity="${esc(self.id)}"><b>${esc(self.name)}</b><small>subjek</small></div>${(rows[0] || []).map(person).join("")}</div>` : ""}
      ${order.filter((l) => l < 0).map((l) => `<div class="tree-gen">${rows[l].map(person).join("")}</div>`).join("")}
      ${tree.edges.length ? '<p class="muted small">Kotak bergaris putus-putus = hubungan turunan (disimpulkan dari relasi lain).</p>' : '<p class="empty">Belum ada relasi keluarga.</p>'}
    </div>
    <div class="row"><input type="search" id="ent-q" placeholder="Saring nama..."></div>
    <div id="ent-list"></div>`;
  const draw = () => {
    const q = $("#ent-q").value.toLowerCase();
    const list = ents.filter((e) => !q || e.name.toLowerCase().includes(q) || e.aliases.some((a) => a.toLowerCase().includes(q)));
    const by = {};
    for (const e of list) (by[e.kind] ||= []).push(e);
    $("#ent-list").innerHTML = Object.entries(by).map(([k, xs]) => `<h2>${kinds[k] || k} (${xs.length})</h2><table>${xs.map((e) => `<tr data-entity="${esc(e.id)}" style="cursor:pointer"><td><b>${esc(e.name)}</b> ${e.status === "provisional" ? '<span class="badge warn">baru sekali disebut</span>' : ""}</td><td class="small muted">${esc(e.aliases.join(", "))}</td><td class="small">${e.mention_count} cerita</td></tr>`).join("")}</table>`).join("");
    bindCites($("#ent-list"));
  };
  $("#ent-q").oninput = draw;
  draw();
  bindCites(el);
}

async function viewThinking(el) {
  const [ps, ds, voice] = await Promise.all([api(`/api/subjects/${S()}/principles`), api(`/api/subjects/${S()}/dispositions`), api(`/api/subjects/${S()}/voice`)]);
  const years = ds.flatMap((d) => [d.valid_from, d.valid_to].filter(Boolean).map((x) => +x.slice(0, 4)));
  const now = new Date().getFullYear();
  const min = Math.min(...years, now - 10), max = now;
  const pos = (y) => ((y - min) / Math.max(1, max - min)) * 100;
  el.innerHTML = `<h1>Cara berpikir</h1>
    <h2>Prinsip hidup</h2>
    ${ps.map((p) => `<div class="card"><h3>${esc(p.principle.statement)}</h3>
      <p class="small muted">${esc(p.principle.domain)}${p.principle.origin ? " · asal: " + esc(p.principle.origin) : ""} · keyakinan ${(p.principle.confidence * 100).toFixed(0)}% · ${p.evidence.length} cerita pendukung</p>
      ${p.evidence.map((e) => `<div class="tl-item" data-ep="${esc(e.id)}"><span class="tl-time">${esc(e.time)}</span><span>${esc(e.title || e.excerpt)}</span></div>`).join("")}</div>`).join("") || '<p class="muted">Belum ada prinsip yang cukup bukti (minimal beberapa cerita dari periode berbeda).</p>'}
    <h2>Sifat & nilai dari waktu ke waktu</h2>
    ${ds.map((d) => {
      const a = d.valid_from ? pos(+d.valid_from.slice(0, 4)) : 0, b = d.valid_to ? pos(+d.valid_to.slice(0, 4)) : 100;
      return `<div class="card"><b>${esc(d.label)}</b> <span class="badge">${d.kind === "value" ? "nilai" : "sifat"}</span>${d.status === "pending_review" ? '<span class="badge warn">menunggu konfirmasi</span>' : ""}
        <div class="bar" title="${esc(d.valid_from || "?")} – ${esc(d.valid_to || "sekarang")}"><span style="left:${a}%;width:${Math.max(2, b - a)}%"></span></div>
        <div class="small muted">${esc(yr(d.valid_from))} – ${d.valid_to ? esc(yr(d.valid_to)) : "sekarang"} · ${d.evidence.length} bukti</div><p>${esc(d.description)}</p></div>`;
    }).join("") || '<p class="muted">Belum ada.</p>'}
    <h2>Gaya bertutur</h2>
    ${voice ? `<div class="card"><p>${esc(voice.style || "Belum dideskripsikan.")}</p>
      <p>${(voice.idioms || []).map(([k, n]) => `<span class="badge">${esc(k)} (${n}x)</span>`).join(" ")}</p>
      <p class="small muted">Bahasa: ${Object.entries(voice.language_mix || {}).map(([k, v]) => `${esc(k)} ${(v * 100).toFixed(0)}%`).join(", ")}</p>
      ${(voice.sample_quotes || []).slice(0, 5).map((q) => `<div class="quote">${esc(q)}</div>`).join("")}</div>` : '<p class="muted">Jalankan "Profil gaya tutur" di Ringkasan.</p>'}`;
  bindCites(el);
}

async function viewSearch(el) {
  el.innerHTML = `<h1>Cari & tanya</h1>
    <div class="card">
      <div class="row"><input type="search" id="q" placeholder="mis. Partai apa yang didukung? Siapa Pakde Karno?"><button class="primary" id="go-recall">${icon("search")} Cari</button><button id="go-ask">Tanya</button></div>
      <div class="row small" style="margin-top:8px">
        <label><input type="checkbox" id="persona"> Jawab sebagai subjek</label>
        <label>Per tanggal <input type="date" id="asof"></label>
        <label>Dari <input type="date" id="from"></label><label>Sampai <input type="date" id="to"></label>
      </div>
    </div><div id="out"></div>`;
  const out = $("#out");
  const body = () => ({ query: $("#q").value, as_of: $("#asof").value || null, from: $("#from").value || null, to: $("#to").value || null });
  $("#go-recall").onclick = async () => {
    if (!$("#q").value.trim()) return;
    out.innerHTML = '<div class="empty">Mencari...</div>';
    const r = await api(`/api/subjects/${S()}/recall`, { method: "POST", json: body() });
    if (!r.found) { out.innerHTML = `<div class="card">${esc(r.note)}</div>`; return; }
    out.innerHTML = `${r.facts.length ? `<h2>Fakta</h2><div class="card">${r.facts.map(factLine).join("")}</div>` : ""}
      ${r.principles.length ? `<h2>Prinsip terkait</h2>${r.principles.map((p) => `<div class="card">${esc(p.statement)}</div>`).join("")}` : ""}
      <h2>Cerita</h2>${r.episodes.map((e) => `<div class="card tl-item" data-ep="${esc(e.id)}"><span class="tl-time">${esc(e.time)}${e.retell_count ? `<br><span class="small">diceritakan ${e.retell_count + 1}x</span>` : ""}</span><span><b>${esc(e.title)}</b><br>${esc(e.summary || e.excerpt)}<br><span class="small muted">via ${esc(e.via.join(", "))}</span></span></div>`).join("")}`;
    bindCites(out);
  };
  $("#go-ask").onclick = async () => {
    if (!$("#q").value.trim()) return;
    out.innerHTML = '<div class="empty">Berpikir...</div>';
    const r = await api(`/api/subjects/${S()}/ask`, { method: "POST", json: { question: $("#q").value, persona: $("#persona").checked, as_of: $("#asof").value || null } });
    out.innerHTML = `<div class="card"><div class="answer">${citeLinks(r.answer)}</div>${r.found ? "" : '<p class="muted small">Tidak ada catatan yang relevan; jawaban tidak dikarang.</p>'}</div>
      <details class="card"><summary>Konteks memori yang dipakai</summary><pre class="src">${citeLinks(r.context)}</pre></details>`;
    bindCites(out);
  };
  $("#q").onkeydown = (e) => { if (e.key === "Enter") $("#go-recall").click(); };
}

async function viewInterview(el) {
  el.innerHTML = `<h1>Wawancara terpandu</h1><p class="muted">Pertanyaan dipilih dari celah memori: fase hidup yang ceritanya masih sedikit, atau orang yang baru disebut sekali.</p><div id="iv"><div class="empty">Menyiapkan pertanyaan...</div></div>`;
  const next = async () => {
    const q = await api(`/api/subjects/${S()}/interview/next`);
    $("#iv").innerHTML = `<div class="card"><p class="small muted">${esc(q.reason)}</p><h2 style="margin-top:4px">${esc(q.question)}</h2>
      <textarea id="ans" placeholder="Ketik jawaban apa adanya; campur bahasa daerah tidak masalah."></textarea>
      <div class="row" style="margin-top:8px"><button class="primary" id="send">${icon("check")} Simpan jawaban</button><button id="skip">Pertanyaan lain</button></div></div>`;
    $("#send").onclick = async () => {
      if (!$("#ans").value.trim()) return;
      await api(`/api/subjects/${S()}/interview/answer`, { method: "POST", json: { question: q.question, answer: $("#ans").value } });
      toast("Jawaban disimpan dan sedang diproses");
      next();
    };
    $("#skip").onclick = next;
  };
  await next();
}

async function viewReview(el) {
  const items = await api(`/api/subjects/${S()}/reviews`);
  const kinds = { gray_zone: "Layak jadi cerita?", conflict: "Fakta bertentangan", entity_merge: "Orang yang sama?", quarantine: "Fakta meragukan", reflective: "Konfirmasi prinsip/sifat" };
  const help = { gray_zone: "Setuju = angkat jadi cerita. Tolak = biarkan meluruh.", conflict: "Setuju = fakta baru menggantikan yang lama. Tolak = fakta baru dibuang.",
    entity_merge: "Setuju = gabungkan. Tolak = orang berbeda.", quarantine: "Setuju = fakta dipakai. Tolak = dibuang.", reflective: "Setuju = aktif. Tolak = dibuang." };
  el.innerHTML = `<h1>Review (${items.length})</h1><p class="muted">Keputusan Anda juga menjadi contoh bagi sistem untuk menilai data berikutnya.</p>
    ${items.map((r) => `<div class="card" id="r-${esc(r.id)}"><span class="badge">${esc(kinds[r.kind] || r.kind)}</span><p>${esc(r.summary)}</p>
      ${r.payload?.text ? `<pre class="src">${esc(r.payload.text)}</pre>` : ""}<p class="small muted">${esc(help[r.kind] || "")}</p>
      <div class="row"><input type="text" placeholder="Catatan (opsional)" data-note><button class="primary" data-d="approve" data-id="${esc(r.id)}">${icon("check")} Setuju</button><button data-d="reject" data-id="${esc(r.id)}">${icon("x")} Tolak</button></div></div>`).join("") || '<div class="empty">Tidak ada yang perlu ditinjau.</div>'}`;
  el.querySelectorAll("[data-d]").forEach((b) => {
    b.onclick = async () => {
      const card = b.closest(".card");
      b.disabled = true;
      try {
        await api(`/api/reviews/${encodeURIComponent(b.dataset.id)}`, { method: "POST", json: { decision: b.dataset.d, note: card.querySelector("[data-note]").value || null } });
        card.remove();
        toast("Tersimpan");
      } catch (e) { toast(e.message); b.disabled = false; }
    };
  });
}

async function viewSources(el) {
  const [sources, batches] = await Promise.all([api(`/api/subjects/${S()}/sources?limit=300`), api(`/api/subjects/${S()}/batches`)]);
  el.innerHTML = `<h1>Sumber & import</h1>
    <div class="card"><div class="drop" id="drop">${icon("upload")} Seret berkas ke sini atau <label class="btn">pilih berkas<input type="file" id="files" multiple hidden></label>
      <p class="small">Teks/Markdown, ekspor WhatsApp (.txt), Telegram (result.json), ChatGPT/Claude (conversations.json), email (.mbox/.eml), foto + caption (.jpg + .txt bernama sama), audio (butuh Whisper).</p>
      <div class="row" style="justify-content:center"><label>Jenis untuk teks biasa <select id="kind"><option value="">otomatis</option><option value="interview">wawancara</option><option value="memoir">memoar</option><option value="note">catatan</option><option value="lifelog">lifelog</option></select></label></div></div>
      <p class="small muted">Bisa juga taruh berkas di folder <code>vault/inbox/</code> (subfolder <code>inbox/&lt;id-subjek&gt;/</code> untuk subjek lain).</p></div>
    <h2>Import terakhir</h2><div id="batches">${batches.map(batchRow).join("") || '<p class="muted">Belum ada.</p>'}</div>
    <h2>Sumber (${sources.length})</h2>
    <table><tr><th>Judul</th><th>Jenis</th><th>Status</th><th>Hasil</th></tr>${sources.map((s) => `<tr data-source="${esc(s.id)}" style="cursor:pointer"><td>${esc(s.title)}<div class="small muted">${esc(s.created_at.slice(0, 10))}${s.occurred_from ? " · terjadi " + esc(s.occurred_from) : ""}</div></td><td>${esc(s.kind)}</td><td><span class="badge ${s.status === "failed" ? "conflict" : s.status === "done" ? "now" : ""}">${esc(s.status)}</span></td><td class="small">${s.stats.episodes} cerita, ${s.stats.stream_items} WM, ${s.stats.dropped} noise</td></tr>`).join("")}</table>`;
  bindCites(el);
  const drop = $("#drop");
  const upload = async (files) => {
    if (!files.length) return;
    const fd = new FormData();
    if ($("#kind").value) fd.append("kind", $("#kind").value);
    fd.append("label", `upload ${new Date().toLocaleString("id-ID")}`);
    for (const f of files) fd.append("file", f, f.name);
    toast(`Mengunggah ${files.length} berkas...`);
    try {
      const r = await api(`/api/subjects/${S()}/import`, { method: "POST", body: fd });
      toast(`${r.queued} diantrekan, ${r.duplicates} duplikat, ${r.failed.length} gagal`);
      pollBatch(r.batch_id);
    } catch (e) { toast(e.message); }
  };
  $("#files").onchange = (e) => upload([...e.target.files]);
  drop.ondragover = (e) => { e.preventDefault(); drop.classList.add("over"); };
  drop.ondragleave = () => drop.classList.remove("over");
  drop.ondrop = (e) => { e.preventDefault(); drop.classList.remove("over"); upload([...e.dataTransfer.files]); };
}

function batchRow(b) {
  const pct = b.total ? Math.round(((b.done + b.failed) / b.total) * 100) : 100;
  return `<div class="card" id="b-${esc(b.id)}"><div class="row" style="justify-content:space-between"><b>${esc(b.label)}</b><span class="small muted">${b.done}/${b.total} selesai · ${b.failed} gagal · ${b.duplicates} duplikat</span></div>
    <div class="progress"><span style="width:${pct}%"></span></div>${(b.failures || []).slice(0, 5).map((f) => `<div class="small" style="color:var(--bad)">${esc(f)}</div>`).join("")}</div>`;
}

async function pollBatch(id) {
  for (let i = 0; i < 600; i++) {
    const r = await api(`/api/batches/${encodeURIComponent(id)}`);
    const el = document.getElementById("b-" + id);
    if (el) el.outerHTML = batchRow(r.batch);
    else if (location.hash.startsWith("#sumber")) $("#batches").insertAdjacentHTML("afterbegin", batchRow(r.batch));
    if (r.batch.done + r.batch.failed >= r.batch.total) { toast("Import selesai diproses"); return; }
    await new Promise((res) => setTimeout(res, 3000));
  }
}

// ---------------------------------------------------------------- boot
$("#subject-select").onchange = (e) => { state.subject = e.target.value; localStorage.setItem("subject", state.subject); route(); };
$("#new-subject").onclick = () => newSubjectForm();
$("#connect-agents").onclick = () => connectAgents();
$("#data-sync").onclick = () => showDataSync();
$("#services-btn").onclick = () => showServices();
$("#settings-btn").onclick = () => showSettings();

// Status layanan latar (server, worker, scheduler, inbox). Hanya ada di aplikasi desktop.
const svc = { list: [] };
async function pollServices() {
  try {
    const r = await api("/api/services");
    if (!r.enabled) { $("#services-btn").hidden = true; return; }
    svc.list = r.services;
  } catch (e) {
    // Server tidak menjawab: tandai semua layanan tidak diketahui.
    svc.list = svc.list.map((s) => Object.assign({}, s, { running: false }));
  }
  renderServicesBadge();
  if (!$("#drawer").hidden && $("#svc-list")) renderServices();
}
function renderServicesBadge() {
  const up = svc.list.filter((s) => s.running).length, all = svc.list.length;
  if (!all) return;
  $("#services-btn").hidden = false;
  $("#services-dot").className = "dot " + (up === all ? "ok" : "bad");
  $("#services-label").textContent = up === all ? "Layanan aktif" : `${all - up} layanan berhenti`;
  $("#services-btn").title = svc.list.map((s) => `${s.name}: ${s.running ? "berjalan" : "berhenti"}`).join("\n");
}
function showServices() {
  openDrawer("Status layanan", `<p class="muted small">Layanan latar engram di aplikasi ini. Layanan yang berhenti bisa dijalankan lagi dari sini atau dari ikon menu bar.</p><div id="svc-list"></div>`);
  renderServices();
  pollServices();
}
function renderServices() {
  $("#svc-list").innerHTML = svc.list.map((s) => `<div class="card" data-svc="${esc(s.id)}">
      <div class="row" style="justify-content:space-between">
        <div class="row"><span class="dot ${s.running ? "ok" : "bad"}"></span><b>${esc(s.name)}</b>
          <span class="badge ${s.running ? "now" : "conflict"}">${s.running ? "Berjalan" : "Berhenti"}</span></div>
        ${s.running ? "" : `<button class="primary" data-start>${icon("play")} Jalankan</button>`}
      </div>
      <div class="small muted">${esc(s.description)}</div>
      <div class="small msg"></div>
    </div>`).join("");
  $("#svc-list").querySelectorAll("[data-start]").forEach((b) => {
    b.onclick = async () => {
      const card = b.closest("[data-svc]");
      b.disabled = true;
      try {
        const r = await api(`/api/services/${encodeURIComponent(card.dataset.svc)}/start`, { method: "POST" });
        svc.list = r.services;
        renderServicesBadge();
        renderServices();
        toast("Layanan dijalankan");
      } catch (e) {
        card.querySelector(".msg").innerHTML = `<span style="color:var(--bad)">${esc(e.message)}</span>`;
        b.disabled = false;
      }
    };
  });
}

// Pasang server MCP ke agent AI (Antigravity, VS Code, Claude Code) dengan satu klik.
const MCP_STATE = {
  installed: ["now", "Terpasang"],
  outdated: ["warn", "Perlu diperbarui"],
  missing: ["", "Belum terpasang"],
  unreadable: ["conflict", "Config tidak terbaca"],
};
async function connectAgents() {
  openDrawer("Hubungkan agent AI", `<div class="empty">Memuat...</div>`);
  let r;
  try { r = await api("/api/mcp/clients"); }
  catch (e) { $("#drawer-body").innerHTML = `<div class="card">Gagal memuat: ${esc(e.message)}</div>`; return; }
  if (!r.enabled) {
    $("#drawer-body").innerHTML = `<div class="card"><p>Pemasangan otomatis hanya tersedia di aplikasi desktop atau <code>engram serve</code> di 127.0.0.1.</p>
      <p class="small muted">Pakai <code>scripts/install-mcp.sh</code> untuk server lain.</p></div>`;
    return;
  }
  $("#drawer-body").innerHTML = `
    <p class="muted small">Daftarkan memori ini sebagai server MCP <b>${esc(r.name)}</b> agar agent AI bisa mengingat dan mencari cerita.
      Alamat: <code>${esc(r.url)}</code></p>
    ${r.clients.map((c) => {
      const [cls, label] = MCP_STATE[c.state] || ["", c.state];
      const has = c.state === "installed" || c.state === "outdated";
      return `<div class="card" data-client="${esc(c.id)}">
        <div class="row" style="justify-content:space-between">
          <div><b>${esc(c.label)}</b> <span class="badge ${cls}">${esc(label)}</span>${c.detected ? "" : ' <span class="small muted">(tidak terdeteksi)</span>'}</div>
          <div class="row">
            ${has ? `<button data-act="uninstall" class="ghost" title="Hapus dari ${esc(c.label)}">${icon("x")}</button>` : ""}
            <button data-act="install" class="${c.state === "installed" ? "" : "primary"}">${icon(c.state === "installed" ? "refresh" : "plug")} ${c.state === "installed" ? "Pasang ulang" : c.state === "outdated" ? "Perbarui" : "Pasang"}</button>
          </div>
        </div>
        <div class="small muted">${esc(c.config_path)}</div>
        <div class="small msg"></div>
      </div>`;
    }).join("")}
    <p class="small muted">Token akses ikut ditulis ke config tersebut. Cadangan berkas lama disimpan sebagai <code>.bak</code>.</p>`;
  $("#drawer-body").querySelectorAll("[data-act]").forEach((b) => {
    b.onclick = async () => {
      const card = b.closest("[data-client]");
      card.querySelectorAll("button").forEach((x) => { x.disabled = true; });
      try {
        const res = await api(`/api/mcp/clients/${encodeURIComponent(card.dataset.client)}/${b.dataset.act}`, { method: "POST" });
        toast(b.dataset.act === "install" ? "MCP terpasang" : "MCP dihapus");
        await connectAgents();
        const msg = $(`[data-client="${CSS.escape(card.dataset.client)}"] .msg`);
        if (msg) msg.textContent = res.message;
      } catch (e) {
        card.querySelector(".msg").innerHTML = `<span style="color:var(--bad)">${esc(e.message)}</span>`;
        card.querySelectorAll("button").forEach((x) => { x.disabled = false; });
      }
    };
  });
}

// ---------------------------------------------------------------- lokasi data & sinkronisasi
const fmtTime = (t) => (t ? new Date(t).toLocaleString("id-ID") : "belum pernah");
const errLine = (m) => `<span style="color:var(--bad)">${esc(m)}</span>`;

async function pickFolder(title) {
  const r = await api("/api/pick-folder", { method: "POST", json: { title } });
  return r.path || "";
}

async function showDataSync() {
  openDrawer("Data & Sync", `<div class="empty">Memuat...</div>`);
  let storage, sync;
  try { [storage, sync] = await Promise.all([api("/api/storage"), api("/api/sync")]); }
  catch (e) { $("#drawer-body").innerHTML = `<div class="card">Gagal memuat: ${esc(e.message)}</div>`; return; }
  const can = storage.can_change;
  const pickBtn = (id) => (can ? `<button type="button" id="${id}">${icon("folder")} Pilih folder</button>` : "");
  const intervals = [[0, "Manual saja"], [5, "Tiap 5 menit"], [10, "Tiap 10 menit"], [30, "Tiap 30 menit"], [60, "Tiap jam"]];
  const cur = sync.settings.interval_minutes;
  if (!intervals.some(([m]) => m === cur)) intervals.push([cur, `Tiap ${cur} menit`]);
  $("#drawer-body").innerHTML = `
    <h2 style="margin-top:0">${icon("folder")} Lokasi data</h2>
    <div class="card">
      <div class="small muted">Database dan berkas sumber disimpan di:</div>
      <p><code>${esc(storage.vault)}</code></p>
      ${storage.in_cloud_folder ? `<p class="small" style="color:var(--warn)">${icon("alert")} Folder ini disinkron layanan cloud. Database bisa rusak; pindahkan ke folder biasa dan pakai Sinkronisasi di bawah.</p>` : ""}
      ${can ? `
        <div class="row"><input type="text" id="mv-path" placeholder="Folder baru, mis. /Users/nama/Documents/engram">${pickBtn("mv-pick")}</div>
        <p class="small muted">Data disalin ke folder baru, lalu aplikasi dibuka ulang. Folder lama tidak dihapus. Bila folder berisi berkas lain, dibuat subfolder <code>engram-vault</code>. Bila folder sudah berisi data engram, data itu yang dipakai.</p>
        <div id="mv-confirm"></div>
        <button class="primary" id="mv-go">${icon("upload")} Pindahkan data</button>
        <div class="small msg" id="mv-msg"></div>`
      : `<p class="small muted">Untuk memindah, hentikan server lalu jalankan <code>engram vault move &lt;folder&gt;</code>.</p>`}
    </div>

    <h2>${icon("cloud")} Sinkronisasi Google Drive</h2>
    <p class="small muted">Pasang <b>Google Drive for desktop</b> di setiap komputer, lalu pilih folder yang <b>sama</b> di My Drive dan isi kata sandi yang <b>sama</b>.
      Tiap komputer menyimpan salinan terenkripsi di folder itu dan menggabungkan data komputer lain. Database tetap di lokasi data di atas.</p>
    <form id="sync-form" class="card">
      <p><label>Folder sync di Google Drive<br>
        <span class="row"><input type="text" id="sy-dir" value="${esc(sync.settings.dir || "")}" placeholder="mis. .../Google Drive/My Drive/engram-sync">${pickBtn("sy-pick")}</span></label></p>
      ${sync.suggestions.length ? `<p class="small muted">Terdeteksi: ${sync.suggestions.map((p) => `<button type="button" class="ghost small" data-suggest="${esc(p)}">${icon("folder")} ${esc(p)}</button>`).join(" ")}</p>` : ""}
      ${sync.configured && !sync.dir_ok ? `<p class="small" style="color:var(--bad)">${icon("alert")} Folder tidak bisa diakses. Pastikan Google Drive berjalan.</p>` : ""}
      <p><label>Kata sandi enkripsi (sama di semua komputer)<br>
        <input type="password" id="sy-pass" autocomplete="new-password" style="width:100%" placeholder="${sync.encrypted ? "tersimpan - kosongkan bila tidak diubah" : "minimal 8 karakter"}"></label></p>
      ${sync.encrypted ? "" : `<p class="small" style="color:var(--warn)">${icon("alert")} Tanpa kata sandi, isi memori tersimpan apa adanya di Google Drive.</p>`}
      <div class="row">
        <label>Jadwal<br><select id="sy-int">${intervals.map(([m, l]) => `<option value="${m}" ${m === cur ? "selected" : ""}>${esc(l)}</option>`).join("")}</select></label>
        <label style="flex:1">Nama komputer ini<br><input type="text" id="sy-name" value="${esc(sync.settings.device_name || "")}" placeholder="${esc(sync.device_name)}" style="width:100%"></label>
      </div>
      <p class="small msg" id="sy-msg"></p>
      <div class="row">
        <button class="primary" type="submit">${icon("check")} Simpan</button>
        ${sync.configured ? `<button type="button" id="sy-run">${icon("refresh")} Sync sekarang</button><button type="button" id="sy-off" class="ghost">${icon("x")} Matikan sync</button>` : ""}
      </div>
    </form>
    ${sync.configured ? `<div class="card">
      <div class="small">Sync terakhir: <b>${esc(fmtTime(sync.last_sync_at))}</b>${sync.pending_embed ? ` &middot; ${sync.pending_embed} record menunggu server embedding` : ""}</div>
      ${sync.last_error ? `<p class="small">${errLine("Galat: " + sync.last_error)}</p>` : ""}
      ${sync.last_warnings.map((w) => `<div class="small" style="color:var(--warn)">${icon("alert")} ${esc(w)}</div>`).join("")}
      ${sync.devices.length ? `<table><tr><th>Komputer</th><th>Record</th><th>Diperbarui</th></tr>
        ${sync.devices.map((d) => `<tr><td>${esc(d.name)}${d.this_device ? ' <span class="badge now">ini</span>' : ""}${d.encrypted ? "" : ' <span class="badge warn">tanpa enkripsi</span>'}</td><td>${d.rows}</td><td class="small">${esc(fmtTime(d.updated_at))}</td></tr>`).join("")}</table>`
      : `<p class="small muted">Belum ada data di folder sync. Klik Sync sekarang.</p>`}
    </div>` : ""}`;

  const msg = (sel, html) => { $(sel).innerHTML = html; };
  const bindPick = (btnId, inputId, title) => {
    const b = $("#" + btnId);
    if (!b) return;
    b.onclick = async () => {
      try { const p = await pickFolder(title); if (p) $("#" + inputId).value = p; }
      catch (e) { toast(e.message); }
    };
  };
  bindPick("mv-pick", "mv-path", "Pilih folder baru untuk data engram");
  bindPick("sy-pick", "sy-dir", "Pilih folder di Google Drive");
  $("#drawer-body").querySelectorAll("[data-suggest]").forEach((b) => { b.onclick = () => { $("#sy-dir").value = b.dataset.suggest; }; });

  // Pindah lokasi data: konfirmasi di halaman (WebView tidak menampilkan confirm()).
  if ($("#mv-go")) {
    $("#mv-go").onclick = () => {
      const path = $("#mv-path").value.trim();
      if (!path) { msg("#mv-msg", errLine("Pilih atau ketik folder tujuan.")); return; }
      $("#mv-confirm").innerHTML = `<div class="card" style="background:var(--accent-soft)">
        <p class="small">Data akan disalin ke <code>${esc(path)}</code> lalu aplikasi dibuka ulang. Lanjutkan?</p>
        <div class="row"><button class="primary" id="mv-yes">${icon("check")} Ya, pindahkan</button><button id="mv-no">${icon("x")} Batal</button></div></div>`;
      $("#mv-no").onclick = () => { $("#mv-confirm").innerHTML = ""; };
      $("#mv-yes").onclick = async () => {
        $("#mv-yes").disabled = true;
        try {
          const r = await api("/api/storage/move", { method: "POST", json: { path } });
          $("#mv-confirm").innerHTML = "";
          msg("#mv-msg", esc(r.message) + ". Tunggu sebentar...");
        } catch (e) { $("#mv-confirm").innerHTML = ""; msg("#mv-msg", errLine(e.message)); }
      };
    };
  }

  const saveSync = async (body, okText) => {
    try {
      await api("/api/sync", { method: "PUT", json: body });
      toast(okText);
      await showDataSync();
    } catch (e) { msg("#sy-msg", errLine(e.message)); }
  };
  $("#sync-form").onsubmit = (e) => {
    e.preventDefault();
    const dir = $("#sy-dir").value.trim();
    if (!dir) { msg("#sy-msg", errLine("Pilih folder sync di Google Drive.")); return; }
    const body = { dir, interval_minutes: Number($("#sy-int").value), device_name: $("#sy-name").value.trim() };
    const pass = $("#sy-pass").value;
    if (pass) body.passphrase = pass;
    saveSync(body, "Pengaturan sync disimpan");
  };
  if ($("#sy-off")) $("#sy-off").onclick = () => saveSync({ dir: null }, "Sinkronisasi dimatikan");
  if ($("#sy-run")) {
    $("#sy-run").onclick = async () => {
      const b = $("#sy-run");
      b.disabled = true;
      msg("#sy-msg", "Menyinkronkan...");
      try {
        const r = await api("/api/sync/run", { method: "POST" });
        const from = r.pulled_from.length ? `Data dari ${r.pulled_from.join(", ")}: ${r.added} baru, ${r.updated} diperbarui. ` : "";
        toast(from + (r.pushed ? "Data komputer ini diunggah." : "Tidak ada perubahan."));
        await showDataSync();
      } catch (e) { msg("#sy-msg", errLine(e.message)); b.disabled = false; }
    };
  }
}

// ---------------------------------------------------------------- pengaturan
// Tab: Umum, Model AI, Pemrosesan, Tentang & update. Nilai model/pemrosesan
// disimpan ke memory.toml dan berlaku setelah aplikasi dibuka ulang.
const SETTINGS_TABS = [["umum", "Umum"], ["model", "Model AI"], ["proses", "Pemrosesan"], ["tentang", "Tentang & update"]];
const settings = { tab: "umum", about: null, data: null, update: null, models: [] };

// [bagian, kunci, label, jenis, petunjuk, pilihan]
const F_LLM = [
  ["llm", "base_url", "Alamat server LLM", "text", "Endpoint OpenAI-compatible, mis. http://localhost:5211/v1 atau https://api.openai.com/v1"],
  ["llm", "extract_model", "Model utama", "model", "Ekstraksi cerita, distilasi, dan jawaban."],
  ["llm", "light_model", "Model ringan", "model", "Segmentasi dan klasifikasi; boleh sama dengan model utama."],
  ["llm", "light_base_url", "Alamat server model ringan", "text", "Kosongkan bila sama dengan server LLM."],
  ["llm", "api_key", "API key", "secret", "Kosongkan untuk server lokal tanpa kunci."],
];
const F_LLM_ADV = [
  ["llm", "api_key_env", "Env var API key", "text", "Dipakai lebih dulu bila terisi, mis. OPENAI_API_KEY."],
  ["llm", "json_mode", "Mode JSON", "select", "json_schema untuk vLLM; json_object untuk OpenAI/Ollama; none bila tidak didukung.", [["json_schema", "json_schema (vLLM)"], ["json_object", "json_object"], ["none", "none"]]],
  ["llm", "disable_thinking", "Matikan mode berpikir (Qwen3)", "bool"],
  ["llm", "temperature", "Temperature", "float"],
  ["llm", "max_tokens", "Maks. token jawaban", "int"],
  ["llm", "timeout_secs", "Batas waktu (detik)", "int"],
  ["llm", "max_retries", "Coba ulang bila gagal", "int"],
];
const F_EMB = [
  ["embedding", "provider", "Penyedia", "select", "", [["openai", "Server embedding (OpenAI-compatible)"], ["hash", "Hash - tanpa server (uji saja)"]]],
  ["embedding", "base_url", "Alamat server embedding", "text", "mis. http://localhost:8081/v1 (TEI)"],
  ["embedding", "model", "Model embedding", "text"],
  ["embedding", "dimension", "Dimensi vektor", "int", "Ganti model/dimensi: jalankan `engram reembed` setelah dibuka ulang."],
  ["embedding", "api_key", "API key", "secret"],
];
const F_EMB_ADV = [
  ["embedding", "api_key_env", "Env var API key", "text"],
  ["embedding", "batch_size", "Ukuran batch", "int"],
];
const F_TR = [
  ["transcription", "base_url", "Alamat server Whisper", "text", "Untuk import audio. Kosongkan bila tidak dipakai."],
  ["transcription", "model", "Model", "text"],
  ["transcription", "language", "Bahasa", "text", "Kode bahasa, mis. id. Kosongkan untuk deteksi otomatis."],
];
const F_PROC = [
  ["pipeline", "workers", "Pekerja paralel", "int", "Jumlah job yang diproses bersamaan. Naikkan bila server LLM kuat."],
  ["schedule", "inbox_poll_secs", "Cek folder inbox tiap (detik)", "int"],
  ["schedule", "retry_failed_minutes", "Proses ulang sumber gagal tiap (menit)", "int", "0 = mati. Berguna bila server LLM kadang mati."],
  ["schedule", "retry_failed_max", "Batas proses ulang otomatis per sumber", "int"],
  ["score", "stream_ttl_days", "Simpan obrolan ringan selama (hari)", "int", "Working memory luruh setelah ini; berkas sumber asli tetap ada."],
  ["retrieval", "top_k", "Jumlah cerita per pencarian", "int", "Dipakai pencarian dan agent AI (MCP)."],
  ["server", "max_upload_mb", "Batas ukuran upload (MB)", "int"],
];

function getTheme() { try { return localStorage.getItem("theme") || "system"; } catch (e) { return "system"; } }
function setTheme(t) {
  try { if (t === "system") localStorage.removeItem("theme"); else localStorage.setItem("theme", t); } catch (e) {}
  if (t === "system") delete document.documentElement.dataset.theme; else document.documentElement.dataset.theme = t;
}

async function openTarget(target, url) {
  if (settings.about?.can_open) {
    try { await api("/api/open", { method: "POST", json: { target } }); } catch (e) { toast(e.message); }
  } else if (url) {
    window.open(url, "_blank", "noopener");
  }
}

async function showSettings(tab) {
  if (tab) settings.tab = tab;
  openDrawer("Pengaturan", `<div class="empty">Memuat...</div>`);
  try { [settings.about, settings.data] = await Promise.all([api("/api/about"), api("/api/settings")]); }
  catch (e) { $("#drawer-body").innerHTML = `<div class="card">Gagal memuat: ${esc(e.message)}</div>`; return; }
  renderSettings();
}

function renderSettings() {
  const d = settings.data;
  $("#drawer-body").innerHTML = `
    <div class="tabs" role="tablist">${SETTINGS_TABS.map(([k, l]) => `<button type="button" role="tab" data-tab="${k}" class="${k === settings.tab ? "active" : ""}" aria-selected="${k === settings.tab}">${esc(l)}</button>`).join("")}</div>
    ${d.restart_required ? `<div class="notice small">${icon("alert")} Pengaturan baru tersimpan, tetapi belum dipakai.
      ${settings.about.can_restart ? `<div class="row" style="margin-top:6px"><button class="primary" id="st-restart">${icon("refresh")} Buka ulang sekarang</button></div>` : " Jalankan ulang <code>engram serve</code> agar berlaku."}</div>` : ""}
    ${!d.editable ? `<div class="notice small">Lokasi <code>memory.toml</code> tidak diketahui; pengaturan hanya bisa dilihat.</div>` : ""}
    <div id="st-body"></div>`;
  $("#drawer-body").querySelectorAll("[data-tab]").forEach((b) => { b.onclick = () => { settings.tab = b.dataset.tab; renderSettings(); }; });
  if ($("#st-restart")) $("#st-restart").onclick = restartApp;
  ({ umum: tabGeneral, model: tabModels, proses: tabProcessing, tentang: tabAbout })[settings.tab]();
}

async function restartApp() {
  try {
    const r = await api("/api/restart", { method: "POST" });
    $("#drawer-body").innerHTML = `<div class="empty">${esc(r.message)}</div>`;
  } catch (e) { toast(e.message); }
}

// Satu input pengaturan. Nilai awal dari settings.data.values.
// f32 dari server (0.1 -> 0.10000000149) dibulatkan untuk ditampilkan & dibandingkan.
const tidy = (v) => (typeof v === "number" ? Number(v.toPrecision(6)) : v);
function fieldHtml([sec, key, label, type, hint, opts]) {
  const v = tidy(settings.data.values[sec]?.[key]);
  const id = `f-${sec}-${key}`;
  const dis = settings.data.editable ? "" : "disabled";
  const attrs = `id="${id}" data-sec="${sec}" data-key="${key}" data-type="${type}" ${dis}`;
  const help = hint ? `<div class="hint">${esc(hint)}</div>` : "";
  if (type === "bool") return `<label class="field check"><input type="checkbox" ${attrs} ${v ? "checked" : ""}><span>${esc(label)}</span></label>`;
  let input;
  if (type === "select") input = `<select ${attrs}>${opts.map(([o, l]) => `<option value="${esc(o)}" ${o === v ? "selected" : ""}>${esc(l)}</option>`).join("")}</select>`;
  else if (type === "secret") {
    const set = settings.data.values[sec]?.[key + "_set"];
    input = `<div class="row"><input type="password" ${attrs} autocomplete="new-password" style="flex:1" placeholder="${set ? "tersimpan - kosongkan bila tidak diubah" : "belum diisi"}">
      ${set && !dis ? `<button type="button" class="ghost small" data-clear="${id}">${icon("x")} Hapus</button>` : ""}</div>`;
  } else if (type === "int" || type === "float") input = `<input type="number" ${attrs} step="${type === "int" ? 1 : 0.05}" value="${esc(v ?? "")}">`;
  else input = `<input type="text" ${attrs} value="${esc(v ?? "")}" ${type === "model" ? `list="st-models"` : ""}>`;
  return `<label class="field" for="${id}"><span>${esc(label)}</span>${input}${help}</label>`;
}

// Kumpulkan input yang berubah: {bagian: {kunci: nilai}}.
function collectChanges(root) {
  const out = {};
  root.querySelectorAll("[data-sec]").forEach((el) => {
    const { sec, key, type } = el.dataset;
    const old = tidy(settings.data.values[sec]?.[key]);
    let v;
    if (type === "bool") v = el.checked;
    else if (type === "secret") {
      if (el.dataset.cleared === "1") v = "";
      else if (el.value) v = el.value;
      else return;
    } else if (type === "int") { if (el.value === "") return; v = parseInt(el.value, 10); }
    else if (type === "float") { if (el.value === "") return; v = parseFloat(el.value); }
    else v = el.value.trim();
    if (type !== "secret" && (v === old || (v === "" && (old === null || old === undefined)))) return;
    (out[sec] = out[sec] || {})[key] = v;
  });
  return out;
}

function bindForm(form) {
  form.querySelectorAll("[data-clear]").forEach((b) => {
    b.onclick = () => {
      const el = $("#" + b.dataset.clear);
      el.dataset.cleared = "1";
      el.value = "";
      el.placeholder = "akan dihapus saat disimpan";
      b.remove();
    };
  });
  form.onsubmit = async (e) => {
    e.preventDefault();
    const changes = collectChanges(form);
    if (!Object.keys(changes).length) { toast("Tidak ada perubahan"); return; }
    const btn = form.querySelector("[type=submit]");
    btn.disabled = true;
    try {
      settings.data = await api("/api/settings", { method: "PUT", json: { values: changes } });
      toast("Pengaturan disimpan");
      renderSettings();
    } catch (err) {
      form.querySelector(".msg").innerHTML = errLine(err.message);
      btn.disabled = false;
    }
  };
}

function tabGeneral() {
  const a = settings.about, theme = getTheme();
  const subj = settings.data.values.vault.default_subject;
  const known = state.subjects.some((s) => s.id === subj);
  $("#st-body").innerHTML = `
    <h2 style="margin-top:0">Tampilan</h2>
    <div class="card">
      <label class="field"><span>Tema</span><select id="st-theme">
        ${[["system", "Ikuti sistem"], ["light", "Terang"], ["dark", "Gelap"]].map(([k, l]) => `<option value="${k}" ${k === theme ? "selected" : ""}>${l}</option>`).join("")}
      </select></label>
    </div>
    <h2>Subjek</h2>
    <form id="st-general" class="card">
      <label class="field"><span>Subjek default</span>
        <select data-sec="vault" data-key="default_subject" data-type="text" ${settings.data.editable ? "" : "disabled"}>
          ${known ? "" : `<option value="${esc(subj)}" selected>${esc(subj)}</option>`}
          ${state.subjects.map((s) => `<option value="${esc(s.id)}" ${s.id === subj ? "selected" : ""}>${esc(s.name)}</option>`).join("")}
        </select>
        <div class="hint">Dipakai pemantau inbox, CLI, dan agent AI bila subjek tidak disebut.</div></label>
      <p class="small msg"></p>
      ${settings.data.editable ? `<button class="primary" type="submit">${icon("check")} Simpan</button>` : ""}
    </form>
    <h2>Pintasan</h2>
    <div class="card row">
      <button type="button" id="st-sync">${icon("cloud")} Data & Sync</button>
      <button type="button" id="st-agents">${icon("plug")} Agent AI</button>
      ${svc.list.length ? `<button type="button" id="st-svc">${icon("play")} Status layanan</button>` : ""}
    </div>
    ${a.can_open ? `<h2>Folder</h2>
    <div class="card row">
      <button type="button" data-open="vault">${icon("folder")} Folder data</button>
      <button type="button" data-open="inbox">${icon("inbox")} Folder inbox</button>
      ${a.config_path ? `<button type="button" data-open="config">${icon("folder")} Folder konfigurasi</button>` : ""}
    </div>` : ""}`;
  $("#st-theme").onchange = (e) => setTheme(e.target.value);
  bindForm($("#st-general"));
  $("#st-sync").onclick = () => showDataSync();
  $("#st-agents").onclick = () => connectAgents();
  if ($("#st-svc")) $("#st-svc").onclick = () => showServices();
  $("#st-body").querySelectorAll("[data-open]").forEach((b) => { b.onclick = () => openTarget(b.dataset.open); });
}

function tabModels() {
  const ed = settings.data.editable;
  const testBtn = (kind, label) => `<div class="row"><button type="button" data-test="${kind}">${icon("refresh")} ${label}</button></div><div class="small" data-test-msg="${kind}"></div>`;
  $("#st-body").innerHTML = `
    <form id="st-models-form">
      <datalist id="st-models">${settings.models.map((m) => `<option value="${esc(m)}">`).join("")}</datalist>
      <h2 style="margin-top:0">Model bahasa (LLM)</h2>
      <div class="card">
        ${F_LLM.map(fieldHtml).join("")}
        <details><summary>Lanjutan</summary><div class="cols">${F_LLM_ADV.map(fieldHtml).join("")}</div></details>
        ${testBtn("llm", "Tes koneksi LLM")}
      </div>
      <h2>Embedding (pencarian makna)</h2>
      <div class="card">
        ${F_EMB.map(fieldHtml).join("")}
        <details><summary>Lanjutan</summary><div class="cols">${F_EMB_ADV.map(fieldHtml).join("")}</div></details>
        ${testBtn("embedding", "Tes koneksi embedding")}
      </div>
      <h2>Transkripsi audio</h2>
      <div class="card">
        ${F_TR.map(fieldHtml).join("")}
        ${testBtn("transcription", "Tes koneksi transkripsi")}
      </div>
      <p class="small msg"></p>
      ${ed ? `<div class="row"><button class="primary" type="submit">${icon("check")} Simpan</button><span class="small muted">Berlaku setelah aplikasi dibuka ulang.</span></div>` : ""}
    </form>`;
  const form = $("#st-models-form");
  bindForm(form);
  form.querySelectorAll("[data-test]").forEach((b) => {
    b.onclick = async () => {
      const kind = b.dataset.test, out = form.querySelector(`[data-test-msg="${kind}"]`);
      b.disabled = true;
      out.textContent = "Menguji...";
      try {
        // Uji nilai di formulir (belum disimpan).
        const r = await api("/api/settings/test", { method: "POST", json: { kind, values: collectChanges(form) } });
        out.innerHTML = `<span class="${r.ok ? "ok-text" : "bad-text"}">${icon(r.ok ? "check" : "alert")} ${esc(r.message)}</span>`;
        if (r.models?.length) {
          settings.models = r.models;
          $("#st-models").innerHTML = r.models.map((m) => `<option value="${esc(m)}">`).join("");
          out.innerHTML += `<div class="hint">Model di server: ${r.models.map(esc).join(", ")}</div>`;
        }
      } catch (e) { out.innerHTML = errLine(e.message); }
      b.disabled = false;
    };
  });
}

function tabProcessing() {
  $("#st-body").innerHTML = `
    <form id="st-proc-form" class="card">
      <div class="cols">${F_PROC.map(fieldHtml).join("")}</div>
      <p class="small msg"></p>
      ${settings.data.editable ? `<div class="row"><button class="primary" type="submit">${icon("check")} Simpan</button><span class="small muted">Berlaku setelah aplikasi dibuka ulang.</span></div>` : ""}
    </form>`;
  bindForm($("#st-proc-form"));
}

function tabAbout() {
  const a = settings.about;
  const host = a.host === "desktop" ? "Aplikasi desktop" : "Server (engram serve)";
  $("#st-body").innerHTML = `
    <div class="card">
      <h3 style="font-size:20px;margin:0">engram <span class="badge">v${esc(a.version)}</span></h3>
      <p class="muted small" style="margin:4px 0 10px">Arsip memori pribadi: menyimpan cerita, fakta, dan cara berpikir seseorang secara lokal, lengkap dengan server MCP untuk agent AI.</p>
      <dl class="kv">
        <dt>Versi</dt><dd>${esc(a.version)}</dd>
        <dt>Mode</dt><dd>${esc(host)}</dd>
        <dt>Sistem</dt><dd>${esc(a.os)} / ${esc(a.arch)}</dd>
        <dt>Lisensi</dt><dd>${esc(a.license)}</dd>
        <dt>Lokasi data</dt><dd><code>${esc(a.vault)}</code></dd>
        ${a.config_path ? `<dt>Konfigurasi</dt><dd><code>${esc(a.config_path)}</code></dd>` : ""}
      </dl>
      <div class="row" style="margin-top:12px">
        <button type="button" data-open="repo">${icon("external")} Halaman proyek</button>
        <button type="button" data-open="releases">${icon("external")} Catatan rilis</button>
        <button type="button" data-open="issues">${icon("flag")} Laporkan masalah</button>
      </div>
    </div>
    <h2>Pembaruan</h2>
    <div class="card">
      <div class="row" style="justify-content:space-between">
        <div>Versi terpasang <b>${esc(a.version)}</b></div>
        <button type="button" id="st-check">${icon("refresh")} Periksa update</button>
      </div>
      <div id="st-update" class="small" style="margin-top:8px"></div>
      ${a.auto_update !== null && a.auto_update !== undefined ? `<label class="field check" style="margin-top:10px"><input type="checkbox" id="st-auto" ${a.auto_update ? "checked" : ""}><span>Periksa update otomatis</span></label>` : ""}
    </div>`;
  const urls = { repo: a.repository, releases: a.repository + "/releases", issues: a.repository + "/issues" };
  $("#st-body").querySelectorAll("[data-open]").forEach((b) => { b.onclick = () => openTarget(b.dataset.open, urls[b.dataset.open]); });
  $("#st-check").onclick = checkUpdate;
  if ($("#st-auto")) {
    $("#st-auto").onchange = async (e) => {
      try {
        await api("/api/update/auto", { method: "POST", json: { enabled: e.target.checked } });
        settings.about.auto_update = e.target.checked;
        toast(e.target.checked ? "Update otomatis aktif" : "Update otomatis dimatikan");
      } catch (err) { toast(err.message); e.target.checked = !e.target.checked; }
    };
  }
  if (settings.update) renderUpdate();
  else checkUpdate();
}

async function checkUpdate() {
  const b = $("#st-check");
  b.disabled = true;
  $("#st-update").textContent = "Memeriksa...";
  try {
    settings.update = await api("/api/update/check", { method: "POST" });
    renderUpdate();
  } catch (e) { $("#st-update").innerHTML = errLine(e.message); }
  b.disabled = false;
}

function renderUpdate() {
  const u = settings.update, el = $("#st-update");
  if (!el) return;
  $("#update-dot").hidden = !u.available;
  if (!u.available) { el.innerHTML = `<span class="ok-text">${icon("check")} Sudah versi terbaru${u.latest ? ` (${esc(u.latest)})` : ""}.</span>`; return; }
  el.innerHTML = `
    <div class="notice">
      <b>Versi ${esc(u.latest)} tersedia</b>${u.date ? ` <span class="muted">&middot; ${esc(new Date(u.date).toLocaleDateString("id-ID"))}</span>` : ""}
      ${u.notes ? `<pre class="src" style="max-height:200px;margin:8px 0">${esc(u.notes)}</pre>` : ""}
      <div class="row" style="margin-top:6px">
        ${u.can_install ? `<button class="primary" id="st-install">${icon("download")} Pasang & buka ulang</button>` : `<button type="button" id="st-dl">${icon("external")} Unduh di GitHub</button>`}
      </div>
      <div class="small" id="st-install-msg"></div>
    </div>`;
  if ($("#st-dl")) $("#st-dl").onclick = () => openTarget("releases", u.url);
  if ($("#st-install")) {
    $("#st-install").onclick = async () => {
      $("#st-install").disabled = true;
      try {
        const r = await api("/api/update/install", { method: "POST" });
        $("#st-install-msg").textContent = r.message;
      } catch (e) { $("#st-install-msg").innerHTML = errLine(e.message); $("#st-install").disabled = false; }
    };
  }
}

// Formulir di dalam halaman; jangan pakai prompt()/confirm(): WebView aplikasi
// desktop tidak menampilkan dialog browser.
function newSubjectForm() {
  openDrawer("Subjek baru", `
    <p class="muted small">Subjek adalah orang yang memorinya disimpan.</p>
    <form id="subject-form" class="card">
      <p><label>Nama<br><input type="text" id="sf-name" required placeholder="mis. Bapak Sukarno" style="width:100%"></label></p>
      <p><label>ID singkat<br><input type="text" id="sf-id" required pattern="[a-z0-9][a-z0-9_-]*" placeholder="huruf kecil, angka, tanda minus" style="width:100%"></label></p>
      <p><label>Tanggal lahir (opsional)<br><input type="date" id="sf-birth"></label></p>
      <p class="small muted">Tanggal lahir dipakai untuk menerjemahkan "umur 20" atau "waktu SD" menjadi tahun.</p>
      <p id="sf-err" class="small" style="color:var(--bad)"></p>
      <button class="primary" type="submit">${icon("check")} Simpan</button>
    </form>`);
  const slug = (v) => v.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
  let touched = false;
  $("#sf-id").oninput = () => { touched = true; };
  $("#sf-name").oninput = () => { if (!touched) $("#sf-id").value = slug($("#sf-name").value); };
  $("#sf-name").focus();
  $("#subject-form").onsubmit = async (e) => {
    e.preventDefault();
    const id = $("#sf-id").value.trim(), name = $("#sf-name").value.trim();
    try {
      await api("/api/subjects", { method: "POST", json: { id, name, birth: $("#sf-birth").value || null } });
      state.subject = id;
      localStorage.setItem("subject", id);
      $("#drawer").hidden = true;
      await loadSubjects();
      route();
      toast("Subjek dibuat");
    } catch (err) { $("#sf-err").textContent = err.message; }
  };
}

function askToken() {
  if (!$("#drawer").hidden && $("#token-form")) return;
  openDrawer("Token akses", `
    <form id="token-form" class="card">
      <p class="small muted">Server ini dilindungi token (env MEMORY_TOKEN).</p>
      <p><input type="password" id="tk" required style="width:100%" placeholder="token"></p>
      <button class="primary" type="submit">${icon("check")} Simpan</button>
    </form>`);
  $("#token-form").onsubmit = (e) => {
    e.preventDefault();
    localStorage.setItem("token", $("#tk").value.trim());
    $("#drawer").hidden = true;
    loadSubjects().then(route);
  };
}

window.addEventListener("hashchange", route);
hydrateIcons();
pollServices();
setInterval(pollServices, 5000);
loadSubjects().then(route).catch((e) => { $("#view").innerHTML = `<div class="card">Tidak bisa menghubungi server: ${esc(e.message)}</div>`; });
