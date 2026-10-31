// UI web lokal mnemonic-memory. Tanpa framework/build step; data selalu di-escape.
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
  refresh: '<path d="M20 11a8 8 0 0 0-14.5-4.5L4 8M4 4v4h4"/><path d="M4 13a8 8 0 0 0 14.5 4.5L20 16M20 20v-4h-4"/>',
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
    <h3>Episode dari sumber ini</h3>
    ${r.episodes.map((ep) => `<div class="tl-item" data-ep="${esc(ep.id)}"><span class="tl-time">${esc(labelTime(ep))}</span><span>${esc(ep.title)}</span></div>`).join("") || '<p class="muted">Belum ada.</p>'}
    <h3>Teks asli <span class="muted small">(sorotan = kutipan yang dipakai sebagai bukti)</span></h3><pre class="src">${text}</pre>`);
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
    </div>
    <h2>Data subjek</h2>
    <div class="card"><div class="row">
      <label>Tanggal lahir <input type="date" id="birth" value="${esc(r.subject?.anchors?.birth || "")}"></label>
      <button class="primary" id="save-birth">Simpan</button>
      <span class="muted small">Dipakai untuk menerjemahkan "umur 20" dan "waktu SD" menjadi tahun.</span>
    </div></div>`;
  el.querySelectorAll("[data-m]").forEach((b) => { b.onclick = async () => { await api(`/api/subjects/${S()}/maintenance/${b.dataset.m}`, { method: "POST" }); toast("Masuk antrean"); }; });
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
    $("#drawer-body").innerHTML = `<div class="card"><p>Pemasangan otomatis hanya tersedia di aplikasi desktop atau <code>memory serve</code> di 127.0.0.1.</p>
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
loadSubjects().then(route).catch((e) => { $("#view").innerHTML = `<div class="card">Tidak bisa menghubungi server: ${esc(e.message)}</div>`; });
