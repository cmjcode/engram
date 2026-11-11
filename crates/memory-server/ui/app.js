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
  code: '<path d="m8 8-4 4 4 4M16 8l4 4-4 4M14 5l-4 14"/>',
  menu: '<path d="M4 7h16M4 12h16M4 17h16"/>',
  cpu: '<rect x="6" y="6" width="12" height="12" rx="2"/><path d="M10 10h4v4h-4zM9 3v3M15 3v3M9 18v3M15 18v3M3 9h3M3 15h3M18 9h3M18 15h3"/>',
  sliders: '<path d="M4 7h10M18 7h2M4 17h4M12 17h8"/><circle cx="16" cy="7" r="2"/><circle cx="10" cy="17" r="2"/>',
  database: '<ellipse cx="12" cy="6" rx="7" ry="3"/><path d="M5 6v12c0 1.7 3.1 3 7 3s7-1.3 7-3V6M5 12c0 1.7 3.1 3 7 3s7-1.3 7-3"/>',
  server: '<rect x="4" y="4" width="16" height="7" rx="2"/><rect x="4" y="13" width="16" height="7" rx="2"/><path d="M8 7.5h.01M8 16.5h.01"/>',
  info: '<circle cx="12" cy="12" r="9"/><path d="M12 11v5M12 8v.01"/>',
  git: '<circle cx="6" cy="6" r="2"/><circle cx="6" cy="18" r="2"/><circle cx="18" cy="8" r="2"/><path d="M6 8v8M18 10a6 6 0 0 1-6 6H8"/>',
  tag: '<path d="M3 12V4h8l9 9-8 8z"/><circle cx="7.5" cy="8.5" r="1.5"/>',
  link: '<path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1"/><path d="M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1"/>',
  edit: '<path d="M4 20h4L19 9l-4-4L4 16z"/><path d="m13.5 6.5 4 4"/>',
  graph: '<circle cx="6" cy="6" r="2.5"/><circle cx="18" cy="8" r="2.5"/><circle cx="9" cy="18" r="2.5"/><path d="M8.3 7.1 15.6 7.6M7 8.4l1.4 7.2M16.4 9.9l-5.6 6.4"/>',
  minus: '<path d="M5 12h14"/>',
  fit: '<path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"/>',
  orbit: '<circle cx="12" cy="12" r="3.5"/><ellipse cx="12" cy="12" rx="10" ry="4.5" transform="rotate(-25 12 12)"/>',
};
const icon = (n) => `<svg class="i" viewBox="0 0 24 24" aria-hidden="true">${ICONS[n] || ""}</svg>`;
const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const $ = (sel, el = document) => el.querySelector(sel);

const state = { subject: localStorage.getItem("subject") || "", subjects: [], mode: localStorage.getItem("mode") || "kerja", project: localStorage.getItem("project") || "", projects: [] };
const P = () => state.project;
const pq = (prefix = "?") => (state.project ? `${prefix}project=${encodeURIComponent(state.project)}` : "");
const OBS = { decision: t("decision"), bugfix: t("bugfix"), discovery: t("discovery"), feature: t("feature"), refactor: t("refactor"), change: t("change"), session_summary: t("session"), lesson: t("lesson") };
const typeBadge = (ty) => (ty ? `<span class="badge t-${esc(ty)}">${esc(OBS[ty] || ty)}</span>` : `<span class="badge t-story">${t("story")}</span>`);
const when = (s) => (s || "").slice(0, 16).replace("T", " ");
const ago = (iso) => {
  if (!iso) return "";
  const d = (Date.now() - new Date(iso).getTime()) / 60000;
  if (d < 1) return t("just now");
  if (d < 60) return t("{n} min ago", { n: Math.floor(d) });
  if (d < 1440) return t("{n} h ago", { n: Math.floor(d / 60) });
  return t("{n} d ago", { n: Math.floor(d / 1440) });
};
async function copyText(text, msg = t("Copied")) {
  try { await navigator.clipboard.writeText(text); toast(msg); return; } catch (e) { /* WebView tanpa izin clipboard */ }
  const ta = document.createElement("textarea");
  ta.value = text; document.body.appendChild(ta); ta.select();
  try { document.execCommand("copy"); toast(msg); } catch (e) { toast(t("Could not copy; select the text manually")); }
  ta.remove();
}
let routeSeq = 0;

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
    throw new Error(t("access token required"));
  }
  const body = res.headers.get("content-type")?.includes("json") ? await res.json() : await res.text();
  if (!res.ok) throw new Error(body?.error || res.statusText);
  return body;
}
const S = () => encodeURIComponent(state.subject);

// Mode "kerja" menonjolkan proyek, sesi, dan review; mode "personal" menambah
// wawancara dan silsilah. Statistik & perawatan (konsolidasi, padatkan, distilasi,
// proses ulang) berlaku untuk kedua mode karena memori proyek pun perlu dirawat.
const ROUTES = [
  // Global: berlaku untuk memori kerja maupun hidup.
  ["cari", "search", t("Search & ask"), viewSearch, "semua"],
  ["review", "flag", t("Review"), viewReview, "semua"],
  ["orang", "users", t("People & organizations"), viewPeople, "semua"],
  ["timeline", "clock", t("Timeline"), viewTimeline, "semua"],
  ["berpikir", "compass", t("Ways of thinking"), viewThinking, "semua"],
  ["sumber", "inbox", t("Sources & import"), viewSources, "semua"],
  ["ringkasan", "book", t("Stats & maintenance"), viewHome, "semua"],
  // Kerja: proyek koding dan sesi agent.
  ["proyek", "home", t("Projects"), viewProjects, "kerja"],
  ["graf", "graph", t("Project graph"), viewGraph, "kerja"],
  ["sesi", "code", t("Sessions & lessons"), viewSessions, "kerja"],
  // Personal: biografi dan keluarga.
  ["wawancara", "mic", t("Interview"), viewInterview, "personal"],
];
// Urutan dan judul kelompok di sidebar. Mode personal juga menampilkan kelompok kerja.
const NAV_GROUPS = [["semua", t("Global")], ["kerja", t("Work")], ["personal", t("Personal")]];
const DEFAULT_ROUTE = () => (state.mode === "personal" ? "ringkasan" : "proyek");

function renderNav() {
  const cur = (location.hash.slice(1).split("/")[0]) || DEFAULT_ROUTE();
  const visible = (tag) => tag === "semua" || tag === state.mode || (state.mode === "personal" && tag === "kerja");
  const link = ([k, ic, label]) => `<a href="#${k}" class="${k === cur ? "active" : ""}" ${k === cur ? 'aria-current="page"' : ""}>${icon(ic)}<span>${label}</span></a>`;
  $("#nav").innerHTML = NAV_GROUPS.filter(([tag]) => visible(tag)).map(([tag, title]) => {
    const items = ROUTES.filter(([, , , , t]) => t === tag);
    return items.length ? `<div class="nav-group" role="group" aria-label="${title}"><div class="nav-title">${title}</div>${items.map(link).join("")}</div>` : "";
  }).join("");
  $("#mode-slot").innerHTML = `<span class="mode" role="group" aria-label="${t("Mode")}"><button class="${state.mode === "kerja" ? "active" : ""}" data-mode="kerja">${t("Work")}</button><button class="${state.mode === "personal" ? "active" : ""}" data-mode="personal">${t("Personal")}</button></span>`;
  $("#mode-slot").querySelectorAll("[data-mode]").forEach((b) => { b.onclick = () => { state.mode = b.dataset.mode; localStorage.setItem("mode", state.mode); if (!location.hash || location.hash === "#proyek" || location.hash === "#ringkasan") location.hash = "#" + DEFAULT_ROUTE(); route(); }; });
  $("#nav").querySelectorAll("a").forEach((a) => { a.addEventListener("click", () => document.body.classList.remove("nav-open")); });
}

async function loadProjects() {
  try {
    const r = await api(`/api/subjects/${S()}/projects`);
    state.projects = r.projects || [];
  } catch (e) { state.projects = []; }
  const sel = $("#project-select");
  if (!sel) return;
  const names = state.projects.map((p) => p.project);
  if (state.project && !names.includes(state.project)) names.unshift(state.project);
  sel.hidden = names.length === 0;
  sel.innerHTML = `<option value="">${t("All projects")}</option>` + names.map((n) => `<option value="${esc(n)}" ${n === state.project ? "selected" : ""}>${esc(n)}</option>`).join("");
}

// Palet cepat (Cmd/Ctrl+K): cari memori atau lompat ke tampilan.
function openPalette() {
  if ($("#palette")) { $("#palette").remove(); return; }
  const box = document.createElement("div");
  box.id = "palette";
  box.innerHTML = `<div class="pal-card"><input type="search" id="pal-q" placeholder="${t("Search memory, or type a view name...")}" autocomplete="off"><div id="pal-list"></div><div class="small muted">${t("Enter = search")} &nbsp;·&nbsp; ${t("Esc = close")}</div></div>`;
  document.body.appendChild(box);
  const q = $("#pal-q"), list = $("#pal-list");
  const draw = () => {
    const v = q.value.trim().toLowerCase();
    const routes = ROUTES.filter(([k, , label]) => !v || label.toLowerCase().includes(v) || k.includes(v)).slice(0, 5);
    const projs = state.projects.filter((p) => v && p.project.toLowerCase().includes(v)).slice(0, 5);
    list.innerHTML = projs.map((p) => `<a href="#proyek/${encodeURIComponent(p.project)}" class="pal-item">${icon("home")} ${t("Project {name}", { name: esc(p.project) })}</a>`).join("")
      + routes.map(([k, ic, label]) => `<a href="#${k}" class="pal-item">${icon(ic)} ${esc(label)}</a>`).join("")
      + (v ? `<a href="#cari/${encodeURIComponent(q.value.trim())}" class="pal-item">${icon("search")} ${t("Search \"{q}\"", { q: esc(q.value.trim()) })}</a>` : "");
    list.querySelectorAll("a").forEach((a) => { a.onclick = () => box.remove(); });
  };
  q.oninput = draw;
  q.onkeydown = (e) => {
    if (e.key === "Enter") { e.preventDefault(); const first = list.querySelector("a"); if (q.value.trim()) location.hash = `#cari/${encodeURIComponent(q.value.trim())}`; else if (first) first.click(); box.remove(); }
    if (e.key === "Escape") box.remove();
  };
  box.onclick = (e) => { if (e.target === box) box.remove(); };
  draw();
  q.focus();
}
document.addEventListener("keydown", (e) => { if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") { e.preventDefault(); openPalette(); } });

function hydrateIcons(root = document) {
  root.querySelectorAll("[data-icon]").forEach((el) => { el.innerHTML = icon(el.dataset.icon); });
}

async function route() {
  const seq = ++routeSeq;
  renderNav();
  const [name, arg] = location.hash.slice(1).split("/");
  const r = ROUTES.find((x) => x[0] === (name || DEFAULT_ROUTE())) || ROUTES[0];
  const view = $("#view");
  if (!state.subject) {
    view.innerHTML = `<div class="empty"><h1>${t("Welcome")}</h1><p>${t("Start by adding the person whose memories you want to keep.")}</p>
      <button class="primary" id="first-subject">${icon("plus")} ${t("Add subject")}</button></div>`;
    $("#first-subject").onclick = newSubjectForm;
    return;
  }
  // `stage` dipasang utuh ke <main> (bukan anak-anaknya saja) agar handler yang menyimpan
  // rujukan `el` tetap menemukan elemennya setelah render, mis. indikator tab aktif.
  const stage = document.createElement("div");
  stage.className = "page";
  view.innerHTML = `<div class="empty">${t("Loading...")}</div>`;
  try {
    if (name === "proyek" && arg) await viewProject(stage, decodeURIComponent(arg));
    else await r[3](stage, arg && decodeURIComponent(arg));
    if (seq !== routeSeq) return;
    view.replaceChildren(stage);
  } catch (e) {
    if (seq !== routeSeq) return;
    view.innerHTML = `<div class="card">${t("Failed to load: {error}", { error: esc(e.message) })}</div>`;
  }
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
function openDrawer(title, html, wide = false) {
  $("#drawer").classList.toggle("drawer--wide", wide);
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
  // Tautan [[proyek]] di dalam baris yang bisa diklik: jangan ikut membuka episode induknya.
  root.querySelectorAll("a.wikilink").forEach((a) => { a.onclick = (e) => { e.stopPropagation(); $("#drawer").hidden = true; }; });
}

function factBadge(f) {
  if (f.status === "conflict") return `<span class="badge conflict">${t("conflicting")}</span>`;
  if (f.current) return `<span class="badge now">${t("current")}</span>`;
  if (f.valid_to) return `<span class="badge past">${t("past")}</span>`;
  return "";
}
const yr = (d) => (d ? d.slice(0, 4) : "?");
function factLine(f) {
  const span = f.valid_to ? `${yr(f.valid_from)}–${yr(f.valid_to)}` : t("since {year}", { year: yr(f.valid_from) });
  const ev = (f.evidence || []).map((e) => `<a class="cite" data-ep="${esc(e.episode_id || e)}">${t("evidence")}</a>`).join(" ");
  const statement = f.statement || `${f.predicate} ${f.object_text}`;
  return `<div class="fact">${factBadge(f)} ${esc(statement)} <span class="muted small">(${esc(span)}; ${esc(f.time?.kind ? "" : f.time || "")})</span> ${ev}</div>`;
}

async function showEpisode(id) {
  const r = await api(`/api/subjects/${S()}/episodes/${encodeURIComponent(id)}`);
  const e = r.episode;
  const ents = r.entities.map((x) => `<a class="cite" data-entity="${esc(x.id)}">${esc(x.name)}</a>`).join(", ");
  const facts = r.facts.map((f) => `<div class="fact">${f.valid_to ? `<span class="badge past">${t("past")}</span>` : `<span class="badge now">${t("current")}</span>`} ${esc(f.predicate)} ${esc(f.object_text)} <span class="muted small">${esc(f.valid_from || "?")} – ${esc(f.valid_to || "")}</span><div class="quote">${esc(f.evidence?.[0]?.quote || "")}</div></div>`).join("");
  const dec = (e.decisions || []).map((d) => `<li>${esc(d.situation)}: <b>${esc(d.action)}</b>${d.reason ? " — " + esc(d.reason) : ""}</li>`).join("");
  const emo = (e.emotions || []).map((m) => `<span class="badge">${esc(m.label)}</span>`).join("");
  const retold = r.retellings.length ? `<p class="small muted">${t("Retold {n}x:", { n: r.retellings.length })} ${r.retellings.map((x) => `<a class="cite" data-ep="${esc(x.id)}">${esc(x.created_at.slice(0, 10))}</a>`).join(", ")}</p>` : "";
  openDrawer(e.title || t("Story"), `
    <p class="muted small">${esc(labelTime(e))} · ${esc(e.life_phase || t("unknown phase"))} · ${t("language:")} ${esc((e.languages || []).join(", "))}</p>
    ${e.retelling_of ? `<p class="small">${t("This is a retelling of the {link}.", { link: `<a class="cite" data-ep="${esc(e.retelling_of)}">${t("original story")}</a>` })}</p>` : ""}
    <p>${esc(e.summary)}</p>${emo}
    ${dec ? `<h3>${t("Decisions")}</h3><ul>${dec}</ul>` : ""}
    ${facts ? `<h3>${t("Facts from this story")}</h3>${facts}` : ""}
    ${ents ? `<p><b>${t("Mentioned:")}</b> ${ents}</p>` : ""}${retold}
    ${(e.links || []).length ? `<p><b>${t("Linked projects:")}</b> ${e.links.map((l) => wikiLink(l)).join(" ")}</p>` : ""}
    <h3>${t("Original text")}</h3><pre class="src">${renderNotes(e.text)}</pre>
    ${r.source ? `<p class="small">${t("Source:")} <a class="cite" data-source="${esc(r.source.id)}">${esc(r.source.title)}</a></p>` : ""}`);
}

function fmtIsoDate(iso) {
  if (!iso || iso.length < 10) return "";
  const [y, m, d] = iso.slice(0, 10).split("-");
  return y && m && d ? `${d}-${m}-${y}` : iso.slice(0, 10);
}

function labelTime(e) {
  const tm = e.time || {};
  switch (tm.kind) {
    case "exact": return fmtIsoDate(tm.date) || tm.date;
    case "approx": return t("around {year}", { year: (tm.around || "").slice(0, 4) });
    case "range": return `${yr(tm.start)}–${tm.ongoing ? t("now") : yr(tm.end)}`;
    case "age": return tm.to && tm.to !== tm.from ? t("age {from}–{to}", { from: tm.from, to: tm.to }) : t("age {age}", { age: tm.from });
    case "relative": return t("{years} yrs after {anchor}", { years: tm.offset_years, anchor: tm.anchor });
    case "life_phase": return t("{phase} period", { phase: tm.phase });
    case "current": return t("now");
    case "past": return t("past");
    default: {
      if (e.time_start) {
        return e.time_end && e.time_end.slice(0, 4) !== e.time_start.slice(0, 4)
          ? `${e.time_start.slice(0, 4)}–${e.time_end.slice(0, 4)}`
          : fmtIsoDate(e.time_start);
      }
      if (e.observed_at) return fmtIsoDate(e.observed_at);
      if (e.created_at) return fmtIsoDate(e.created_at);
      return t("unknown time");
    }
  }
}

async function showEntity(id) {
  const w = await api(`/api/subjects/${S()}/entities/${encodeURIComponent(id)}`);
  const e = w.entity;
  const kin = w.kin.map((k) => `<li>${esc(k.relation)}: <a class="cite" data-entity="${esc(k.person_id)}">${esc(k.name)}</a>${k.inferred ? ` <span class="muted small">(${t("inferred")})</span>` : ""}</li>`).join("");
  openDrawer(e.name, `
    <p>${w.relation_to_subject ? `<span class="badge">${esc(w.relation_to_subject)}</span>` : ""}<span class="badge">${esc(e.kind)}</span><span class="badge ${e.status === "provisional" ? "warn" : ""}">${esc(e.status)}</span></p>
    ${e.aliases.length ? `<p class="small">${t("Aliases:")} ${esc(e.aliases.join(", "))}</p>` : ""}
    ${e.description ? `<p>${esc(e.description)}</p>` : ""}
    ${w.current_facts.length ? `<h3>${t("Current")}</h3>${w.current_facts.map(factLine).join("")}` : ""}
    ${w.history.length ? `<h3>${t("History")}</h3>${w.history.map(factLine).join("")}` : ""}
    ${kin ? `<h3>${t("Kinship")}</h3><ul>${kin}</ul>` : ""}
    ${["person", "org", "project"].includes(e.kind) ? `<div class="row"><button id="briefing-btn">${icon("book")} ${t("Briefing before meeting/working")}</button></div><pre class="src" id="briefing" hidden></pre>` : ""}
    <h3>${t("Stories ({n})", { n: w.episodes.length })}</h3>
    ${w.episodes.map((ep) => `<div class="tl-item" data-ep="${esc(ep.id)}"><span class="tl-time">${esc(ep.time)}</span><span>${esc(ep.title || ep.excerpt)}</span></div>`).join("") || `<p class="muted">${t("Nothing yet.")}</p>`}`);
  const bb = $("#briefing-btn");
  if (bb) bb.onclick = () => {
    // Ringkasan siap pakai: siapa/apa, hubungan, yang berlaku sekarang, riwayat, 8 interaksi terakhir.
    const lines = [`${t("Briefing:")} ${e.name}${w.relation_to_subject ? ` (${w.relation_to_subject})` : ""} - ${new Date().toLocaleDateString(LOCALE)}`];
    if (e.description) lines.push(e.description);
    if (w.current_facts.length) { lines.push("", t("Current:")); w.current_facts.slice(0, 12).forEach((f) => lines.push(`- ${f.statement}`)); }
    if (w.history.length) { lines.push("", t("History:")); w.history.slice(0, 6).forEach((f) => lines.push(`- ${f.statement} (${f.valid_from || "?"} - ${f.valid_to || "?"})`)); }
    if (w.episodes.length) { lines.push("", t("Recent interactions/notes:")); w.episodes.slice(-8).reverse().forEach((ep) => lines.push(`- ${ep.time}: ${ep.title || ep.excerpt}`)); }
    const text = lines.join("\n");
    const pre = $("#briefing"); pre.textContent = text; pre.hidden = false;
    copyText(text, t("Briefing copied"));
  };
}

// Detail sesi: alur giliran (prompt -> tool -> observasi -> jawaban) + handoff.
async function showSession(id) {
  const r = await api(`/api/sessions/${encodeURIComponent(id)}`);
  const s = r.session;
  const byPrompt = {};
  for (const e of r.episodes) (byPrompt[e.prompt_number || 0] ||= []).push(e);
  const summary = [...r.episodes].reverse().find((e) => e.obs_type === "session_summary");
  const turns = r.turns.map((tn) => `<div class="turn"><div class="turn-head">${t("Prompt {n}", { n: tn.prompt_number })}${tn.summarized ? "" : ` <span class="badge warn">${t("not summarized yet")}</span>`}</div>
    ${tn.items.map((it) => it.kind === "prompt" ? `<div class="turn-item prompt"><b>${t("User")}</b><div>${esc(it.text)}</div></div>`
    : it.kind === "assistant" ? `<div class="turn-item assistant"><b>${t("Agent")}</b><div>${esc(it.text.slice(0, 1500))}${it.text.length > 1500 ? "…" : ""}</div></div>`
      : `<div class="turn-item tool ${it.failed ? "failed" : ""}"><b>${esc(it.name.split("__").pop())}</b> <span class="small muted">${esc(it.summary)}</span>${it.response ? `<pre class="src small">${esc(it.response.slice(0, 400))}</pre>` : ""}</div>`).join("")}
    ${(byPrompt[tn.prompt_number] || []).map((e) => `<div class="turn-item born" data-ep="${esc(e.id)}">${typeBadge(e.obs_type)} <b>${esc(e.title)}</b><div class="small muted">${esc(e.summary)}</div></div>`).join("")}
  </div>`).join("");
  const handoff = () => {
    const lines = [t("Session handoff {client} - project {project} ({time})", { client: s.client, project: s.project || "-", time: when(s.last_activity_at) }), t("Goal: {goal}", { goal: s.first_prompt || "-" })];
    if (summary) {
      lines.push("", summary.text.trim());
      lines.push("", t("Reference: ep:{id}", { id: summary.id }));
    }
    const obs = r.episodes.filter((e) => e.obs_type && e.obs_type !== "session_summary");
    if (obs.length) { lines.push("", t("Observations:")); obs.forEach((e) => lines.push(`- [${OBS[e.obs_type] || e.obs_type}] ${e.title} (ep:${e.id})`)); }
    return lines.join("\n");
  };
  openDrawer(t("Session {client}: {project}", { client: s.client, project: s.project || t("no project") }), `
    <p class="small muted">${esc(when(s.started_at))} – ${esc(when(s.last_activity_at))} · ${t("{n} prompts", { n: s.prompt_counter })} · ${t("{n} tools", { n: s.tool_uses })} · ${t("{n} episodes", { n: s.episodes })} · <span class="badge ${s.status === "active" ? "now" : "past"}">${esc(s.status)}</span></p>
    ${s.cwd ? `<p class="small muted">${esc(s.cwd)}${s.worktree ? " · worktree " + esc(s.worktree) : ""}</p>` : ""}
    <div class="row"><button class="primary" id="handoff-btn">${icon("download")} ${t("Copy handoff")}</button>${summary ? `<button data-ep="${esc(summary.id)}">${icon("book")} ${t("Session summary")}</button>` : ""}</div>
    ${summary ? `<div class="card"><pre class="src">${esc(summary.text)}</pre></div>` : ""}
    <h3>${t("Turn flow")}</h3>${turns || `<p class="muted">${t("Turn spool not found (old session or vault moved).")}</p>`}`);
  $("#handoff-btn").onclick = () => copyText(handoff(), t("Handoff copied"));
}

// Panel kesehatan integrasi: apakah sesi benar-benar terekam.
async function healthPanel() {
  let h;
  try { h = await api("/api/hooks/status"); } catch (e) { return `<div class="card"><span class="badge conflict">${t("integration status unavailable")}</span> ${esc(e.message)}</div>`; }
  const inst = h.hooks_installed || {};
  const cl = h.clients || {};
  const names = { "claude-code": "Claude Code", "gemini-cli": "Gemini CLI", antigravity: "Antigravity" };
  const row = (k) => {
    const i = inst[k] || {}, c = cl[k];
    const st = c ? `<span class="badge now">${t("last event {when}", { when: esc(ago(c.last_at)) })}</span> <span class="small muted">${esc(c.last_event)}${c.last_project ? " · " + esc(c.last_project) : ""}${c.spool_pending ? ` · <span class="badge warn">${t("{n} held in spool", { n: c.spool_pending })}</span>` : ""}</span>`
      : i.installed ? `<span class="badge past">${t("no events since server started")}</span>` : i.legacy ? `<span class="badge warn">${t("legacy hook (Python); click \"Reinstall integration\"")}</span>` : `<span class="badge conflict">${t("hook not installed")}</span>`;
    return `<div class="health-row"><b>${names[k]}</b>${st}</div>`;
  };
  const be = h.backends?.ok ? `<span class="badge now">${t("LLM & embedding up")}</span>` : `<span class="badge conflict">${t("LLM/embedding down")}</span> <span class="small muted">${esc(h.backends?.error || "")}</span>`;
  return `<div class="card health"><div class="health-grid">${["claude-code", "gemini-cli", "antigravity"].map(row).join("")}
    <div class="health-row"><b>${t("Server")}</b>${be}</div>
    <div class="health-row"><b>${t("Queue")}</b><span class="small">${t("{n} jobs pending", { n: h.pending_jobs })} · ${t("{n} active sessions", { n: h.sessions_active })} · ${h.inject_enabled ? t("context injection on") : t("context injection off")} · ${h.redact_enabled ? t("redaction on") : t("redaction off")}</span></div></div>
    <div class="row" style="margin-top:10px"><button id="install-hooks-btn" title="${t("Runs scripts/install-mcp.sh --client all")}">${icon("refresh")} ${t("Reinstall integration")}</button>
      <span class="small muted">${t("MCP, Global Rule & hooks for all clients. Restart the agents afterwards.")}</span></div>
    <pre class="src" id="install-hooks-out" hidden></pre></div>`;
}

// Tombol "Pasang ulang integrasi": server menjalankan install-mcp.sh --client all.
function wireInstallHooks(el) {
  const btn = el.querySelector("#install-hooks-btn"), out = el.querySelector("#install-hooks-out");
  if (!btn) return;
  btn.onclick = async () => {
    btn.disabled = true;
    const label = btn.innerHTML;
    btn.innerHTML = `${icon("refresh")} ${t("Installing...")}`;
    out.hidden = false;
    out.textContent = t("Running scripts/install-mcp.sh --client all ...");
    try {
      const r = await api("/api/hooks/install", { method: "POST" });
      out.textContent = r.output || t("(no output)");
      toast(r.ok ? t("Integration installed. Restart Claude Code, Gemini CLI & Antigravity.") : t("Installer failed (code {code})", { code: r.code ?? "?" }));
      if (r.ok) {
        // Muat ulang status integrasi, tetap tampilkan log installer.
        await route();
        const o = $("#install-hooks-out");
        if (o) { o.hidden = false; o.textContent = r.output || ""; }
      }
    } catch (e) {
      out.textContent = e.message;
      toast(t("Installer failed"));
    } finally {
      btn.disabled = false;
      btn.innerHTML = label;
    }
  };
}

// --- Metadata proyek: repositori git, tautan [[...]] bergaya Obsidian, tag grup.
const projHref = (name) => `#proyek/${encodeURIComponent(name)}`;
const projKey = (n) => String(n || "").trim().toLowerCase().replace(/\\/g, "/").replace(/ /g, "");
const sameProj = (a, b) => { const ka = projKey(a), kb = projKey(b); return !!ka && !!kb && (ka === kb || ka.endsWith("/" + kb) || kb.endsWith("/" + ka)); };
const knownProject = (name) => state.projects.some((p) => sameProj(p.project, name));
const tagChip = (tg) => `<a class="tag-chip" data-tag="${esc(tg)}" href="#proyek" title="${t("Show projects tagged #{tag}", { tag: esc(tg) })}">#${esc(tg)}</a>`;
const wikiLink = (target, label) => `<a class="wikilink${knownProject(target) ? "" : " missing"}" href="${projHref(target)}" title="${knownProject(target) ? t("Open project") : t("No memory for this project yet")}">${esc(label || target)}</a>`;
// Teks catatan: [[Nama]], [[Nama|label]], [[Nama#bagian]] jadi tautan proyek. Teks mentah
// dipotong per tautan; teks biasa di-escape sekali, target/label mentah di-escape oleh wikiLink.
function renderNotes(text) {
  const raw = String(text ?? "");
  const re = /\[\[([^\[\]\n]+?)\]\]/g;
  let out = "", last = 0, m;
  while ((m = re.exec(raw))) {
    const [left, label] = m[1].split("|");
    const target = left.split("#")[0].trim();
    out += esc(raw.slice(last, m.index)) + (target ? wikiLink(target, (label || left).trim()) : esc(m[0]));
    last = re.lastIndex;
  }
  return out + esc(raw.slice(last));
}
function bindTagChips(root) {
  root.querySelectorAll("[data-tag]").forEach((a) => {
    a.onclick = (e) => { e.preventDefault(); e.stopPropagation(); localStorage.setItem("proj-tag", a.dataset.tag); if (location.hash === "#proyek") route(); else location.hash = "#proyek"; };
  });
}
async function openRepo(name, url) {
  try { await api("/api/open", { method: "POST", json: { target: "project-repo", subject: state.subject, project: name } }); }
  catch (e) { if (url) window.open(url, "_blank", "noopener"); else toast(e.message); }
}

// Beranda mode kerja: proyek aktif, dikelompokkan per tag bila ada.
async function viewProjects(el) {
  const [r, health] = await Promise.all([api(`/api/subjects/${S()}/projects`), healthPanel()]);
  state.projects = r.projects;
  const projCard = (p) => {
    const types = Object.entries(p.by_type || {}).filter(([k]) => k !== "cerita").sort((a, b) => b[1] - a[1]).slice(0, 5).map(([k, n]) => `${typeBadge(k)}<span class="small">${n}</span>`).join(" ");
    const ls = p.last_session, sm = p.last_summary;
    const links = (p.links || []).length + (p.backlinks || []).length + (p.episode_backlinks || []).length;
    return `<div class="card proj" data-proj="${esc(p.project)}" role="button" tabindex="0">
      <div class="proj-head"><h3>${esc(p.project)}</h3><span class="small muted">${esc(ago(p.last_activity))}</span></div>
      ${p.repo_name ? `<div class="small muted proj-repo">${icon("git")} ${esc(p.repo_name)}</div>` : ""}
      ${(p.tags || []).length ? `<div class="tag-list">${p.tags.map(tagChip).join("")}</div>` : ""}
      <div class="small">${t("{week} observations in 7 days · {total} total · {lessons} lessons · {sessions} sessions", { week: p.last_week, total: p.episodes, lessons: p.lessons, sessions: p.sessions })}${links ? ` · ${t("{n} links", { n: links })}` : ""}</div>
      <div class="proj-types">${types}</div>
      ${sm?.next_steps ? `<div class="next"><b>${t("Next steps:")}</b> ${esc(sm.next_steps)}</div>` : sm ? `<div class="next muted small">${esc(sm.title)}</div>` : ""}
      ${ls ? `<div class="small muted">${t("Last session:")} ${esc(ls.client)} · ${esc(ago(ls.at))} · ${esc(ls.status)}${ls.first_prompt ? " · " + esc(ls.first_prompt.slice(0, 80)) : ""}</div>` : ""}
      ${engramRow(p)}
    </div>`;
  };
  // Baris memori tim (.engram/) untuk proyek yang tertaut ke repo git.
  const engramRow = (p) => {
    const e = p.engram;
    if (!e && !p.repo) return "";
    if (e?.state === "active") return `<div class="proj-engram small"><span class="badge now">${t(".engram active")}</span><span class="muted">${t("project memory is shared through the repo")}</span></div>`;
    const label = e?.state === "bundle" ? t("Enable .engram") : t("Set up .engram");
    const hint = e?.state === "bundle" ? t("The repo already holds team memory; enable it so this device writes too") : e ? t("Create a .engram/ folder in {root}", { root: e.root }) : t("Repo folder unknown; enter its path on the project page");
    return `<div class="proj-engram small"><button type="button" class="primary" data-engram="${esc(p.project)}" title="${esc(hint)}">${icon("git")} ${label}</button>${e?.state === "bundle" ? `<span class="badge warn">${t("has team memory")}</span>` : ""}</div>`;
  };
  // Tag unik (tanpa membedakan huruf besar-kecil) beserta jumlah proyeknya.
  const tags = [];
  for (const p of r.projects) for (const tg of p.tags || []) {
    const hit = tags.find((x) => x.key === tg.toLowerCase());
    if (hit) hit.n++; else tags.push({ key: tg.toLowerCase(), label: tg, n: 1 });
  }
  tags.sort((a, b) => a.key.localeCompare(b.key));
  const untagged = r.projects.filter((p) => !(p.tags || []).length);
  let sel = localStorage.getItem("proj-tag") || "";
  if (sel && sel !== "__none" && !tags.some((tg) => tg.key === sel.toLowerCase())) sel = "";
  const hasTag = (p, k) => (p.tags || []).some((tg) => tg.toLowerCase() === k);
  const grid = (list) => `<div class="grid proj-grid">${list.map(projCard).join("")}</div>`;
  let body;
  if (!r.projects.length) {
    body = `<div class="card empty-state"><p><b>${t("No project memory yet.")}</b></p><ol class="small">
      <li>${t("Install the hooks:")} <code>scripts/install-mcp.sh --client claude</code> ${t("(or gemini/antigravity).")}</li>
      <li>${t("Open a new agent session and do some work; every turn is recorded automatically.")}</li>
      <li>${t("Or import existing memory:")} <code>engram import-claude-memory --all --apply</code>, <code>engram repo import &lt;repo&gt;</code>.</li></ol></div>`;
  } else if (sel === "__none") {
    body = untagged.length ? grid(untagged) : `<p class="muted">${t("Every project already has a tag.")}</p>`;
  } else if (sel) {
    body = grid(r.projects.filter((p) => hasTag(p, sel.toLowerCase())));
  } else if (tags.length) {
    body = tags.map((tg) => `<h3 class="group-head">${icon("tag")} ${esc(tg.label)} <span class="small muted">${t("{n} projects", { n: tg.n })}</span></h3>${grid(r.projects.filter((p) => hasTag(p, tg.key)))}`).join("")
      + (untagged.length ? `<h3 class="group-head">${t("Untagged")} <span class="small muted">${t("{n} projects", { n: untagged.length })}</span></h3>${grid(untagged)}` : "");
  } else {
    body = grid(r.projects);
  }
  const filterBar = tags.length ? `<div class="tag-filter" role="toolbar" aria-label="${t("Filter projects by tag")}">
      <button type="button" data-filter="" class="${sel ? "" : "active"}">${t("All")}</button>
      ${tags.map((tg) => `<button type="button" data-filter="${esc(tg.label)}" class="${sel.toLowerCase() === tg.key ? "active" : ""}">#${esc(tg.label)} <span class="small muted">${tg.n}</span></button>`).join("")}
      ${untagged.length ? `<button type="button" data-filter="__none" class="${sel === "__none" ? "active" : ""}">${t("Untagged")} <span class="small muted">${untagged.length}</span></button>` : ""}
    </div>` : (r.projects.length ? `<p class="small muted">${t("Group projects by adding tags on the project page (Edit project info button).")}</p>` : "");
  el.innerHTML = `<h1>${t("Projects")}</h1>
    <h2>${t("Integrations")}</h2>${health}
    <h2>${t("Active projects")}</h2>
    ${filterBar}
    ${body}
    ${r.episodes_without_project ? `<p class="small muted">${t("{n} memories without a project (personal stories or older sources) are in", { n: r.episodes_without_project })} <a href="#timeline">${t("Timeline")}</a>.</p>` : ""}`;
  wireInstallHooks(el);
  el.querySelectorAll("[data-filter]").forEach((b) => { b.onclick = () => { localStorage.setItem("proj-tag", b.dataset.filter); route(); }; });
  bindTagChips(el);
  el.querySelectorAll("[data-proj]").forEach((c) => {
    const go = () => { location.hash = projHref(c.dataset.proj); };
    c.onclick = go;
    c.onkeydown = (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); go(); } };
  });
  el.querySelectorAll("[data-engram]").forEach((b) => {
    b.onkeydown = (e) => e.stopPropagation();
    b.onclick = async (e) => {
      e.stopPropagation();
      const name = b.dataset.engram;
      const p = r.projects.find((x) => x.project === name);
      // Akar repo belum diketahui: buka halaman proyek, isi path di panel memori tim.
      if (!p?.engram) { sessionStorage.setItem("share-focus", "1"); location.hash = projHref(name); return; }
      b.disabled = true;
      try {
        const res = await api(`/api/subjects/${S()}/projects/share`, { method: "POST", json: { project: name, action: "init", root: p.engram.root } });
        const ex = res.result?.export || {};
        toast(ex.unchanged ? t(".engram/ already exists and is unchanged") : t(".engram/ created in {root}. Commit that folder so teammates receive it", { root: p.engram.root }));
        route();
      } catch (err) { toast(err.message); b.disabled = false; }
    };
  });
}

// Panel info proyek di atas tab: repositori, tag, catatan dengan [[tautan]], backlink.
async function projectMetaPanel(box, name) {
  let d;
  try { d = await api(`/api/subjects/${S()}/projects/meta?project=${encodeURIComponent(name)}`); }
  catch (e) { box.innerHTML = `<div class="card small muted">${t("Could not load project info:")} ${esc(e.message)}</div>`; return; }
  const m = d.meta || { repo: null, notes: "", tags: [], links: [] };
  const repo = m.repo || d.repo_detected;
  const epBack = (d.episode_backlinks || []).filter((l) => !d.backlinks.some((b) => sameProj(b, l)));
  const show = () => {
    const links = m.links || [];
    box.innerHTML = `<div class="card proj-meta">
      <div class="proj-meta-head">
        <div class="proj-meta-repo">${icon("git")} ${repo
          ? `${d.repo_url ? `<a href="${esc(d.repo_url)}" id="repo-link" title="${esc(repo)}">${esc(d.repo_name || repo)}</a>` : `<code>${esc(repo)}</code>`}${!m.repo ? ` <span class="small muted">${t("(detected from .git/config)")}</span>` : ""}`
          : `<span class="muted small">${t("No git repository set")}</span>`}</div>
        <button type="button" id="meta-edit">${icon("edit")} ${t("Edit project info")}</button>
      </div>
      <div class="tag-list">${m.tags.length ? `${icon("tag")} ${m.tags.map(tagChip).join("")}` : `<span class="small muted">${t("No group tags yet.")}</span>`}</div>
      ${m.notes ? `<div class="proj-notes">${renderNotes(m.notes)}</div>` : ""}
      ${links.length || d.backlinks.length || epBack.length ? `<div class="proj-links small">
        ${links.length ? `<div>${icon("link")} <b>${t("Links to")}</b> ${links.map((l) => wikiLink(l)).join(" ")}</div>` : ""}
        ${d.backlinks.length ? `<div>${icon("link")} <b>${t("Linked from")}</b> ${d.backlinks.map((l) => wikiLink(l)).join(" ")}</div>` : ""}
        ${epBack.length ? `<div>${icon("link")} <b>${t("Linked in memories")}</b> ${epBack.map((l) => wikiLink(l)).join(" ")}</div>` : ""}
        <div><button type="button" class="ghost" id="meta-graph">${icon("graph")} ${t("View in graph")}</button> <a class="small" href="#graf/${encodeURIComponent(name)}">${t("Open full graph")}</a></div>
      </div>` : ""}
    </div>`;
    bindTagChips(box);
    const rl = $("#repo-link", box);
    if (rl) rl.onclick = (e) => { e.preventDefault(); openRepo(name, d.repo_url); };
    $("#meta-edit", box).onclick = edit;
    const mg = $("#meta-graph", box);
    if (mg) mg.onclick = () => { const tb = document.querySelector('[data-tab="graf"]'); if (tb) { tb.click(); tb.scrollIntoView({ block: "start", behavior: "smooth" }); } };
  };
  const edit = () => {
    let tags = [...m.tags];
    const names = [...new Set([...state.projects.map((p) => p.project), ...(d.all_projects || [])])].filter((n) => !sameProj(n, name)).sort((a, b) => a.localeCompare(b));
    box.innerHTML = `<form class="card proj-meta" id="meta-form">
      <label class="field">${t("Git repository")}
        <input type="text" id="mf-repo" value="${esc(m.repo || "")}" placeholder="${esc(d.repo_detected || t("e.g. git@github.com:org/repo.git or github.com/org/repo"))}" autocomplete="off" spellcheck="false">
        ${d.repo_detected && !m.repo ? `<span class="small muted">${t("Leave empty to use the detected remote:")} ${esc(d.repo_detected)}</span>` : ""}
      </label>
      <label class="field">${t("Group tags")} <span class="small muted">${t("(Enter or comma to add)")}</span>
        <div class="tag-editor"><span id="mf-tags"></span><input type="text" id="mf-tag" list="mf-tag-list" placeholder="${t("e.g. OPERON")}" autocomplete="off"></div>
        <datalist id="mf-tag-list">${(d.all_tags || []).map((tg) => `<option value="${esc(tg)}">`).join("")}</datalist>
      </label>
      <label class="field">${t("Notes")} <span class="small muted">${t("(type <code>[[</code> to link another project)")}</span>
        <div class="wl-wrap"><textarea id="mf-notes" placeholder="${t("Example: Frontend for [[api-master-sensor]], using the schema from [[TABULAR]].")}">${esc(m.notes || "")}</textarea><div class="wl-suggest" id="mf-suggest" hidden></div></div>
      </label>
      <div class="row"><button class="primary" type="submit">${icon("check")} ${t("Save")}</button><button type="button" id="mf-cancel">${t("Cancel")}</button></div>
    </form>`;
    const tagBox = $("#mf-tags", box), tagIn = $("#mf-tag", box);
    const drawTags = () => {
      tagBox.innerHTML = tags.map((tg, i) => `<span class="tag-chip">#${esc(tg)}<button type="button" class="tag-x" data-i="${i}" aria-label="${t("Remove tag {tag}", { tag: esc(tg) })}">${icon("x")}</button></span>`).join("");
      tagBox.querySelectorAll("[data-i]").forEach((b) => { b.onclick = () => { tags.splice(+b.dataset.i, 1); drawTags(); }; });
    };
    const addTag = () => {
      for (const raw of tagIn.value.split(/[,\s]+/)) {
        const tg = raw.replace(/^#+/, "").trim();
        if (tg && !tags.some((x) => x.toLowerCase() === tg.toLowerCase())) tags.push(tg);
      }
      tagIn.value = ""; drawTags();
    };
    tagIn.onkeydown = (e) => {
      if (e.key === "Enter" || e.key === ",") { e.preventDefault(); addTag(); }
      else if (e.key === "Backspace" && !tagIn.value && tags.length) { tags.pop(); drawTags(); }
    };
    tagIn.onchange = () => { if (tagIn.value.trim()) addTag(); };
    drawTags();
    // Saran [[tautan]]: teks setelah `[[` terakhir sebelum kursor.
    const ta = $("#mf-notes", box), sug = $("#mf-suggest", box);
    const pending = () => { const m2 = ta.value.slice(0, ta.selectionStart).match(/\[\[([^\[\]\n|#]*)$/); return m2 ? m2[1] : null; };
    const pick = (n) => {
      const pos = ta.selectionStart, before = ta.value.slice(0, pos), after = ta.value.slice(pos);
      const start = before.lastIndexOf("[[") + 2;
      const rest = after.startsWith("]]") ? after.slice(2) : after;
      ta.value = before.slice(0, start) + n + "]]" + rest;
      const c = start + n.length + 2;
      ta.setSelectionRange(c, c); ta.focus(); sug.hidden = true;
    };
    const suggest = () => {
      const q = pending();
      if (q === null) { sug.hidden = true; return; }
      const hits = names.filter((n) => n.toLowerCase().includes(q.trim().toLowerCase())).slice(0, 8);
      if (!hits.length) { sug.hidden = true; return; }
      sug.innerHTML = hits.map((n, i) => `<button type="button" class="${i === 0 ? "active" : ""}" data-n="${esc(n)}">${esc(n)}</button>`).join("");
      sug.querySelectorAll("[data-n]").forEach((b) => { b.onmousedown = (e) => { e.preventDefault(); pick(b.dataset.n); }; });
      sug.hidden = false;
    };
    ta.oninput = suggest;
    ta.onclick = suggest;
    ta.onblur = () => { sug.hidden = true; };
    ta.onkeydown = (e) => {
      if (sug.hidden) return;
      const items = [...sug.querySelectorAll("button")];
      const cur = items.findIndex((b) => b.classList.contains("active"));
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        const nx = (cur + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length;
        items.forEach((b, i) => b.classList.toggle("active", i === nx));
      } else if ((e.key === "Enter" || e.key === "Tab") && cur >= 0) { e.preventDefault(); pick(items[cur].dataset.n); }
      else if (e.key === "Escape") { e.stopPropagation(); sug.hidden = true; }
    };
    $("#mf-cancel", box).onclick = show;
    $("#meta-form", box).onsubmit = async (e) => {
      e.preventDefault();
      if (tagIn.value.trim()) addTag();
      try {
        const res = await api(`/api/subjects/${S()}/projects/meta`, { method: "PUT", json: { project: name, repo: $("#mf-repo", box).value.trim() || null, notes: ta.value, tags } });
        toast(res.meta ? t("Project info saved") : t("Project info cleared"));
        await projectMetaPanel(box, name);
      } catch (err) { toast(err.message); }
    };
    $("#mf-repo", box).focus();
  };
  show();
}

// Panel memori tim lewat repo git (.engram/): status anggota, impor/ekspor, hook git.
async function projectSharePanel(box, name) {
  let d;
  try { d = await api(`/api/subjects/${S()}/projects/share?project=${encodeURIComponent(name)}`); }
  catch (e) { box.innerHTML = `<div class="card small muted">${t("Could not load team memory status:")} ${esc(e.message)}</div>`; return; }
  const act = async (action, extra = {}) => {
    try {
      const res = await api(`/api/subjects/${S()}/projects/share`, { method: "POST", json: { project: name, action, ...extra } });
      const r = res.result || {};
      const ex = r.export || r;
      if (action === "init" || action === "export") toast(ex.unchanged ? t("Project memory unchanged") : t("Written to .engram/. Commit that folder so teammates receive it"));
      else if (action === "import") toast(r.members && r.members.length ? t("Merged from {n} members", { n: r.members.length }) : t("No new memory from teammates"));
      else toast(action === "hooks" ? t("Git hooks installed") : t("Git hooks removed"));
      await projectSharePanel(box, name);
    } catch (err) { toast(err.message); }
  };
  if (!d.found) {
    box.innerHTML = `<div class="card proj-share">
      <div class="proj-share-head">${icon("users")} <b>${t("Team memory via git repo")}</b></div>
      <p class="small muted">${t("Anyone with access to this project's repo can share project conventions, decisions, and facts through the <code>.engram/</code> folder. The repo folder is unknown; start an agent session in that repo, or enter its path.")}</p>
      <form class="row" id="share-root"><input type="text" id="share-path" placeholder="${t("/path/to/repo")}" autocomplete="off" spellcheck="false"><button class="primary" type="submit">${icon("git")} ${t("Set up .engram")}</button></form>
    </div>`;
    $("#share-root", box).onsubmit = (e) => { e.preventDefault(); const root = $("#share-path", box).value.trim(); if (root) act("init", { root }); };
    if (sessionStorage.getItem("share-focus")) { sessionStorage.removeItem("share-focus"); box.scrollIntoView({ block: "center" }); $("#share-path", box).focus(); }
    return;
  }
  const s = d.status;
  const counts = (c) => t("{lessons} lessons · {decisions} decisions · {facts} facts", { lessons: c.lessons || 0, decisions: c.decisions || 0, facts: c.facts || 0 });
  const state = s.active
    ? `<span class="badge now">${t("active")}</span> ${t("This device's project memory is written to <code>.engram/</code>; teammates receive it after it is committed and pulled.")}`
    : s.bundle
      ? `<span class="badge warn">${t("receiving")}</span> ${t("The repo contains team memory that is merged automatically. Enable it so this device writes too.")}`
      : `<span class="badge">${t("not active")}</span> ${t("Enable to start sharing project memory with anyone who has access to this repo.")}`;
  box.innerHTML = `<div class="card proj-share">
    <div class="proj-share-head">${icon("users")} <b>${t("Team memory via git repo")}</b> <code class="small" title="${esc(s.root)}">${esc(s.root)}</code></div>
    <div class="small">${state}</div>
    ${s.members.length ? `<ul class="proj-share-members">${s.members.map((m) => `<li><b>${esc(m.name)}</b> <span class="muted">${esc(m.device || "")}${m.own ? ` · ${t("this device")}` : ""}</span><span class="small muted">${counts(m.counts || {})}</span></li>`).join("")}</ul>` : ""}
    <div class="small muted">${t("Last import: {imp} · last export: {exp} · {n} teammate records merged · git hooks: {hooks}", { imp: s.last_import_at ? esc(ago(s.last_import_at)) : t("never"), exp: s.last_export_at ? esc(ago(s.last_export_at)) : t("never"), n: s.imported, hooks: s.hooks.length ? esc(s.hooks.join(", ")) : t("none") })}</div>
    ${s.pending_import ? `<div class="small">${icon("refresh")} ${t("There are <code>.engram/</code> changes not merged yet.")}</div>` : ""}
    ${s.last_error ? `<div class="small bad">${icon("alert")} ${esc(s.last_error)}</div>` : ""}
    ${!s.enabled ? `<div class="small muted">${icon("info")} ${t("Automation is turned off in memory.toml (<code>[repo_share] enabled = false</code>); the buttons below still work.")}</div>` : ""}
    <div class="row">
      ${s.active ? `<button type="button" data-act="export">${icon("upload")} ${t("Write now")}</button>` : `<button class="primary" type="button" data-act="init">${icon("git")} ${s.bundle ? t("Enable .engram") : t("Set up .engram")}</button>`}
      ${s.bundle ? `<button type="button" data-act="import">${icon("download")} ${t("Merge now")}</button>` : ""}
      ${s.hooks.length ? `<button type="button" data-act="unhooks">${icon("git")} ${t("Remove git hooks")}</button>` : `<button type="button" data-act="hooks" title="${t("post-merge & post-checkout merge after a pull; pre-commit writes the bundle before a commit")}">${icon("git")} ${t("Install git hooks")}</button>`}
    </div>
  </div>`;
  box.querySelectorAll("[data-act]").forEach((b) => { b.onclick = () => { b.disabled = true; act(b.dataset.act); }; });
}

// ---------------------------------------------------------------- graf proyek
// Panel, layout 3D/2D, dan interaksi graf ada di graph.js (dimuat sebelum app.js).
// URL: #graf = graf global; #graf/<proyek terenkode>[?depth=N] = graf lokal satu proyek.
async function viewGraph(el) {
  // `route()` sudah men-decode argumen; baca hash mentah agar `?depth=` tidak tertukar dengan nama proyek.
  const [raw = "", qs = ""] = (location.hash.slice(1).split("/")[1] || "").split("?");
  let project = "";
  try { project = decodeURIComponent(raw); } catch (e) { project = raw; }
  const depth = Math.min(3, Math.max(1, +new URLSearchParams(qs).get("depth") || +(sessionStorage.getItem("proj-graph-depth") || 1)));
  el.innerHTML = `<h1>${t("Project graph")}</h1>
    <p class="small muted">${t("Projects as nodes on a globe, like an Obsidian notes graph. Arrows = <code>[[...]]</code> links from project notes or agent memory; dashed lines = mentions of other projects in memory; dotted lines = group tags; thin lines = tools, organizations, or people used by several projects.")}</p>
    <div id="graph-box"></div>`;
  if (!state.projects.length) await loadProjects();
  await graphPanel($("#graph-box", el), { project, depth, page: true });
}

// Halaman satu proyek: Konteks, Keputusan, Konvensi, Berkas, Sesi, Fakta, Graf.
async function viewProject(el, name) {
  const q = `?project=${encodeURIComponent(name)}`;
  const tabs = [["konteks", t("Agent context")], ["keputusan", t("Decisions")], ["konvensi", t("Conventions & pitfalls")], ["berkas", t("Files")], ["sesi", t("Sessions & observations")], ["fakta", t("Facts")], ["graf", t("Graph")]];
  const cur = sessionStorage.getItem("proj-tab") || "konteks";
  el.innerHTML = `<p class="small"><a href="#proyek">${t("Projects")}</a> / <b>${esc(name)}</b></p><h1>${esc(name)}</h1>
    <div id="proj-meta"></div>
    <div id="proj-share"></div>
    <div class="tabs">${tabs.map(([k, l]) => `<button data-tab="${k}" class="${k === cur ? "active" : ""}">${esc(l)}</button>`).join("")}</div><div id="tab-body"><div class="empty">${t("Loading...")}</div></div>`;
  const body = $("#tab-body", el);
  const render = async (tab) => {
    sessionStorage.setItem("proj-tab", tab);
    el.querySelectorAll("[data-tab]").forEach((b) => b.classList.toggle("active", b.dataset.tab === tab));
    body.innerHTML = `<div class="empty">${t("Loading...")}</div>`;
    try {
      if (tab === "konteks") {
        const c = await api(`/api/subjects/${S()}/context/inject${q}`);
        body.innerHTML = `<div class="card"><div class="row"><button class="primary" id="copy-ctx">${icon("download")} ${t("Copy context")}</button><span class="small muted">${c.found ? t("{chars} characters · {obs} observations · {sessions} sessions · {red} reductions to fit", { chars: c.chars, obs: c.observations, sessions: c.sessions, red: c.reductions }) : t("No memory for this project yet.")}</span></div>
          <p class="small muted">${t("This block is exactly what is injected into the agent when a session starts (SessionStart hook). The agent then fetches details by ID with get_episodes.")}</p>
          <pre class="src" id="ctx-pre">${citeLinks(c.text || "")}</pre></div>`;
        $("#copy-ctx", body).onclick = () => copyText(c.text || "");
      } else if (tab === "keputusan" || tab === "fakta") {
        const pc = await api(`/api/subjects/${S()}/project-context${q}`);
        if (tab === "keputusan") {
          body.innerHTML = `${pc.decisions.length ? `<div class="card">${pc.decisions.map((d) => `<div class="fact"><b>${esc(d.situation)}</b> &rarr; ${esc(d.action)}${d.reason ? `<div class="small muted">${t("Reason:")} ${esc(d.reason)}</div>` : ""}</div>`).join("")}</div>` : `<p class="muted">${t("No decisions recorded yet. Decisions come from work sessions (decision observations) or the remember tool.")}</p>`}
            ${pc.principles.length ? `<h2>${t("Relevant work principles")}</h2><div class="card">${pc.principles.map((p) => `<div class="fact">${esc(p.principle.statement)} <span class="small muted">(${(p.principle.confidence * 100).toFixed(0)}%)</span></div>`).join("")}</div>` : ""}`;
        } else {
          body.innerHTML = `<h2>${t("Current ({n})", { n: pc.current_facts.length })}</h2><div class="card">${pc.current_facts.map(factLine).join("") || `<p class="muted">${t("None yet.")}</p>`}</div>
            <h2>${t("History ({n})", { n: pc.history_facts.length })}</h2><div class="card">${pc.history_facts.map(factLine).join("") || `<p class="muted">${t("None yet.")}</p>`}</div>`;
        }
      } else if (tab === "konvensi") {
        const ls = await api(`/api/subjects/${S()}/lessons${q}`);
        body.innerHTML = ls.length ? `<div class="card">${ls.map(lessonRow).join("")}</div>` : `<p class="muted">${t("No lessons yet. Lessons come from your corrections and conventions mentioned during work sessions.")}</p>`;
      } else if (tab === "berkas") {
        const f = await api(`/api/subjects/${S()}/projects/files${q}`);
        body.innerHTML = f.files.length ? `<table class="tbl"><thead><tr><th>${t("File")}</th><th>${t("Modified")}</th><th>${t("Read")}</th><th>${t("Observations")}</th></tr></thead><tbody>${f.files.slice(0, 300).map((x) => `<tr><td><code>${esc(x.path)}</code></td><td>${x.modified}</td><td>${x.read}</td><td>${x.episodes.map((e) => `<a class="cite" data-ep="${esc(e.id)}" title="${esc(e.title)}">${esc(e.at)}${e.modified ? "*" : ""}</a>`).join(" ")}</td></tr>`).join("")}</tbody></table><p class="small muted">${t("* = file modified in that observation. This history is also inserted for the agent before it reads the file (PreToolUse hook).")}</p>` : `<p class="muted">${t("No files recorded yet; they appear after work sessions with the PostToolUse hook.")}</p>`;
      } else if (tab === "graf") {
        await graphPanel(body, { project: name, depth: +(sessionStorage.getItem("proj-graph-depth") || 1) });
      } else if (tab === "sesi") {
        const [sessions, obs] = await Promise.all([api(`/api/subjects/${S()}/sessions${q}`), api(`/api/subjects/${S()}/episodes${q}&newest_first=true&limit=200`)]);
        body.innerHTML = `${sessionTable(sessions)}<h2>${t("Observations")}</h2>${obsList(obs.filter((e) => e.obs_type))}`;
      }
    } catch (e) { body.innerHTML = `<div class="card">${t("Failed to load:")} ${esc(e.message)}</div>`; }
    bindCites(body);
    body.querySelectorAll("[data-session]").forEach((tr) => { tr.onclick = () => showSession(tr.dataset.session); });
  };
  el.querySelectorAll("[data-tab]").forEach((b) => { b.onclick = () => render(b.dataset.tab); });
  if (!state.projects.length) await loadProjects();
  await projectMetaPanel($("#proj-meta", el), name);
  projectSharePanel($("#proj-share", el), name);
  render(cur);
}

function lessonRow(l) {
  const kindLabel = { correction: t("correction"), convention: t("convention"), pitfall: t("pitfall"), discovery: t("discovery") };
  return `<div class="fact"><span class="badge l-${esc(l.kind)}">${esc(kindLabel[l.kind] || l.kind)}</span> <b>${esc(l.title)}</b> <span class="small muted">x${l.occurrences}${l.project ? " · " + esc(l.project) : ""}${l.promoted_to ? ` · ${t("promoted to a principle")}` : ""}</span>
    ${l.detail ? `<div class="small">${esc(l.detail)}</div>` : ""}
    <div class="small">${(l.evidence || []).slice(0, 4).map((ev) => `<a class="cite" data-ep="${esc(ev.episode_id)}">[ep:${esc(ev.episode_id.slice(0, 6))}]</a>`).join(" ")}</div></div>`;
}
function sessionTable(sessions) {
  return sessions.length ? `<table class="tbl"><thead><tr><th>${t("Started")}</th><th>${t("Client")}</th><th>${t("Project")}</th><th>${t("First prompt")}</th><th>${t("Prompts")}</th><th>${t("Tools")}</th><th>${t("Episodes")}</th><th>${t("Status")}</th></tr></thead><tbody>
    ${sessions.map((s) => `<tr data-session="${esc(s.id)}" style="cursor:pointer"><td>${esc(when(s.started_at))}</td><td>${esc(s.client)}</td><td>${esc(s.project || "-")}</td><td>${esc(s.first_prompt || "")}</td><td>${s.prompt_counter}</td><td>${s.tool_uses}</td><td>${s.episodes}</td><td><span class="badge ${s.status === "active" ? "now" : "past"}">${esc(s.status)}</span></td></tr>`).join("")}</tbody></table>` : `<p class="muted">${t("No sessions recorded yet.")}</p>`;
}
function obsList(obs) {
  return obs.length ? obs.slice(0, 200).map((e) => `<div class="tl-item" data-ep="${esc(e.id)}"><span class="tl-time">${esc(when(e.created_at))}<br><span class="small">${esc(e.project || "")}</span></span>
    <span>${typeBadge(e.obs_type)} <b>${esc(e.title || t("Untitled"))}</b><br><span class="small muted">${esc(e.summary || e.text.slice(0, 200))}</span>
    ${(e.files_modified || []).length ? `<br><span class="small">${t("modified:")} ${e.files_modified.slice(0, 5).map(esc).join(", ")}</span>` : ""}
    ${(e.links || []).length ? `<br><span class="small">${icon("link")} ${e.links.map((l) => wikiLink(l)).join(" ")}</span>` : ""}</span></div>`).join("") : `<p class="muted">${t("No observations yet.")}</p>`;
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
    <p class="small">${t("Episodes {episodes} · working memory {stream} · dropped (noise) {dropped} · not about the subject {notAbout} · facts {facts} · quotes rejected {rejected}", { episodes: s.stats.episodes, stream: s.stats.stream_items, dropped: s.stats.dropped, notAbout: s.stats.not_about_subject, facts: s.stats.facts, rejected: s.stats.facts_rejected })}</p>
    ${s.error ? `<p class="badge conflict">${esc(s.error)}</p>` : ""}
    ${s.status === "failed" ? `<div class="row"><button id="src-retry">${icon("refresh")} ${t("Reprocess")}</button><span class="muted small">${t("Retried automatically when the LLM &amp; embedding servers are up.")}</span></div>` : ""}
    <h3>${t("Episodes from this source")}</h3>
    ${r.episodes.map((ep) => `<div class="tl-item" data-ep="${esc(ep.id)}"><span class="tl-time">${esc(labelTime(ep))}</span><span>${esc(ep.title)}</span></div>`).join("") || `<p class="muted">${t("None yet.")}</p>`}
    <h3>${t("Original text")} <span class="muted small">${t("(highlights = quotes used as evidence)")}</span></h3><pre class="src">${text}</pre>`);
  const rb = $("#src-retry");
  if (rb) rb.onclick = async () => {
    rb.disabled = true;
    try { await api(`/api/sources/${encodeURIComponent(id)}/retry`, { method: "POST" }); toast(t("Queued")); showSource(id); }
    catch (e) { toast(e.message); rb.disabled = false; }
  };
}

// ---------------------------------------------------------------- views
async function viewHome(el) {
  const r = await api(`/api/subjects/${S()}/stats`);
  const st = r.stats;
  const items = [
    ["episodes", t("Stories"), st.episodes],
    ["retellings", t("Retold"), st.retellings],
    ["stream", t("Working memory"), st.stream_items],
    ["entities", t("Entities & projects"), st.entities],
    ["facts", t("Facts"), st.facts],
    ["kin", t("Family relations"), st.kin],
    ["principles", t("Principles"), st.principles],
    ["dispositions", t("Traits & values"), st.dispositions],
    ["reviews", t("Awaiting review"), st.open_reviews],
    ["sources", t("Sources"), st.sources],
  ];
  el.innerHTML = `<h1>${esc(r.subject?.name || state.subject)}</h1>
    <p class="muted">${r.subject?.anchors?.birth ? t("Born {date}", { date: esc(r.subject.anchors.birth) }) + " · " : ""}<a class="cite" id="show-jobs">${t("{n} jobs in queue", { n: r.pending_jobs })}</a></p>
    <div class="grid">${items.map(([k, label, v]) => `<div class="card stat" role="button" tabindex="0" data-stat="${k}" data-label="${esc(label)}"><b>${v}</b><span>${label}</span></div>`).join("")}</div>
    <div id="stat-panel"></div>
    <h2>${t("Maintenance")}</h2>
    <div class="row">
      <button data-m="review">${icon("flag")} ${t("Auto review (agy)")}</button>
      <button data-m="consolidate">${icon("refresh")} ${t("Consolidate")}</button>
      <button data-m="compact">${icon("book")} ${t("Compact memory")}</button>
      <button data-m="distill">${icon("compass")} ${t("Distill principles")}</button>
      <button data-m="voice">${icon("mic")} ${t("Speaking style profile")}</button>
      <button id="retry-failed">${icon("refresh")} ${t("Reprocess failed items")}</button>
    </div>
    <h2>${t("Subject data")}</h2>
    <div class="card"><div class="row">
      <label>${t("Date of birth")} <input type="date" id="birth" value="${esc(r.subject?.anchors?.birth || "")}"></label>
      <button class="primary" id="save-birth">${t("Save")}</button>
      <span class="muted small">${t("Used to turn \"age 20\" or \"in grade school\" into years.")}</span>
    </div></div>`;
  let activeKey = null;
  const selectStat = (key, label, cardEl) => {
    const panel = $("#stat-panel", el);
    if (activeKey === key) {
      activeKey = null;
      el.querySelectorAll(".card.stat").forEach((c) => c.classList.remove("active"));
      panel.innerHTML = "";
      return;
    }
    activeKey = key;
    el.querySelectorAll(".card.stat").forEach((c) => c.classList.toggle("active", c === cardEl));
    showStatDetail(panel, key, label, () => {
      activeKey = null;
      el.querySelectorAll(".card.stat").forEach((c) => c.classList.remove("active"));
      panel.innerHTML = "";
    });
  };
  el.querySelectorAll("[data-stat]").forEach((card) => {
    card.onclick = () => selectStat(card.dataset.stat, card.dataset.label, card);
    card.onkeydown = (e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        selectStat(card.dataset.stat, card.dataset.label, card);
      }
    };
  });
  $("#show-jobs", el).onclick = () => selectStat("jobs", t("Queued jobs"), null);
  el.querySelectorAll("[data-m]").forEach((b) => { b.onclick = async () => { await api(`/api/subjects/${S()}/maintenance/${b.dataset.m}`, { method: "POST" }); toast(t("Queued")); }; });
  $("#retry-failed", el).onclick = async () => {
    try {
      const r = await api("/api/jobs/retry-failed", { method: "POST" });
      toast(r.backend_error ? t("{n} failed, the server can't be reached yet", { n: r.failed }) : r.failed ? t("{n} sources requeued", { n: r.requeued }) : t("No failed sources"));
    } catch (e) { toast(e.message); }
  };
  $("#save-birth", el).onclick = async () => {
    await api("/api/subjects", { method: "POST", json: { id: state.subject, name: r.subject.name, birth: $("#birth", el).value } });
    toast(t("Saved"));
  };
}

async function showStatDetail(panel, key, label, onClose) {
  panel.innerHTML = `<div class="card" style="margin-top:12px"><div class="empty">${t("Loading {what}...", { what: esc(label.toLowerCase()) })}</div></div>`;
  const kinds = { person: t("People"), project: t("Projects & services"), component: t("Components & files"), tool: t("Tools & technology"), place: t("Places"), org: t("Organizations"), event: t("Events"), object: t("Objects"), concept: t("Concepts") };
  const revKinds = { gray_zone: t("Worth a story?"), conflict: t("Conflicting facts"), entity_merge: t("Same entity?"), quarantine: t("Doubtful facts"), reflective: t("Confirm principle/trait") };
  const revBadge = (r) => {
    if (r.kind === "entity_merge") {
      const ek = r.payload?.entity_kind;
      if (ek === "project") return t("Same project?");
      if (ek === "component") return t("Same component?");
      if (ek === "tool") return t("Same tool?");
      if (ek === "person") return t("Same person?");
    }
    return revKinds[r.kind] || r.kind;
  };
  let rows = [];
  let extraHead = "";
  let matchFn = () => true;
  let renderFn = () => "";

  try {
    if (key === "episodes" || key === "retellings") {
      const st = key === "retellings" ? "retelling" : "active";
      rows = await api(`/api/subjects/${S()}/episodes?status=${st}&limit=500`);
      if (key === "episodes") extraHead = `<a class="btn small" href="#timeline">${icon("clock")} ${t("Open Timeline")}</a>`;
      matchFn = (e, q) => [e.title, e.summary, e.text, e.life_phase, labelTime(e)].some((x) => (x || "").toLowerCase().includes(q));
      renderFn = (list) => list.map((e) => `<div class="tl-item" data-ep="${esc(e.id)}">
        <span class="tl-time">${esc(labelTime(e))}${e.life_phase ? `<br><span class="small">${esc(e.life_phase)}</span>` : ""}</span>
        <span><b>${esc(e.title || t("Untitled"))}</b><br><span class="small muted">${esc(e.summary || e.text.slice(0, 220))}</span>
        ${e.retelling_of ? `<br><span class="small">${t("Retelling of {link}", { link: `<a class="cite" data-ep="${esc(e.retelling_of)}">[ep:${esc(e.retelling_of.slice(0, 6))}]</a>` })}</span>` : ""}</span>
      </div>`).join("");
    } else if (key === "stream") {
      rows = await api(`/api/subjects/${S()}/stream?limit=500`);
      matchFn = (s, q) => [s.text, s.speaker].some((x) => (x || "").toLowerCase().includes(q));
      renderFn = (list) => list.map((s) => `<div class="fact">
        <div>${esc(s.text)}</div>
        <div class="small muted">${s.speaker ? `${t("Speaker: {name}", { name: esc(s.speaker) })} · ` : ""}${t("score {n}%", { n: (s.importance * 100).toFixed(0) })} · ${t("created {date}", { date: esc((s.created_at || "").slice(0, 10)) })} · ${t("decays {date}", { date: esc((s.expires_at || "").slice(0, 10)) })}
          · <a class="cite" data-source="${esc(s.source_id)}">${t("source")}</a>
          ${s.promoted_to ? ` · ${t("promoted to a {link}", { link: `<a class="cite" data-ep="${esc(s.promoted_to)}">${t("story")}</a>` })}` : ""}</div>
      </div>`).join("");
    } else if (key === "entities") {
      rows = await api(`/api/subjects/${S()}/entities?limit=500`);
      extraHead = `<a class="btn small" href="#orang">${icon("users")} ${t("Open People & Family tree")}</a>`;
      matchFn = (e, q) => [e.name, e.description, kinds[e.kind] || e.kind, ...(e.aliases || [])].some((x) => (x || "").toLowerCase().includes(q));
      renderFn = (list) => `<table><tr><th>${t("Name")}</th><th>${t("Kind")}</th><th>${t("Aliases & notes")}</th><th>${t("Mentions")}</th></tr>
        ${list.map((e) => `<tr data-entity="${esc(e.id)}" style="cursor:pointer">
          <td><b>${esc(e.name)}</b> ${e.status === "provisional" ? `<span class="badge warn">${t("mentioned once")}</span>` : ""}</td>
          <td><span class="badge">${esc(kinds[e.kind] || e.kind)}</span></td>
          <td class="small muted">${esc((e.aliases || []).join(", "))}${e.aliases?.length && e.description ? " · " : ""}${esc(e.description || "")}</td>
          <td class="small">${t("{n} stories", { n: e.mention_count })}</td>
        </tr>`).join("")}</table>`;
    } else if (key === "facts") {
      rows = await api(`/api/subjects/${S()}/facts?limit=500`);
      matchFn = (f, q) => [f.entity_name, f.predicate, f.object_text, f.category, f.statement, ...(f.evidence || []).map((e) => e.quote)].some((x) => (x || "").toLowerCase().includes(q));
      renderFn = (list) => list.map((f) => {
        const span = f.valid_to ? `${yr(f.valid_from)}–${yr(f.valid_to)}` : t("since {year}", { year: yr(f.valid_from) });
        const ev = (f.evidence || []).map((e) => `<a class="cite" data-ep="${esc(e.episode_id || e)}">${t("evidence")}</a>`).join(" ");
        const quote = f.evidence?.[0]?.quote ? `<div class="quote small">${esc(f.evidence[0].quote)}</div>` : "";
        return `<div class="fact">${factBadge(f)}${f.category ? `<span class="badge">${esc(f.category)}</span>` : ""}
          <a class="cite" data-entity="${esc(f.entity_id)}"><b>${esc(f.entity_name || f.entity_id)}</b></a> ${esc(f.predicate)} ${esc(f.object_text)}
          <span class="muted small">(${esc(span)}${f.time ? "; " + esc(f.time) : ""})</span> ${ev}${quote}</div>`;
      }).join("");
    } else if (key === "kin") {
      rows = await api(`/api/subjects/${S()}/kin`);
      extraHead = `<a class="btn small" href="#orang">${icon("users")} ${t("Open Family tree")}</a>`;
      matchFn = (k, q) => [k.from_name, k.to_name, k.relation_label, k.term].some((x) => (x || "").toLowerCase().includes(q));
      renderFn = (list) => list.map((k) => {
        const ev = (k.evidence || []).map((e) => `<a class="cite" data-ep="${esc(e.episode_id || e)}">${t("evidence")}</a>`).join(" ");
        const quote = k.evidence?.[0]?.quote ? `<div class="quote small">${esc(k.evidence[0].quote)}</div>` : "";
        return `<div class="fact">
          ${t("{from} is the {relation} of {to}", { from: `<a class="cite" data-entity="${esc(k.from)}"><b>${esc(k.from_name)}</b></a>`, relation: `<b>${esc(k.relation_label)}</b>`, to: `<a class="cite" data-entity="${esc(k.to)}"><b>${esc(k.to_name)}</b></a>` })}
          ${k.term && k.term !== k.relation_label ? `<span class="badge">${esc(k.term)}</span>` : ""}
          ${k.inferred ? `<span class="muted small">(${t("inferred")})</span>` : ""} ${ev}${quote}</div>`;
      }).join("");
    } else if (key === "principles") {
      rows = await api(`/api/subjects/${S()}/principles`);
      extraHead = `<a class="btn small" href="#berpikir">${icon("compass")} ${t("Open Ways of thinking")}</a>`;
      matchFn = (p, q) => [p.principle.statement, p.principle.domain, p.principle.origin].some((x) => (x || "").toLowerCase().includes(q));
      renderFn = (list) => list.map((p) => `<div class="fact">
        <b>${esc(p.principle.statement)}</b>
        <div class="small muted">${esc(p.principle.domain)}${p.principle.origin ? " · " + t("origin: {origin}", { origin: esc(p.principle.origin) }) : ""} · ${t("confidence {n}%", { n: (p.principle.confidence * 100).toFixed(0) })} · ${t("{n} supporting stories", { n: p.evidence.length })}</div>
        ${p.evidence.map((e) => `<div class="tl-item" data-ep="${esc(e.id)}"><span class="tl-time">${esc(e.time)}</span><span>${esc(e.title || e.excerpt)}</span></div>`).join("")}
      </div>`).join("");
    } else if (key === "dispositions") {
      const all = await api(`/api/subjects/${S()}/dispositions`);
      rows = all.filter((d) => d.status === "active");
      extraHead = `<a class="btn small" href="#berpikir">${icon("compass")} ${t("Open Ways of thinking")}</a>`;
      matchFn = (d, q) => [d.label, d.description].some((x) => (x || "").toLowerCase().includes(q));
      renderFn = (list) => list.map((d) => {
        const ev = (d.evidence || []).map((e) => `<a class="cite" data-ep="${esc(e.episode_id || e)}">${t("evidence")}</a>`).join(" ");
        return `<div class="fact"><b>${esc(d.label)}</b> <span class="badge">${d.kind === "value" ? t("value") : t("trait")}</span>
          <span class="small muted">${esc(yr(d.valid_from))} – ${d.valid_to ? esc(yr(d.valid_to)) : t("present")}</span> ${ev}
          <div class="small">${esc(d.description)}</div></div>`;
      }).join("");
    } else if (key === "reviews") {
      rows = await api(`/api/subjects/${S()}/reviews`);
      extraHead = `<a class="btn small" href="#review">${icon("flag")} ${t("Open Review page")}</a>`;
      matchFn = (r, q) => [r.summary, r.kind, r.payload?.text].some((x) => (x || "").toLowerCase().includes(q));
      renderFn = (list) => list.map((r) => `<div class="fact" data-rev="${esc(r.id)}">
        <span class="badge">${esc(revBadge(r))}</span> <span>${esc(r.summary)}</span>
        ${r.payload?.text ? `<pre class="src small" style="margin:6px 0">${esc(r.payload.text)}</pre>` : ""}
        <div class="row" style="margin-top:6px"><input type="text" placeholder="${t("Note (optional)")}" data-note><button class="primary" data-d="approve" data-id="${esc(r.id)}">${icon("check")} ${t("Approve")}</button>${r.kind === "conflict" ? `<button data-d="keep_both" data-id="${esc(r.id)}">${t("Keep both")}</button>` : ""}<button data-d="reject" data-id="${esc(r.id)}">${icon("x")} ${t("Reject")}</button></div>
      </div>`).join("");
    } else if (key === "sources") {
      rows = await api(`/api/subjects/${S()}/sources?limit=500`);
      extraHead = `<a class="btn small" href="#sumber">${icon("inbox")} ${t("Open Sources & import")}</a>`;
      matchFn = (s, q) => [s.title, s.kind, s.status].some((x) => (x || "").toLowerCase().includes(q));
      renderFn = (list) => `<table><tr><th>${t("Title")}</th><th>${t("Kind")}</th><th>${t("Status")}</th><th>${t("Result")}</th></tr>
        ${list.map((s) => `<tr data-source="${esc(s.id)}" style="cursor:pointer">
          <td>${esc(s.title)}<div class="small muted">${esc((s.created_at || "").slice(0, 10))}${s.occurred_from ? " · " + t("happened {date}", { date: esc(s.occurred_from) }) : ""}</div></td>
          <td>${esc(s.kind)}</td>
          <td><span class="badge ${s.status === "failed" ? "conflict" : s.status === "done" ? "now" : ""}">${esc(s.status)}</span></td>
          <td class="small">${t("{eps} stories, {wm} WM, {noise} noise", { eps: s.stats.episodes, wm: s.stats.stream_items, noise: s.stats.dropped })}</td>
        </tr>`).join("")}</table>`;
    } else if (key === "jobs") {
      const res = await api("/api/jobs?limit=200");
      rows = res.jobs || [];
      matchFn = (j, q) => [j.kind, j.status, j.stage, j.error].some((x) => (x || "").toLowerCase().includes(q));
      renderFn = (list) => `<table><tr><th>${t("Kind")}</th><th>${t("Status")}</th><th>${t("Stage")}</th><th>${t("Time")}</th></tr>
        ${list.map((j) => `<tr>
          <td><b>${esc(j.kind)}</b></td>
          <td><span class="badge ${j.status === "failed" ? "conflict" : j.status === "done" ? "now" : j.status === "running" ? "warn" : ""}">${esc(j.status)}</span></td>
          <td class="small">${esc(j.stage || "")}${j.progress_total ? ` (${j.progress_done}/${j.progress_total})` : ""}${j.error ? `<div style="color:var(--bad)">${esc(j.error)}</div>` : ""}</td>
          <td class="small muted">${esc((j.updated_at || j.created_at || "").slice(0, 19).replace("T", " "))}</td>
        </tr>`).join("")}</table>`;
    }
  } catch (e) {
    panel.innerHTML = `<div class="card" style="margin-top:12px">${t("Failed to load: {error}", { error: esc(e.message) })}</div>`;
    return;
  }

  panel.innerHTML = `<div class="card" style="margin-top:12px">
    <div class="row" style="justify-content:space-between;margin-bottom:8px">
      <div class="row"><h2 style="margin:0">${esc(label)} (<span id="stat-count">${rows.length}</span>)</h2>${extraHead}</div>
      <div class="row">
        ${rows.length > 3 ? `<input type="search" id="stat-q" placeholder="${t("Filter {what}...", { what: esc(label.toLowerCase()) })}">` : ""}
        <button class="ghost" id="stat-close" title="${t("Close")}">${icon("x")}</button>
      </div>
    </div>
    <div id="stat-body"></div>
  </div>`;

  $("#stat-close", panel).onclick = onClose;
  const body = $("#stat-body", panel);
  const draw = () => {
    const q = ($("#stat-q", panel)?.value || "").trim().toLowerCase();
    const filtered = q ? rows.filter((r) => matchFn(r, q)) : rows;
    $("#stat-count", panel).textContent = filtered.length;
    body.innerHTML = filtered.length ? renderFn(filtered) : `<div class="empty">${t("No data.")}</div>`;
    bindCites(body);
    if (key === "reviews") {
      body.querySelectorAll("[data-d]").forEach((b) => {
        b.onclick = async () => {
          const item = b.closest("[data-rev]");
          b.disabled = true;
          try {
            await api(`/api/reviews/${encodeURIComponent(b.dataset.id)}`, { method: "POST", json: { decision: b.dataset.d, note: item.querySelector("[data-note]").value || null } });
            rows = rows.filter((x) => x.id !== b.dataset.id);
            const statB = document.querySelector('[data-stat="reviews"] b');
            if (statB) statB.textContent = rows.length;
            toast(t("Saved"));
            draw();
          } catch (e) { toast(e.message); b.disabled = false; }
        };
      });
    }
  };
  if ($("#stat-q", panel)) $("#stat-q", panel).oninput = draw;
  draw();
}

async function viewTimeline(el) {
  const tl = await api(`/api/subjects/${S()}/timeline?limit=2000`);
  const groups = new Map();
  for (const it of tl.items) {
    const k = it.life_phase || t("no phase");
    if (!groups.has(k)) groups.set(k, []);
    groups.get(k).push(it);
  }
  const phaseSummary = new Map(tl.phases.map((p) => [p.name, p.summary]));
  const fuzzy = (time) => !/^\d{2}-\d{2}-\d{4}$/.test(time);
  el.innerHTML = `<h1>${t("Timeline")}</h1>
    <p class="muted small">${t("Dashed time = estimate (\"age 20\", \"in grade school\", \"around 2015\").")}</p>
    ${[...groups].map(([phase, items]) => `<section class="phase"><h2>${esc(phase)}</h2>
      ${phaseSummary.get(phase) ? `<p class="muted">${esc(phaseSummary.get(phase))}</p>` : ""}
      ${items.map((it) => `<div class="tl-item" ${it.kind === "episode" ? `data-ep="${esc(it.id)}"` : ""}><span class="tl-time ${fuzzy(it.time) ? "fuzzy" : ""}">${esc(it.time)}<br><span class="small">${esc(it.date || "")}</span></span><span>${esc(it.label)}</span></div>`).join("")}
    </section>`).join("") || `<div class="empty">${t("No dated stories yet.")}</div>`}
    ${tl.undated.length ? `<h2>${t("Unknown time ({n})", { n: tl.undated.length })}</h2>${tl.undated.map((e) => `<div class="tl-item" data-ep="${esc(e.id)}"><span class="tl-time">${esc(e.time && e.time !== "waktu tidak diketahui" ? e.time : "?")}</span><span>${esc(e.title || e.excerpt)}</span></div>`).join("")}` : ""} `;
  bindCites(el);
}

async function viewPeople(el) {
  const [tree, ents] = await Promise.all([api(`/api/subjects/${S()}/family`), api(`/api/subjects/${S()}/entities?limit=500`)]);
  const self = tree.nodes.find((n) => n.is_self);
  const rel = new Map();
  if (self) for (const e of tree.edges) if (e.to === self.id && !rel.has(e.from)) rel.set(e.from, e);
  // Label relasi berasal dari data server (bahasa Indonesia); hanya dipakai untuk menentukan generasi.
  const gen = {
    2: ["buyut"], 1: ["kakek", "nenek", "kakek/nenek"], 0.5: ["ayah", "ibu", "orang tua", "pakde", "bude", "paman", "bibi", "paman/bibi", "mertua", "orang tua angkat"],
    0: ["suami", "istri", "pasangan", "kakak", "adik", "kakak laki-laki", "kakak perempuan", "saudara kandung", "sepupu", "ipar", "besan"], "-1": ["anak", "menantu", "keponakan", "anak angkat"], "-2": ["cucu", "cicit"]
  };
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
  const kinds = { person: t("People"), project: t("Projects & services"), component: t("Components & files"), tool: t("Tools & technology"), place: t("Places"), org: t("Organizations"), event: t("Events"), object: t("Objects"), concept: t("Concepts") };
  el.innerHTML = `<h1>${t("Entities & relations")}</h1>
    <div class="card">
      ${order.filter((l) => l > 0).map((l) => `<div class="tree-gen">${rows[l].map(person).join("")}</div>`).join("")}
      ${self ? `<div class="tree-gen"><div class="person self" data-entity="${esc(self.id)}"><b>${esc(self.name)}</b><small>${t("subject")}</small></div>${(rows[0] || []).map(person).join("")}</div>` : ""}
      ${order.filter((l) => l < 0).map((l) => `<div class="tree-gen">${rows[l].map(person).join("")}</div>`).join("")}
      ${tree.edges.length ? `<p class="muted small">${t("Dashed boxes = inferred relations (derived from other relations).")}</p>` : `<p class="empty">${t("No family relations yet.")}</p>`}
    </div>
    <div class="row"><input type="search" id="ent-q" placeholder="${t("Filter names...")}"></div>
    <div id="ent-list"></div>`;
  const entQ = $("#ent-q", el), entList = $("#ent-list", el);
  const draw = () => {
    const q = entQ.value.toLowerCase();
    const list = ents.filter((e) => !q || e.name.toLowerCase().includes(q) || e.aliases.some((a) => a.toLowerCase().includes(q)));
    const by = {};
    for (const e of list) (by[e.kind] ||= []).push(e);
    entList.innerHTML = Object.entries(by).map(([k, xs]) => `<h2>${kinds[k] || k} (${xs.length})</h2><table>${xs.map((e) => `<tr data-entity="${esc(e.id)}" style="cursor:pointer"><td><b>${esc(e.name)}</b> ${e.status === "provisional" ? `<span class="badge warn">${t("mentioned once")}</span>` : ""}</td><td class="small muted">${esc(e.aliases.join(", "))}</td><td class="small">${t("{n} stories", { n: e.mention_count })}</td></tr>`).join("")}</table>`).join("");
    bindCites(entList);
  };
  entQ.oninput = draw;
  draw();
  bindCites(el);
}

async function viewThinking(el) {
  const [ps, ds, voice] = await Promise.all([api(`/api/subjects/${S()}/principles`), api(`/api/subjects/${S()}/dispositions`), api(`/api/subjects/${S()}/voice`)]);
  const years = ds.flatMap((d) => [d.valid_from, d.valid_to].filter(Boolean).map((x) => +x.slice(0, 4)));
  const now = new Date().getFullYear();
  const min = Math.min(...years, now - 10), max = now;
  const pos = (y) => ((y - min) / Math.max(1, max - min)) * 100;
  el.innerHTML = `<h1>${t("Ways of thinking")}</h1>
    <h2>${t("Life principles")}</h2>
    ${ps.map((p) => `<div class="card"><h3>${esc(p.principle.statement)}</h3>
      <p class="small muted">${esc(p.principle.domain)}${p.principle.origin ? " · " + t("origin: {origin}", { origin: esc(p.principle.origin) }) : ""} · ${t("confidence {n}%", { n: (p.principle.confidence * 100).toFixed(0) })} · ${t("{n} supporting stories", { n: p.evidence.length })}</p>
      ${p.evidence.map((e) => `<div class="tl-item" data-ep="${esc(e.id)}"><span class="tl-time">${esc(e.time)}</span><span>${esc(e.title || e.excerpt)}</span></div>`).join("")}</div>`).join("") || `<p class="muted">${t("No principles with enough evidence yet (needs several stories from different periods).")}</p>`}
    <h2>${t("Traits & values over time")}</h2>
    ${ds.map((d) => {
    const a = d.valid_from ? pos(+d.valid_from.slice(0, 4)) : 0, b = d.valid_to ? pos(+d.valid_to.slice(0, 4)) : 100;
    return `<div class="card"><b>${esc(d.label)}</b> <span class="badge">${d.kind === "value" ? t("value") : t("trait")}</span>${d.status === "pending_review" ? `<span class="badge warn">${t("awaiting confirmation")}</span>` : ""}
        <div class="bar" title="${esc(d.valid_from || "?")} – ${esc(d.valid_to || t("present"))}"><span style="left:${a}%;width:${Math.max(2, b - a)}%"></span></div>
        <div class="small muted">${esc(yr(d.valid_from))} – ${d.valid_to ? esc(yr(d.valid_to)) : t("present")} · ${t("{n} evidence", { n: d.evidence.length })}</div><p>${esc(d.description)}</p></div>`;
  }).join("") || `<p class="muted">${t("None yet.")}</p>`}
    <h2>${t("Speaking style")}</h2>
    ${voice ? `<div class="card"><p>${esc(voice.style || t("Not described yet."))}</p>
      <p>${(voice.idioms || []).map(([k, n]) => `<span class="badge">${esc(k)} (${n}x)</span>`).join(" ")}</p>
      <p class="small muted">${t("Languages: {list}", { list: Object.entries(voice.language_mix || {}).map(([k, v]) => `${esc(k)} ${(v * 100).toFixed(0)}%`).join(", ") })}</p>
      ${(voice.sample_quotes || []).slice(0, 5).map((q) => `<div class="quote">${esc(q)}</div>`).join("")}</div>` : `<p class="muted">${t("Run \"Speaking style profile\" in Stats & maintenance.")}</p>`}`;
  bindCites(el);
}

async function viewSearch(el, prefill) {
  const projOpts = state.projects.map((p) => p.project);
  el.innerHTML = `<h1>${t("Search & ask")}</h1>
    <div class="card">
      <div class="row"><input type="search" id="q" placeholder="${t("e.g. why did the migration fail? decisions about caching? who is Uncle Karno?")}" value="${esc(prefill || "")}"><button class="primary" id="go-recall">${icon("search")} ${t("Search")}</button><button id="go-ask">${t("Ask")}</button></div>
      <div class="row small" style="margin-top:8px">
        <label>${t("Project")} <select id="sproj"><option value="">${t("all")}</option>${projOpts.map((n) => `<option value="${esc(n)}" ${n === P() ? "selected" : ""}>${esc(n)}</option>`).join("")}</select></label>
        <label>${t("Kind")} <select id="stype"><option value="">${t("all")}</option>${Object.entries(OBS).filter(([k]) => k !== "lesson").map(([k, v]) => `<option value="${k}">${v}</option>`).join("")}<option value="story">${t("story")}</option></select></label>
        <label>${t("From")} <input type="date" id="from"></label><label>${t("To")} <input type="date" id="to"></label>
        <label>${t("As of")} <input type="date" id="asof"></label>
        <label><input type="checkbox" id="persona"> ${t("Answer as the subject")}</label>
      </div>
    </div><div id="out"></div>`;
  const out = $("#out", el), qEl = $("#q", el);
  const body = () => ({ query: qEl.value, project: $("#sproj", el).value || null, as_of: $("#asof", el).value || null, from: $("#from", el).value || null, to: $("#to", el).value || null, detail: "full", limit: 30 });
  const tokens = (e) => Math.max(20, Math.round(Math.max((e.excerpt || "").length, (e.summary || "").length) / 4));
  $("#go-recall", el).onclick = async () => {
    if (!qEl.value.trim()) return;
    out.innerHTML = `<div class="empty">${t("Searching...")}</div>`;
    const r = await api(`/api/subjects/${S()}/recall`, { method: "POST", json: body() });
    if (!r.found) { out.innerHTML = `<div class="card">${esc(r.note)}</div>`; return; }
    const ty = $("#stype", el).value;
    const eps = r.episodes.filter((e) => !ty || (ty === "story" ? !e.obs_type : e.obs_type === ty));
    const facts = r.facts.filter((f) => f.status === "conflict" || (f.current && !["peristiwa", "event"].includes(f.category)));
    out.innerHTML = `${facts.length ? `<h2>${t("Facts")}</h2><div class="card">${facts.map(factLine).join("")}</div>` : ""}
      ${r.principles.length ? `<h2>${t("Related principles")}</h2>${r.principles.map((p) => `<div class="card">${esc(p.statement)}</div>`).join("")}` : ""}
      <h2>${t("Memory index ({n})", { n: eps.length })} <span class="small muted">${t("click a row for details; this is what agents see through recall")}</span></h2>
      <table class="tbl idx"><thead><tr><th>ID</th><th>${t("Date")}</th><th>${t("Kind")}</th><th>${t("Title")}</th><th>${t("Project")}</th><th>${t("Cost")}</th><th>${t("Via")}</th></tr></thead><tbody>
      ${eps.map((e) => `<tr data-ep="${esc(e.id)}" style="cursor:pointer"><td><code>ep:${esc(e.id.slice(0, 8))}</code></td><td>${esc(e.time_start || e.time)}</td><td>${typeBadge(e.obs_type)}</td><td><b>${esc(e.title)}</b><div class="small muted">${esc((e.summary || e.excerpt || "").slice(0, 140))}</div></td><td class="small">${esc(e.project || "")}</td><td class="small">~${tokens(e)} tok</td><td class="small muted">${esc(e.via.join(", "))}</td></tr>`).join("")}</tbody></table>`;
    bindCites(out);
  };
  $("#go-ask", el).onclick = async () => {
    if (!qEl.value.trim()) return;
    out.innerHTML = `<div class="empty">${t("Thinking...")}</div>`;
    const r = await api(`/api/subjects/${S()}/ask`, { method: "POST", json: { question: qEl.value, persona: $("#persona", el).checked, as_of: $("#asof", el).value || null } });
    out.innerHTML = `<div class="card"><div class="answer">${citeLinks(r.answer)}</div>${r.found ? "" : `<p class="muted small">${t("No relevant records; the answer was not made up.")}</p>`}</div>
      <details class="card"><summary>${t("Memory context used")}</summary><pre class="src">${citeLinks(r.context)}</pre></details>`;
    bindCites(out);
  };
  qEl.onkeydown = (e) => { if (e.key === "Enter") $("#go-recall", el).click(); };
  if (prefill) $("#go-recall", el).click();
}

async function viewInterview(el) {
  el.innerHTML = `<h1>${t("Guided interview")}</h1><p class="muted">${t("Questions are picked from memory gaps: life phases with few stories, or people mentioned only once.")}</p><div id="iv"><div class="empty">${t("Preparing a question...")}</div></div>`;
  const iv = $("#iv", el);
  const next = async () => {
    const q = await api(`/api/subjects/${S()}/interview/next`);
    if (!iv) return;
    iv.innerHTML = `<div class="card"><p class="small muted">${esc(q.reason)}</p><h2 style="margin-top:4px">${esc(q.question)}</h2>
      <textarea id="ans" placeholder="${t("Type your answer as it comes; mixing in local languages is fine.")}"></textarea>
      <div class="row" style="margin-top:8px"><button class="primary" id="send">${icon("check")} ${t("Save answer")}</button><button id="skip">${t("Another question")}</button></div></div>`;
    $("#send", iv).onclick = async () => {
      const ans = $("#ans", iv);
      if (!ans || !ans.value.trim()) return;
      await api(`/api/subjects/${S()}/interview/answer`, { method: "POST", json: { question: q.question, answer: ans.value } });
      toast(t("Answer saved and being processed"));
      next();
    };
    $("#skip", iv).onclick = next;
  };
  await next();
}

// Sesi kerja agent (hook engram) dan pelajaran proyek (konvensi/koreksi/jebakan).
async function viewSessions(el, projectArg) {
  const project = projectArg || P() || "";
  const q = project ? `?project=${encodeURIComponent(project)}` : "";
  const [sessions, lessons, eps] = await Promise.all([
    api(`/api/subjects/${S()}/sessions${q}`),
    api(`/api/subjects/${S()}/lessons${q}`),
    api(`/api/subjects/${S()}/episodes?limit=500`),
  ]);
  const obs = eps.filter((e) => e.obs_type && (!project || (e.project || "") === project)).sort((a, b) => (b.created_at || "").localeCompare(a.created_at || ""));
  const projects = [...new Set([...sessions.map((s) => s.project), ...obs.map((e) => e.project)].filter(Boolean))].sort();
  el.innerHTML = `<h1>${t("Sessions & lessons")}</h1>
    <div class="card row small"><label>${t("Project")} <select id="proj"><option value="">${t("all")}</option>${projects.map((p) => `<option value="${esc(p)}" ${p === project ? "selected" : ""}>${esc(p)}</option>`).join("")}</select></label>
      <span class="muted">${t("Recorded automatically by {hook} (Claude Code, Gemini CLI, Antigravity). The session start context is available at the {link}.", { hook: "<code>engram hook</code>", link: `<a href="/api/subjects/${S()}/context/inject${q}" target="_blank">${t("inject endpoint")}</a>` })}</span></div>
    <h2>${t("Project conventions, corrections & pitfalls")}</h2>
    ${lessons.length ? `<div class="card">${lessons.map(lessonRow).join("")}</div>` : `<p class="muted">${t("No lessons yet. Lessons come from user corrections and conventions mentioned during work sessions.")}</p>`}
    <h2>${t("Recent observations")}</h2>
    ${obs.length ? obsList(obs) : `<div class="card empty-state"><b>${t("No observations yet.")}</b><p class="small">${t("Install the hook with {cmd}, then open a new Claude Code session. Installation status is on the Projects tab.", { cmd: "<code>scripts/install-mcp.sh --client claude</code>" })}</p></div>`}
    <h2>${t("Sessions")}</h2>
    ${sessionTable(sessions)}`;
  $("#proj", el).onchange = (ev) => { location.hash = ev.target.value ? `#sesi/${encodeURIComponent(ev.target.value)}` : "#sesi"; };
  bindCites(el);
  el.querySelectorAll("[data-session]").forEach((tr) => { tr.onclick = () => showSession(tr.dataset.session); });
}

async function viewReview(el) {
  const [items, watched] = await Promise.all([
    api(`/api/subjects/${S()}/reviews`),
    api(`/api/subjects/${S()}/watched-folders`).catch(() => ({ agy_available: false })),
  ]);
  const kinds = { conflict: t("Conflicting facts"), reflective: t("Confirm principle/trait"), entity_merge: t("Same entity?"), quarantine: t("Doubtful facts"), gray_zone: t("Worth a story?") };
  const help = {
    gray_zone: t("Approve = promote to a story. Reject = let it decay."), conflict: t("Approve = the new fact replaces the old one. Keep both = both stay active. Reject = the new fact is discarded."),
    entity_merge: t("Approve = merge into one entity. Reject = different entities."), quarantine: t("Approve = the fact is used. Reject = discarded."), reflective: t("Approve = active. Reject = discarded.")
  };
  const sub = (r) => {
    if (r.kind !== "entity_merge") return "";
    const ek = r.payload?.entity_kind;
    return ek === "project" ? t("project") : ek === "component" ? t("component") : ek === "tool" ? t("tool") : ek === "person" ? t("person") : "";
  };
  // Dua fakta yang bertentangan ditampilkan berdampingan bila payload membawa keduanya.
  const sideBySide = (r) => {
    const a = r.payload?.fact_new || r.payload?.incoming || r.payload?.new, b = r.payload?.fact_old || r.payload?.old || r.payload?.existing;
    const renderSide = (x) => {
      if (!x) return "";
      if (typeof x === "string") return esc(x);
      const stmt = x.statement || [x.predicate, x.object_text].filter(Boolean).join(" ") || JSON.stringify(x);
      const time = x.time ? ` <span class="small muted">(${esc(x.time)})</span>` : "";
      const quote = x.quote ? `<div class="quote small" style="margin-top:4px">${esc(x.quote)}</div>` : "";
      return `<div>${esc(stmt)}${time}</div>${quote}`;
    };
    if (!a && !b) return r.payload?.text ? `<pre class="src">${esc(r.payload.text)}</pre>` : (r.payload?.quote ? `<div class="quote">${esc(r.payload.quote)}</div>` : "");
    return `<div class="versus"><div><div class="small muted">${t("New")}</div>${renderSide(a)}</div><div><div class="small muted">${t("Old")}</div>${renderSide(b)}</div></div>`;
  };
  const agyBadge = watched.agy_available
    ? `<span class="badge now" title="${esc(watched.agy_path || "")}">${t("agy CLI ready")}</span>`
    : `<span class="badge warn">${t("agy CLI not found")}</span>`;
  const groups = {};
  for (const r of items) (groups[r.kind] ||= []).push(r);
  const order = ["conflict", "reflective", "entity_merge", "quarantine", "gray_zone"].filter((k) => groups[k]);
  el.innerHTML = `<div class="row" style="justify-content:space-between;align-items:center">
      <h1 style="margin:0">${t("Review")} (<span id="rev-total">${items.length}</span>)</h1>
      <div class="row">
        ${agyBadge}
        <button class="primary" id="btn-auto-review" ${!items.length ? "disabled" : ""}>${icon("flag")} ${t("Auto review via agy")}</button>
      </div>
    </div>
    <p class="muted">${t("Engram uses the {agy} CLI automatically in the background, and when the button above is clicked, to resolve fact conflicts, entity merges, and quarantine.", { agy: "<code>agy</code>" })}</p>
    ${order.map((k) => `<section class="rev-group" data-kind="${k}"><h2>${esc(kinds[k] || k)} <span class="small muted">(${groups[k].length})</span></h2>
      <div class="row small"><label><input type="checkbox" data-all> ${t("select all")}</label><button data-bulk="approve">${icon("check")} ${t("Approve selected")}</button>${k === "conflict" ? `<button data-bulk="keep_both">${t("Keep both")}</button>` : ""}<button data-bulk="reject">${icon("x")} ${t("Reject selected")}</button><span class="muted">${esc(help[k] || "")}</span></div>
      ${groups[k].map((r) => `<div class="card rev" id="r-${esc(r.id)}"><label class="rev-pick"><input type="checkbox" data-pick="${esc(r.id)}"></label><div class="rev-body">
        ${sub(r) ? `<span class="badge">${esc(sub(r))}</span>` : ""}<p>${esc(r.summary)}</p>${sideBySide(r)}
        <div class="row"><input type="text" placeholder="${t("Note (optional)")}" data-note><button class="primary" data-d="approve" data-id="${esc(r.id)}">${icon("check")} ${t("Approve")}</button>${r.kind === "conflict" ? `<button data-d="keep_both" data-id="${esc(r.id)}">${t("Keep both")}</button>` : ""}<button data-d="reject" data-id="${esc(r.id)}">${icon("x")} ${t("Reject")}</button>${(r.targets || []).slice(0, 2).map((tg) => `<a class="cite" data-ep="${esc(tg)}">${t("evidence")}</a>`).join(" ")}</div></div></div>`).join("")}
    </section>`).join("") || `<div class="empty">${t("Nothing to review.")}</div>`}`;
  const btnAuto = $("#btn-auto-review", el);
  if (btnAuto) {
    btnAuto.onclick = async () => {
      btnAuto.disabled = true;
      const orig = btnAuto.innerHTML;
      btnAuto.textContent = t("agy is reviewing...");
      try {
        const res = await api(`/api/subjects/${S()}/reviews/auto`, { method: "POST" });
        toast(t("{n} reviews resolved automatically ({approved} approved, {both} kept both, {rejected} rejected)", { n: res.resolved, approved: res.approved, both: res.kept_both, rejected: res.rejected }));
        await route();
      } catch (e) {
        toast(e.message);
        btnAuto.disabled = false;
        btnAuto.innerHTML = orig;
      }
    };
  }
  const decide = async (id, d, note) => {
    await api(`/api/reviews/${encodeURIComponent(id)}`, { method: "POST", json: { decision: d, note: note || null } });
    $(`#r-${CSS.escape(id)}`, el)?.remove();
  };
  el.querySelectorAll("[data-d]").forEach((b) => {
    b.onclick = async () => {
      const card = b.closest(".card");
      b.disabled = true;
      try { await decide(b.dataset.id, b.dataset.d, card.querySelector("[data-note]").value); toast(t("Saved")); }
      catch (e) { toast(e.message); b.disabled = false; }
    };
  });
  el.querySelectorAll(".rev-group").forEach((g) => {
    g.querySelector("[data-all]").onchange = (ev) => g.querySelectorAll("[data-pick]").forEach((c) => { c.checked = ev.target.checked; });
    g.querySelectorAll("[data-bulk]").forEach((b) => {
      b.onclick = async () => {
        const ids = [...g.querySelectorAll("[data-pick]:checked")].map((c) => c.dataset.pick);
        if (!ids.length) { toast(t("Nothing selected yet")); return; }
        b.disabled = true;
        let ok = 0;
        for (const id of ids) { try { await decide(id, b.dataset.bulk); ok++; } catch (e) { toast(e.message); } }
        toast(t("{n} decisions saved", { n: ok }));
        b.disabled = false;
        if (!g.querySelector(".rev")) g.remove();
      };
    });
  });
  bindCites(el);
}

async function viewSources(el) {
  const [sources, batches, watched, storage] = await Promise.all([
    api(`/api/subjects/${S()}/sources?limit=300`),
    api(`/api/subjects/${S()}/batches`),
    api(`/api/subjects/${S()}/watched-folders`),
    api("/api/storage").catch(() => ({ can_change: false })),
  ]);
  const canPick = !!storage?.can_change;
  const agyBadge = watched.agy_available
    ? `<span class="badge now" title="${esc(watched.agy_path || "")}">${t("agy CLI ready")}</span>`
    : `<span class="badge conflict">${t("agy CLI not found")}</span>`;
  const watchMode = watched.poll_secs ? t("checked every {n} seconds", { n: watched.poll_secs }) : t("manual mode");
  el.innerHTML = `<h1>${t("Sources & import")}</h1>
    <h2>${icon("folder")} ${t("Important folders")} <span class="small">${agyBadge}</span></h2>
    <div class="card">
      <p class="small muted" style="margin-top:0">${t("Engram reads the entire contents of important folders (coding projects as well as documents/notes) using the <code>agy</code> CLI, stores their context as memory, and updates that memory automatically whenever files change ({mode}).", { mode: watchMode })}</p>
      <form id="wf-form" class="row" style="margin-bottom:8px">
        <input type="text" id="wf-path" placeholder="${t("Important folder path, e.g. /Users/name/Documents/main-project")}" style="flex:2" required>
        ${canPick ? `<button type="button" id="wf-pick">${icon("folder")} ${t("Choose folder")}</button>` : ""}
        <input type="text" id="wf-label" placeholder="${t("Name/label (optional)")}" style="flex:1">
        <button class="primary" type="submit" id="wf-add">${icon("plus")} ${t("Add &amp; read")}</button>
      </form>
      <div class="small" id="wf-msg"></div>
      <div id="wf-list">${watched.folders.length ? watched.folders.map(watchedFolderRow).join("") : `<p class="small muted" style="margin:6px 0 0">${t("No important folders are being watched yet.")}</p>`}</div>
    </div>
    <h2>${icon("upload")} ${t("Upload &amp; file inbox")}</h2>
    <div class="card"><div class="drop" id="drop">${icon("upload")} ${t("Drag files here or")} <label class="btn">${t("choose files")}<input type="file" id="files" multiple hidden></label>
      <p class="small">${t("Text/Markdown, WhatsApp export (.txt), Telegram (result.json), ChatGPT/Claude (conversations.json), email (.mbox/.eml), photo + caption (.jpg + .txt with the same name), audio (requires Whisper).")}</p>
      <div class="row" style="justify-content:center"><label>${t("Type for plain text")} <select id="kind"><option value="">${t("automatic")}</option><option value="interview">${t("interview")}</option><option value="memoir">${t("memoir")}</option><option value="note">${t("note")}</option><option value="lifelog">${t("lifelog")}</option></select></label></div></div>
      <p class="small muted">${t("You can also put files in the <code>vault/inbox/</code> folder (subfolder <code>inbox/&lt;subject-id&gt;/</code> for other subjects).")}</p></div>
    <h2>${t("Recent imports")}</h2><div id="batches">${batches.map(batchRow).join("") || `<p class="muted">${t("None yet.")}</p>`}</div>
    <h2>${t("Sources ({n})", { n: sources.length })}</h2>
    <table><tr><th>${t("Title")}</th><th>${t("Type")}</th><th>${t("Status")}</th><th>${t("Results")}</th></tr>${sources.map((s) => `<tr data-source="${esc(s.id)}" style="cursor:pointer"><td>${esc(s.title)}<div class="small muted">${esc(s.created_at.slice(0, 10))}${s.occurred_from ? " · " + t("occurred {date}", { date: esc(s.occurred_from) }) : ""}</div></td><td>${esc(s.kind)}</td><td><span class="badge ${s.status === "failed" ? "conflict" : s.status === "done" ? "now" : ""}">${esc(s.status)}</span></td><td class="small">${t("{episodes} stories, {wm} WM, {noise} noise", { episodes: s.stats.episodes, wm: s.stats.stream_items, noise: s.stats.dropped })}</td></tr>`).join("")}</table>`;
  bindCites(el);
  bindWatchedFolders(el);
  const drop = $("#drop", el);
  const upload = async (files) => {
    if (!files.length) return;
    const fd = new FormData();
    if ($("#kind", el).value) fd.append("kind", $("#kind", el).value);
    fd.append("label", `upload ${new Date().toLocaleString(LOCALE)}`);
    for (const f of files) fd.append("file", f, f.name);
    toast(t("Uploading {n} files...", { n: files.length }));
    try {
      const r = await api(`/api/subjects/${S()}/import`, { method: "POST", body: fd });
      toast(t("{queued} queued, {duplicates} duplicates, {failed} failed", { queued: r.queued, duplicates: r.duplicates, failed: r.failed.length }));
      pollBatch(r.batch_id);
    } catch (e) { toast(e.message); }
  };
  $("#files", el).onchange = (e) => upload([...e.target.files]);
  drop.ondragover = (e) => { e.preventDefault(); drop.classList.add("over"); };
  drop.ondragleave = () => drop.classList.remove("over");
  drop.ondrop = (e) => { e.preventDefault(); drop.classList.remove("over"); upload([...e.dataTransfer.files]); };
}

function watchedFolderRow(f) {
  const statusBadge = f.last_error
    ? `<span class="badge conflict">${t("error")}</span>`
    : f.last_change_at
      ? `<span class="badge now">${t("watched ({n} files)", { n: f.file_count })}</span>`
      : `<span class="badge warn">${t("not read yet")}</span>`;
  return `<div class="fact" id="wf-${esc(f.id)}" data-wf="${esc(f.id)}">
    <div class="row" style="justify-content:space-between">
      <div>
        <b>${esc(f.label)}</b> ${statusBadge}
        <div class="small muted"><code>${esc(f.path)}</code></div>
        <div class="small muted">${t("Checked: {scan} &middot; Context updated: {change}", { scan: esc(fmtTime(f.last_scan_at)), change: esc(fmtTime(f.last_change_at)) })}${f.last_source_id ? ` &middot; <a class="cite" data-source="${esc(f.last_source_id)}">${t("view source")}</a>` : ""}</div>
        ${f.last_error ? `<div class="small" style="color:var(--bad)">${esc(f.last_error)}</div>` : ""}
      </div>
      <div class="row">
        <button type="button" data-wf-scan="${esc(f.id)}" title="${t("Scan for changes or re-read this folder's context via agy CLI")}">${icon("refresh")} ${t("Re-read")}</button>
        <button type="button" class="ghost" data-wf-del="${esc(f.id)}" title="${t("Remove from important folders")}">${icon("x")}</button>
      </div>
    </div>
  </div>`;
}

function bindWatchedFolders(el) {
  const pick = $("#wf-pick", el);
  if (pick) {
    pick.onclick = async () => {
      try {
        const p = await pickFolder(t("Choose an important folder for engram to watch"));
        if (p) $("#wf-path", el).value = p;
      } catch (e) { toast(e.message); }
    };
  }
  const runScan = async (id, force, btn) => {
    const msg = $("#wf-msg", el);
    if (btn) btn.disabled = true;
    if (msg) msg.innerHTML = `<span class="muted">${t("Reading the folder and extracting context via <code>agy</code> CLI...")}</span>`;
    try {
      const r = await api(`/api/watched-folders/${encodeURIComponent(id)}/scan`, { method: "POST", json: { force } });
      if (r.error) toast(t("Failed: {error}", { error: r.error }));
      else if (r.changed) toast(t("Context of \"{label}\" updated ({files} files) and added to the memory queue", { label: r.label, files: r.files }));
      else toast(t("Folder \"{label}\" has not changed ({files} files)", { label: r.label, files: r.files }));
      if (location.hash.startsWith("#sumber")) await route();
    } catch (e) {
      if (msg) msg.innerHTML = errLine(e.message);
      if (btn) btn.disabled = false;
    }
  };
  const form = $("#wf-form", el);
  if (form) {
    form.onsubmit = async (e) => {
      e.preventDefault();
      const path = $("#wf-path", el).value.trim();
      const label = $("#wf-label", el).value.trim() || null;
      if (!path) return;
      const addBtn = $("#wf-add", el);
      addBtn.disabled = true;
      try {
        const r = await api(`/api/subjects/${S()}/watched-folders`, { method: "POST", json: { path, label, scan: false } });
        toast(t("Folder \"{label}\" added; reading context via agy CLI...", { label: r.folder.label }));
        await route();
        await runScan(r.folder.id, true, null);
      } catch (err) {
        $("#wf-msg", el).innerHTML = errLine(err.message);
        addBtn.disabled = false;
      }
    };
  }
  el.querySelectorAll("[data-wf-scan]").forEach((b) => {
    b.onclick = () => runScan(b.dataset.wfScan, true, b);
  });
  el.querySelectorAll("[data-wf-del]").forEach((b) => {
    b.onclick = async () => {
      b.disabled = true;
      try {
        await api(`/api/watched-folders/${encodeURIComponent(b.dataset.wfDel)}/remove`, { method: "POST" });
        toast(t("Important folder is no longer watched"));
        await route();
      } catch (e) { toast(e.message); b.disabled = false; }
    };
  });
}

function batchRow(b) {
  const pct = b.total ? Math.round(((b.done + b.failed) / b.total) * 100) : 100;
  return `<div class="card" id="b-${esc(b.id)}"><div class="row" style="justify-content:space-between"><b>${esc(b.label)}</b><span class="small muted">${t("{done}/{total} done · {failed} failed · {duplicates} duplicates", { done: b.done, total: b.total, failed: b.failed, duplicates: b.duplicates })}</span></div>
    <div class="progress"><span style="width:${pct}%"></span></div>${(b.failures || []).slice(0, 5).map((f) => `<div class="small" style="color:var(--bad)">${esc(f)}</div>`).join("")}</div>`;
}

async function pollBatch(id) {
  for (let i = 0; i < 600; i++) {
    const r = await api(`/api/batches/${encodeURIComponent(id)}`);
    const el = document.getElementById("b-" + id);
    if (el) el.outerHTML = batchRow(r.batch);
    else if (location.hash.startsWith("#sumber") && $("#batches")) $("#batches").insertAdjacentHTML("afterbegin", batchRow(r.batch));
    if (r.batch.done + r.batch.failed >= r.batch.total) { toast(t("Import finished processing")); return; }
    await new Promise((res) => setTimeout(res, 3000));
  }
}

// ---------------------------------------------------------------- boot
$("#subject-select").onchange = async (e) => { state.subject = e.target.value; localStorage.setItem("subject", state.subject); state.project = ""; localStorage.removeItem("project"); await loadProjects(); route(); };
$("#new-subject").onclick = () => newSubjectForm();
$("#start-services-btn").onclick = () => startAllServices($("#start-services-btn"));
$("#settings-btn").onclick = () => showSettings();

// Status layanan latar (server, worker, scheduler, inbox). Hanya ada di aplikasi desktop.
const svc = { list: [] };
async function startAllServices(triggerBtn) {
  const topBtn = $("#start-services-btn");
  const btns = [topBtn, triggerBtn].filter(Boolean);
  btns.forEach((b) => { b.disabled = true; });
  const lbl = $("#start-services-label");
  const origLabel = lbl ? lbl.textContent : "";
  if (lbl) lbl.textContent = t("Starting...");
  try {
    const r = await api("/api/services/start-all", { method: "POST" });
    if (Array.isArray(r.services)) svc.list = r.services;
    updateGearDot();
    if (!$("#drawer").hidden && $("#svc-list")) renderServices();
    const cur = (location.hash.slice(1).split("/")[0]) || DEFAULT_ROUTE();
    if (cur === "proyek" || cur === "ringkasan") await route();
    const extStopped = (r.services || []).filter((x) => x.external && !x.running);
    const parts = [];
    if (r.started?.length) parts.push(t("{n} services started", { n: r.started.length }));
    else parts.push(t("All services are running"));
    if (r.retried_sources) parts.push(t("{n} sources requeued", { n: r.retried_sources }));
    if (r.requeued_stale) parts.push(t("{n} stuck jobs retried", { n: r.requeued_stale }));
    if (extStopped.length) parts.push(t("Warning: {names} not responding yet", { names: extStopped.map((x) => x.name).join(", ") }));
    toast(parts.join(" · "));
  } catch (e) {
    toast(t("Failed to start services: {error}", { error: e.message }));
  } finally {
    if (lbl && origLabel) lbl.textContent = origLabel;
    btns.forEach((b) => { b.disabled = false; });
  }
}
async function pollServices() {
  try {
    const r = await api("/api/services");
    if (!r.enabled) { svc.list = []; updateGearDot(); return; }
    svc.list = r.services;
    if (r.update_progress) applyUpdateProgress(r.update_progress);
  } catch (e) {
    // Server tidak menjawab: tandai semua layanan tidak diketahui.
    svc.list = svc.list.map((s) => Object.assign({}, s, { running: false }));
  }
  updateGearDot();
  if (!$("#drawer").hidden && $("#svc-list")) renderServices();
}
// Titik di tombol Pengaturan & Start semua service: merah bila ada layanan berhenti, hijau bila berjalan/ada update.
function updateGearDot() {
  const stopped = svc.list.filter((x) => !x.running);
  const upd = settings.update?.available;
  const dot = $("#settings-dot"), btn = $("#settings-btn");
  dot.hidden = !stopped.length && !upd;
  dot.className = "dot " + (stopped.length ? "bad" : "ok");
  const names = stopped.map((x) => x.name).join(", ");
  const lines = [t("Settings")];
  if (stopped.length) lines.push(t("{n} services stopped: {names}", { n: stopped.length, names }));
  if (upd) lines.push(t("Version {v} available", { v: settings.update.latest }));
  btn.title = lines.join("\n");
  const svcDot = $("#services-dot"), svcBtn = $("#start-services-btn");
  if (svcDot && svcBtn) {
    svcDot.hidden = svc.list.length === 0;
    svcDot.className = "dot " + (stopped.length ? "bad" : "ok");
    svcBtn.title = stopped.length
      ? t("{n} services stopped ({names}) — click to start all services", { n: stopped.length, names })
      : t("All services running — click to make sure all services are active & reprocess the queue");
  }
}
function tabServices() {
  $("#st-body").innerHTML = `<div class="row" style="justify-content:space-between;margin-bottom:10px"><p class="muted small" style="margin:0">${t("engram's background services in this app. Stopped services can be started again from here, the button at the top right, or the menu bar icon.")}</p><button class="primary" id="svc-start-all">${icon("play")} ${t("Start all services")}</button></div><div id="svc-list"></div>`;
  const allBtn = $("#svc-start-all");
  if (allBtn) allBtn.onclick = () => startAllServices(allBtn);
  renderServices();
  pollServices();
}
function renderServices() {
  $("#svc-list").innerHTML = svc.list.map((s) => `<div class="card" data-svc="${esc(s.id)}">
      <div class="row" style="justify-content:space-between">
        <div class="row"><span class="dot ${s.running ? "ok" : "bad"}"></span><b>${esc(s.name)}</b>
          <span class="badge ${s.running ? "now" : "conflict"}">${s.running ? t("Running") : t("Stopped")}</span></div>
        ${s.running || s.external ? "" : `<button class="primary" data-start>${icon("play")} ${t("Start")}</button>`}
      </div>
      <div class="small muted">${esc(s.description)}</div>
      ${s.error ? `<div class="small" style="color:var(--bad)">${esc(s.error)}</div>` : ""}
      <div class="small msg"></div>
    </div>`).join("");
  $("#svc-list").querySelectorAll("[data-start]").forEach((b) => {
    b.onclick = async () => {
      const card = b.closest("[data-svc]");
      b.disabled = true;
      try {
        const r = await api(`/api/services/${encodeURIComponent(card.dataset.svc)}/start`, { method: "POST" });
        svc.list = r.services;
        updateGearDot();
        renderServices();
        toast(t("Service started"));
      } catch (e) {
        card.querySelector(".msg").innerHTML = `<span style="color:var(--bad)">${esc(e.message)}</span>`;
        b.disabled = false;
      }
    };
  });
}

// Pasang server MCP ke agent AI (Antigravity, VS Code, Claude Code, Cursor, Codex, Windsurf) dengan satu klik.
// Client koding murni memakai profil tool `coding` (lihat install.rs).
const MCP_STATE = {
  installed: ["now", t("Installed")],
  outdated: ["warn", t("Needs update")],
  missing: ["", t("Not installed")],
  unreadable: ["conflict", t("Config unreadable")],
};
async function connectAgents() {
  $("#st-body").innerHTML = `<div class="empty">${t("Loading...")}</div>`;
  let r;
  try { r = await api("/api/mcp/clients"); }
  catch (e) { $("#st-body").innerHTML = `<div class="card">${t("Failed to load: {error}", { error: esc(e.message) })}</div>`; return; }
  if (!r.enabled) {
    $("#st-body").innerHTML = `<div class="card"><p>${t("Automatic installation is only available in the desktop app or with <code>engram serve</code> on 127.0.0.1.")}</p>
      <p class="small muted">${t("Use <code>scripts/install-mcp.sh</code> for other servers.")}</p></div>`;
    return;
  }
  $("#st-body").innerHTML = `
    <p class="muted small">${t("Register this memory as the MCP server <b>{name}</b> so AI agents can remember and search stories.", { name: esc(r.name) })}
      ${t("Address: <code>{url}</code>", { url: esc(r.url) })}</p>
    ${r.clients.map((c) => {
    const [cls, label] = MCP_STATE[c.state] || ["", c.state];
    const has = c.state === "installed" || c.state === "outdated";
    return `<div class="card" data-client="${esc(c.id)}">
        <div class="row" style="justify-content:space-between">
          <div><b>${esc(c.label)}</b> <span class="badge ${cls}">${esc(label)}</span>${c.profile === "coding" ? ` <span class="badge" title="${t("Coding-session tools only: project_context, recall, remember, who_is, timeline, principles")}">${t("coding profile")}</span>` : ""}${c.detected ? "" : ` <span class="small muted">${t("(not detected)")}</span>`}</div>
          <div class="row">
            ${has ? `<button data-act="uninstall" class="ghost" title="${t("Remove from {client}", { client: esc(c.label) })}">${icon("x")}</button>` : ""}
            <button data-act="install" class="${c.state === "installed" ? "" : "primary"}">${icon(c.state === "installed" ? "refresh" : "plug")} ${c.state === "installed" ? t("Reinstall") : c.state === "outdated" ? t("Update") : t("Install")}</button>
          </div>
        </div>
        <div class="small muted">${esc(c.config_path)}</div>
        <div class="small msg"></div>
      </div>`;
  }).join("")}
    <p class="small muted">${t("The access token is written to that config too. A backup of the old file is saved as <code>.bak</code>.")}</p>`;
  $("#st-body").querySelectorAll("[data-act]").forEach((b) => {
    b.onclick = async () => {
      const card = b.closest("[data-client]");
      card.querySelectorAll("button").forEach((x) => { x.disabled = true; });
      try {
        const res = await api(`/api/mcp/clients/${encodeURIComponent(card.dataset.client)}/${b.dataset.act}`, { method: "POST" });
        toast(b.dataset.act === "install" ? t("MCP installed") : t("MCP removed"));
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
const fmtTime = (ts) => (ts ? new Date(ts).toLocaleString(LOCALE) : t("never"));
const errLine = (m) => `<span style="color:var(--bad)">${esc(m)}</span>`;

async function pickFolder(title) {
  const r = await api("/api/pick-folder", { method: "POST", json: { title } });
  return r.path || "";
}

async function showDataSync() {
  $("#st-body").innerHTML = `<div class="empty">${t("Loading...")}</div>`;
  let storage, sync;
  try { [storage, sync] = await Promise.all([api("/api/storage"), api("/api/sync")]); }
  catch (e) { $("#st-body").innerHTML = `<div class="card">${t("Failed to load: {error}", { error: esc(e.message) })}</div>`; return; }
  const can = storage.can_change;
  const pickBtn = (id) => (can ? `<button type="button" id="${id}">${icon("folder")} ${t("Choose folder")}</button>` : "");
  const intervals = [[0, t("Manual only")], [5, t("Every 5 minutes")], [10, t("Every 10 minutes")], [30, t("Every 30 minutes")], [60, t("Every hour")]];
  const cur = sync.settings.interval_minutes;
  if (!intervals.some(([m]) => m === cur)) intervals.push([cur, t("Every {n} minutes", { n: cur })]);
  $("#st-body").innerHTML = `
    <h2 style="margin-top:0">${icon("folder")} ${t("Data location")}</h2>
    <div class="card">
      <div class="small muted">${t("The database and source files are stored in:")}</div>
      <p><code>${esc(storage.vault)}</code></p>
      ${storage.in_cloud_folder ? `<p class="small" style="color:var(--warn)">${icon("alert")} ${t("This folder is synced by a cloud service. The database can get corrupted; move it to a regular folder and use Sync below.")}</p>` : ""}
      ${can ? `
        <div class="row"><input type="text" id="mv-path" placeholder="${t("New folder, e.g. /Users/name/Documents/engram")}">${pickBtn("mv-pick")}</div>
        <p class="small muted">${t("Data is copied to the new folder, then the app restarts. The old folder is not deleted. If the folder contains other files, an <code>engram-vault</code> subfolder is created. If the folder already contains engram data, that data is used.")}</p>
        <div id="mv-confirm"></div>
        <button class="primary" id="mv-go">${icon("upload")} ${t("Move data")}</button>
        <div class="small msg" id="mv-msg"></div>`
      : `<p class="small muted">${t("To move it, stop the server and run <code>engram vault move &lt;folder&gt;</code>.")}</p>`}
    </div>

    <h2>${icon("cloud")} ${t("Google Drive sync")}</h2>
    <p class="small muted">${t("Install <b>Google Drive for desktop</b> on every computer, then choose the <b>same</b> folder in My Drive and enter the <b>same</b> password.")}
      ${t("Each computer stores an encrypted copy in that folder and merges data from the other computers. The database stays in the data location above.")}</p>
    <form id="sync-form" class="card">
      <p><label>${t("Sync folder in Google Drive")}<br>
        <span class="row"><input type="text" id="sy-dir" value="${esc(sync.settings.dir || "")}" placeholder="${t("e.g. .../Google Drive/My Drive/engram-sync")}">${pickBtn("sy-pick")}</span></label></p>
      ${sync.suggestions.length ? `<p class="small muted">${t("Detected:")} ${sync.suggestions.map((p) => `<button type="button" class="ghost small" data-suggest="${esc(p)}">${icon("folder")} ${esc(p)}</button>`).join(" ")}</p>` : ""}
      ${sync.configured && !sync.dir_ok ? `<p class="small" style="color:var(--bad)">${icon("alert")} ${t("The folder cannot be accessed. Make sure Google Drive is running.")}</p>` : ""}
      <p><label>${t("Encryption password (same on all computers)")}<br>
        <input type="password" id="sy-pass" autocomplete="new-password" style="width:100%" placeholder="${sync.encrypted ? t("saved - leave empty to keep it") : t("at least 8 characters")}"></label></p>
      ${sync.encrypted ? "" : `<p class="small" style="color:var(--warn)">${icon("alert")} ${t("Without a password, memory contents are stored as-is in Google Drive.")}</p>`}
      <div class="row">
        <label>${t("Schedule")}<br><select id="sy-int">${intervals.map(([m, l]) => `<option value="${m}" ${m === cur ? "selected" : ""}>${esc(l)}</option>`).join("")}</select></label>
        <label style="flex:1">${t("This computer's name")}<br><input type="text" id="sy-name" value="${esc(sync.settings.device_name || "")}" placeholder="${esc(sync.device_name)}" style="width:100%"></label>
      </div>
      <p class="small msg" id="sy-msg"></p>
      <div class="row">
        <button class="primary" type="submit">${icon("check")} ${t("Save")}</button>
        ${sync.configured ? `<button type="button" id="sy-run">${icon("refresh")} ${t("Sync now")}</button><button type="button" id="sy-off" class="ghost">${icon("x")} ${t("Turn off sync")}</button>` : ""}
      </div>
    </form>
    ${sync.configured ? `<div class="card">
      <div class="small">${t("Last sync: <b>{time}</b>", { time: esc(fmtTime(sync.last_sync_at)) })}${sync.pending_embed ? ` &middot; ${t("{n} records waiting for the embedding server", { n: sync.pending_embed })}` : ""}</div>
      ${sync.last_error ? `<p class="small">${errLine(t("Error: {error}", { error: sync.last_error }))}</p>` : ""}
      ${sync.last_warnings.map((w) => `<div class="small" style="color:var(--warn)">${icon("alert")} ${esc(w)}</div>`).join("")}
      ${sync.devices.length ? `<table><tr><th>${t("Computer")}</th><th>${t("Records")}</th><th>${t("Updated")}</th></tr>
        ${sync.devices.map((d) => `<tr><td>${esc(d.name)}${d.this_device ? ` <span class="badge now">${t("this one")}</span>` : ""}${d.encrypted ? "" : ` <span class="badge warn">${t("unencrypted")}</span>`}</td><td>${d.rows}</td><td class="small">${esc(fmtTime(d.updated_at))}</td></tr>`).join("")}</table>`
        : `<p class="small muted">${t("No data in the sync folder yet. Click Sync now.")}</p>`}
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
  bindPick("mv-pick", "mv-path", t("Choose a new folder for engram data"));
  bindPick("sy-pick", "sy-dir", t("Choose a folder in Google Drive"));
  $("#st-body").querySelectorAll("[data-suggest]").forEach((b) => { b.onclick = () => { $("#sy-dir").value = b.dataset.suggest; }; });

  // Pindah lokasi data: konfirmasi di halaman (WebView tidak menampilkan confirm()).
  if ($("#mv-go")) {
    $("#mv-go").onclick = () => {
      const path = $("#mv-path").value.trim();
      if (!path) { msg("#mv-msg", errLine(t("Choose or type a destination folder."))); return; }
      $("#mv-confirm").innerHTML = `<div class="card" style="background:var(--accent-soft)">
        <p class="small">${t("Data will be copied to <code>{path}</code> and then the app restarts. Continue?", { path: esc(path) })}</p>
        <div class="row"><button class="primary" id="mv-yes">${icon("check")} ${t("Yes, move it")}</button><button id="mv-no">${icon("x")} ${t("Cancel")}</button></div></div>`;
      $("#mv-no").onclick = () => { $("#mv-confirm").innerHTML = ""; };
      $("#mv-yes").onclick = async () => {
        $("#mv-yes").disabled = true;
        try {
          const r = await api("/api/storage/move", { method: "POST", json: { path } });
          $("#mv-confirm").innerHTML = "";
          msg("#mv-msg", t("{message}. Please wait...", { message: esc(r.message) }));
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
    if (!dir) { msg("#sy-msg", errLine(t("Choose a sync folder in Google Drive."))); return; }
    const body = { dir, interval_minutes: Number($("#sy-int").value), device_name: $("#sy-name").value.trim() };
    const pass = $("#sy-pass").value;
    if (pass) body.passphrase = pass;
    saveSync(body, t("Sync settings saved"));
  };
  if ($("#sy-off")) $("#sy-off").onclick = () => saveSync({ dir: null }, t("Sync turned off"));
  if ($("#sy-run")) {
    $("#sy-run").onclick = async () => {
      const b = $("#sy-run");
      b.disabled = true;
      msg("#sy-msg", t("Syncing..."));
      try {
        const r = await api("/api/sync/run", { method: "POST" });
        const from = r.pulled_from.length ? t("Data from {devices}: {added} new, {updated} updated.", { devices: r.pulled_from.join(", "), added: r.added, updated: r.updated }) + " " : "";
        toast(from + (r.pushed ? t("This computer's data was uploaded.") : t("No changes.")));
        await showDataSync();
      } catch (e) { msg("#sy-msg", errLine(e.message)); b.disabled = false; }
    };
  }
}

// ---------------------------------------------------------------- pengaturan
// Tab: Umum, Model AI, Pemrosesan, Tentang & update. Nilai model/pemrosesan
// disimpan ke memory.toml dan berlaku setelah aplikasi dibuka ulang.
// Tab Layanan hanya di aplikasi desktop (svc.list kosong di `engram serve`).
// [kunci, label, ikon, perlu perhatian]
const settingsTabs = () => [["umum", t("General"), "settings"], ["model", t("AI models"), "cpu"], ["proses", t("Processing"), "sliders"],
["data", t("Data & Sync"), "database"], ["agent", t("AI agents"), "plug"],
...(svc.list.length ? [["layanan", t("Services"), "server", svc.list.some((x) => !x.running)]] : []), ["tentang", t("About & updates"), "info"]];
const settings = { tab: "umum", about: null, data: null, update: null, updateProgress: null, models: [] };

// [bagian, kunci, label, jenis, petunjuk, pilihan]
const F_LLM = [
  ["llm", "base_url", t("LLM server URL"), "text", t("OpenAI-compatible endpoint, e.g. http://localhost:5211/v1 or https://api.openai.com/v1")],
  ["llm", "extract_model", t("Main model"), "model", t("Story extraction, distillation, and answers.")],
  ["llm", "light_model", t("Light model"), "model", t("Segmentation and classification; may be the same as the main model.")],
  ["llm", "light_base_url", t("Light model server URL"), "text", t("Leave empty if it is the same as the LLM server.")],
  ["llm", "api_key", t("API key"), "secret", t("Leave empty for a local server without a key.")],
];
const F_LLM_ADV = [
  ["llm", "api_key_env", t("API key env var"), "text", t("Takes precedence when set, e.g. OPENAI_API_KEY.")],
  ["llm", "json_mode", t("JSON mode"), "select", t("json_schema for vLLM; json_object for OpenAI/Ollama; none if unsupported."), [["json_schema", "json_schema (vLLM)"], ["json_object", "json_object"], ["none", "none"]]],
  ["llm", "disable_thinking", t("Disable thinking mode (Qwen3)"), "bool"],
  ["llm", "temperature", t("Temperature"), "float"],
  ["llm", "max_tokens", t("Max answer tokens"), "int"],
  ["llm", "timeout_secs", t("Timeout (seconds)"), "int"],
  ["llm", "max_retries", t("Retries on failure"), "int"],
];
const F_EMB = [
  ["embedding", "provider", t("Provider"), "select", "", [["openai", t("Embedding server (OpenAI-compatible)")], ["hash", t("Hash - no server (testing only)")]]],
  ["embedding", "base_url", t("Embedding server URL"), "text", t("e.g. http://localhost:8081/v1 (TEI)")],
  ["embedding", "model", t("Embedding model"), "text"],
  ["embedding", "dimension", t("Vector dimension"), "int", t("Changed model/dimension? Run `engram reembed` after restarting.")],
  ["embedding", "api_key", t("API key"), "secret"],
];
const F_EMB_ADV = [
  ["embedding", "api_key_env", t("API key env var"), "text"],
  ["embedding", "batch_size", t("Batch size"), "int"],
];
const F_TR = [
  ["transcription", "base_url", t("Whisper server URL"), "text", t("For audio import. Leave empty if unused.")],
  ["transcription", "model", t("Model"), "text"],
  ["transcription", "language", t("Language"), "text", t("Language code, e.g. en. Leave empty for auto-detection.")],
];
const F_PROC = [
  ["pipeline", "workers", t("Parallel workers"), "int", t("Number of jobs processed at once. Increase if the LLM server is powerful.")],
  ["schedule", "inbox_poll_secs", t("Check inbox folder every (seconds)"), "int"],
  ["schedule", "watch_poll_secs", t("Check watched folders for changes every (seconds)"), "int", t("0 = manual only. Changes in watched folders are re-read using the agy CLI.")],
  ["schedule", "retry_failed_minutes", t("Reprocess failed sources every (minutes)"), "int", t("0 = off. Useful if the LLM server is sometimes down.")],
  ["schedule", "retry_failed_max", t("Max automatic retries per source"), "int"],
  ["score", "stream_ttl_days", t("Keep casual chat for (days)"), "int", t("Working memory decays after this; the original source files remain.")],
  ["retrieval", "top_k", t("Stories per search"), "int", t("Used by search and AI agents (MCP).")],
  ["server", "max_upload_mb", t("Upload size limit (MB)"), "int"],
];

function getTheme() { try { return localStorage.getItem("theme") || "system"; } catch (e) { return "system"; } }
function setTheme(tm) {
  try { if (tm === "system") localStorage.removeItem("theme"); else localStorage.setItem("theme", tm); } catch (e) { }
  if (tm === "system") delete document.documentElement.dataset.theme; else document.documentElement.dataset.theme = tm;
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
  openDrawer(t("Settings"), `<div class="empty">${t("Loading...")}</div>`, true);
  try { [settings.about, settings.data] = await Promise.all([api("/api/about"), api("/api/settings")]); }
  catch (e) { $("#drawer-body").innerHTML = `<div class="card">${t("Failed to load: {error}", { error: esc(e.message) })}</div>`; return; }
  renderSettings();
}

function renderSettings() {
  const d = settings.data;
  if (!settingsTabs().some(([k]) => k === settings.tab)) settings.tab = "umum";
  const cur = settingsTabs().find(([k]) => k === settings.tab);
  $("#drawer-body").innerHTML = `<div class="st-layout">
    <nav class="st-nav" role="tablist" aria-orientation="vertical">${settingsTabs().map(([k, l, ic, warn]) => `<button type="button" role="tab" data-tab="${k}" class="${k === settings.tab ? "active" : ""}" aria-selected="${k === settings.tab}">${icon(ic)}<span>${esc(l)}</span>${warn ? `<span class="dot bad" title="${t("A service has stopped")}"></span>` : ""}</button>`).join("")}</nav>
    <section class="st-main">
    <h2 class="st-title">${esc(cur[1])}</h2>
    ${d.restart_required ? `<div class="notice small">${icon("alert")} ${t("New settings are saved but not in use yet.")}
      ${settings.about.can_restart ? `<div class="row" style="margin-top:6px"><button class="primary" id="st-restart">${icon("refresh")} ${t("Restart now")}</button></div>` : " " + t("Restart <code>engram serve</code> to apply them.")}</div>` : ""}
    ${!d.editable ? `<div class="notice small">${t("The location of <code>memory.toml</code> is unknown; settings are read-only.")}</div>` : ""}
    <div id="st-body"></div></section></div>`;
  $("#drawer-body").querySelectorAll("[data-tab]").forEach((b) => { b.onclick = () => { settings.tab = b.dataset.tab; renderSettings(); }; });
  if ($("#st-restart")) $("#st-restart").onclick = restartApp;
  $("#drawer-body .st-nav .active")?.scrollIntoView({ block: "nearest", inline: "nearest" });
  ({ umum: tabGeneral, model: tabModels, proses: tabProcessing, data: showDataSync, agent: connectAgents, layanan: tabServices, tentang: tabAbout })[settings.tab]();
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
    input = `<div class="row"><input type="password" ${attrs} autocomplete="new-password" style="flex:1" placeholder="${set ? t("saved - leave empty to keep") : t("not set")}">
      ${set && !dis ? `<button type="button" class="ghost small" data-clear="${id}">${icon("x")} ${t("Remove")}</button>` : ""}</div>`;
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
      el.placeholder = t("will be removed on save");
      b.remove();
    };
  });
  form.onsubmit = async (e) => {
    e.preventDefault();
    const changes = collectChanges(form);
    if (!Object.keys(changes).length) { toast(t("No changes")); return; }
    const btn = form.querySelector("[type=submit]");
    btn.disabled = true;
    try {
      settings.data = await api("/api/settings", { method: "PUT", json: { values: changes } });
      toast(t("Settings saved"));
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
    <h2 style="margin-top:0">${t("Appearance")}</h2>
    <div class="card">
      <label class="field"><span>${t("Theme")}</span><select id="st-theme">
        ${[["system", t("Follow system")], ["light", t("Light")], ["dark", t("Dark")]].map(([k, l]) => `<option value="${k}" ${k === theme ? "selected" : ""}>${l}</option>`).join("")}
      </select></label>
      <label class="field"><span>${t("Language")}</span><select id="st-lang">${LANGS.map(([k, l]) => `<option value="${k}" ${k === LANG ? "selected" : ""}>${esc(l)}</option>`).join("")}</select></label>
    </div>
    <h2>${t("Subject")}</h2>
    <form id="st-general" class="card">
      <label class="field"><span>${t("Default subject")}</span>
        <select data-sec="vault" data-key="default_subject" data-type="text" ${settings.data.editable ? "" : "disabled"}>
          ${known ? "" : `<option value="${esc(subj)}" selected>${esc(subj)}</option>`}
          ${state.subjects.map((s) => `<option value="${esc(s.id)}" ${s.id === subj ? "selected" : ""}>${esc(s.name)}</option>`).join("")}
        </select>
        <div class="hint">${t("Used by the inbox watcher, CLI, and AI agents when no subject is given.")}</div></label>
      <p class="small msg"></p>
      ${settings.data.editable ? `<button class="primary" type="submit">${icon("check")} ${t("Save")}</button>` : ""}
    </form>
    ${a.can_open ? `<h2>${t("Folders")}</h2>
    <div class="card row">
      <button type="button" data-open="vault">${icon("folder")} ${t("Data folder")}</button>
      <button type="button" data-open="inbox">${icon("inbox")} ${t("Inbox folder")}</button>
      ${a.config_path ? `<button type="button" data-open="config">${icon("folder")} ${t("Config folder")}</button>` : ""}
    </div>` : ""}`;
  $("#st-theme").onchange = (e) => setTheme(e.target.value);
  $("#st-lang").onchange = (e) => setLang(e.target.value);
  bindForm($("#st-general"));
  $("#st-body").querySelectorAll("[data-open]").forEach((b) => { b.onclick = () => openTarget(b.dataset.open); });
}

function tabModels() {
  const ed = settings.data.editable;
  const testBtn = (kind, label) => `<div class="row"><button type="button" data-test="${kind}">${icon("refresh")} ${label}</button></div><div class="small" data-test-msg="${kind}"></div>`;
  $("#st-body").innerHTML = `
    <form id="st-models-form">
      <datalist id="st-models">${settings.models.map((m) => `<option value="${esc(m)}">`).join("")}</datalist>
      <h2 style="margin-top:0">${t("Language model (LLM)")}</h2>
      <div class="card">
        ${F_LLM.map(fieldHtml).join("")}
        <details><summary>${t("Advanced")}</summary><div class="cols">${F_LLM_ADV.map(fieldHtml).join("")}</div></details>
        ${testBtn("llm", t("Test LLM connection"))}
      </div>
      <h2>${t("Embedding (semantic search)")}</h2>
      <div class="card">
        ${F_EMB.map(fieldHtml).join("")}
        <details><summary>${t("Advanced")}</summary><div class="cols">${F_EMB_ADV.map(fieldHtml).join("")}</div></details>
        ${testBtn("embedding", t("Test embedding connection"))}
      </div>
      <h2>${t("Audio transcription")}</h2>
      <div class="card">
        ${F_TR.map(fieldHtml).join("")}
        ${testBtn("transcription", t("Test transcription connection"))}
      </div>
      <p class="small msg"></p>
      ${ed ? `<div class="row"><button class="primary" type="submit">${icon("check")} ${t("Save")}</button><span class="small muted">${t("Takes effect after the app restarts.")}</span></div>` : ""}
    </form>`;
  const form = $("#st-models-form");
  bindForm(form);
  form.querySelectorAll("[data-test]").forEach((b) => {
    b.onclick = async () => {
      const kind = b.dataset.test, out = form.querySelector(`[data-test-msg="${kind}"]`);
      b.disabled = true;
      out.textContent = t("Testing...");
      try {
        // Uji nilai di formulir (belum disimpan).
        const r = await api("/api/settings/test", { method: "POST", json: { kind, values: collectChanges(form) } });
        out.innerHTML = `<span class="${r.ok ? "ok-text" : "bad-text"}">${icon(r.ok ? "check" : "alert")} ${esc(r.message)}</span>`;
        if (r.models?.length) {
          settings.models = r.models;
          $("#st-models").innerHTML = r.models.map((m) => `<option value="${esc(m)}">`).join("");
          out.innerHTML += `<div class="hint">${t("Models on server: {models}", { models: r.models.map(esc).join(", ") })}</div>`;
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
      ${settings.data.editable ? `<div class="row"><button class="primary" type="submit">${icon("check")} ${t("Save")}</button><span class="small muted">${t("Takes effect after the app restarts.")}</span></div>` : ""}
    </form>`;
  bindForm($("#st-proc-form"));
}

function tabAbout() {
  const a = settings.about;
  const host = a.host === "desktop" ? t("Desktop app") : t("Server (engram serve)");
  $("#st-body").innerHTML = `
    <div class="card">
      <h3 style="font-size:20px;margin:0">engram <span class="badge">v${esc(a.version)}</span></h3>
      <p class="muted small" style="margin:4px 0 10px">${t("Personal memory archive: stores a person's stories, facts, and ways of thinking locally, with an MCP server for AI agents.")}</p>
      <dl class="kv">
        <dt>${t("Version")}</dt><dd>${esc(a.version)}</dd>
        <dt>${t("Mode")}</dt><dd>${esc(host)}</dd>
        <dt>${t("System")}</dt><dd>${esc(a.os)} / ${esc(a.arch)}</dd>
        <dt>${t("License")}</dt><dd>${esc(a.license)}</dd>
        <dt>${t("Data location")}</dt><dd><code>${esc(a.vault)}</code></dd>
        ${a.config_path ? `<dt>${t("Configuration")}</dt><dd><code>${esc(a.config_path)}</code></dd>` : ""}
      </dl>
      <div class="row" style="margin-top:12px">
        <button type="button" data-open="repo">${icon("external")} ${t("Project page")}</button>
        <button type="button" data-open="releases">${icon("external")} ${t("Release notes")}</button>
        <button type="button" data-open="issues">${icon("flag")} ${t("Report an issue")}</button>
      </div>
    </div>
    <h2>${t("Updates")}</h2>
    <div class="card">
      <div class="row" style="justify-content:space-between">
        <div>${t("Installed version {version}", { version: `<b>${esc(a.version)}</b>` })}</div>
        <button type="button" id="st-check">${icon("refresh")} ${t("Check for updates")}</button>
      </div>
      <div id="st-update" class="small" style="margin-top:8px"></div>
      ${a.auto_update !== null && a.auto_update !== undefined ? `<label class="field check" style="margin-top:10px"><input type="checkbox" id="st-auto" ${a.auto_update ? "checked" : ""}><span>${t("Check for updates automatically")}</span></label>` : ""}
    </div>`;
  const urls = { repo: a.repository, releases: a.repository + "/releases", issues: a.repository + "/issues" };
  $("#st-body").querySelectorAll("[data-open]").forEach((b) => { b.onclick = () => openTarget(b.dataset.open, urls[b.dataset.open]); });
  $("#st-check").onclick = checkUpdate;
  if ($("#st-auto")) {
    $("#st-auto").onchange = async (e) => {
      try {
        await api("/api/update/auto", { method: "POST", json: { enabled: e.target.checked } });
        settings.about.auto_update = e.target.checked;
        toast(e.target.checked ? t("Automatic updates on") : t("Automatic updates off"));
      } catch (err) { toast(err.message); e.target.checked = !e.target.checked; }
    };
  }
  if (settings.update) renderUpdate();
  else if (settings.updateProgress?.running) applyUpdateProgress(settings.updateProgress);
  else checkUpdate();
}

async function checkUpdate() {
  if (settings.updateProgress?.running) return;
  const b = $("#st-check");
  if (b) b.disabled = true;
  if ($("#st-update")) $("#st-update").textContent = t("Checking...");
  try {
    settings.update = await api("/api/update/check", { method: "POST" });
    renderUpdate();
  } catch (e) { if ($("#st-update")) $("#st-update").innerHTML = errLine(e.message); }
  if (b) b.disabled = Boolean(settings.updateProgress?.running);
}

function formatBytes(n) {
  if (!n || n <= 0) return "0 MB";
  if (n < 1048576) return `${Math.max(1, Math.round(n / 1024))} KB`;
  return `${(n / 1048576).toFixed(1)} MB`;
}

function updateProgressHtml(p) {
  const hasPct = typeof p.percent === "number" && p.stage === "downloading";
  const pct = typeof p.percent === "number" ? Math.max(0, Math.min(100, p.percent)) : (p.stage === "installing" || p.stage === "restarting" ? 100 : 0);
  const indet = !hasPct && p.stage !== "error";
  const ver = p.version ? " v" + p.version : "";
  const stageText = p.stage === "checking"
    ? t("Preparing update download...")
    : p.stage === "installing"
      ? t("Installing update{version}...", { version: ver })
      : p.stage === "restarting"
        ? t("Update installed, restarting the app...")
        : t("Downloading update{version}...", { version: ver });
  const detailParts = [];
  if (p.downloaded_bytes > 0) {
    detailParts.push(p.total_bytes ? `${formatBytes(p.downloaded_bytes)} / ${formatBytes(p.total_bytes)}` : formatBytes(p.downloaded_bytes));
  }
  if (typeof p.percent === "number") detailParts.push(`${pct}%`);
  return `
    <div style="margin-top:8px">
      <div class="row" style="justify-content:space-between;gap:8px">
        <span class="small"><b>${icon("download")} ${esc(stageText)}</b></span>
        <span class="small muted">${esc(detailParts.join(" · "))}</span>
      </div>
      <div class="progress ${indet ? "indeterminate" : ""}" role="progressbar" aria-valuemin="0" aria-valuemax="100" ${hasPct ? `aria-valuenow="${pct}"` : ""}>
        <span style="width:${pct}%"></span>
      </div>
    </div>`;
}

let updatePollTimer = null;
function startUpdatePoll() {
  if (updatePollTimer) return;
  updatePollTimer = setInterval(async () => {
    try {
      const p = await api("/api/update/progress");
      applyUpdateProgress(p);
      if (!p.running) {
        clearInterval(updatePollTimer);
        updatePollTimer = null;
      }
    } catch (e) { /* aplikasi sedang ditutup untuk restart */ }
  }, 250);
}

function applyUpdateProgress(p) {
  if (!p) return;
  settings.updateProgress = p;
  const banner = $("#update-banner");
  if (banner) {
    if (p.running) {
      banner.hidden = false;
      banner.innerHTML = updateProgressHtml(p);
    } else {
      banner.hidden = true;
      banner.innerHTML = "";
    }
  }
  const checkBtn = $("#st-check");
  if (checkBtn) checkBtn.disabled = Boolean(p.running);
  const installBtn = $("#st-install");
  if (installBtn) installBtn.disabled = Boolean(p.running);
  const msgEl = $("#st-install-msg") || (p.running ? $("#st-update") : null);
  if (msgEl) {
    if (p.running) {
      msgEl.innerHTML = updateProgressHtml(p);
    } else if (p.stage === "error" && p.error) {
      msgEl.innerHTML = errLine(p.error);
    }
  }
  if (p.running) startUpdatePoll();
}

window.__onUpdateProgress = (p) => applyUpdateProgress(p);

function renderUpdate() {
  const u = settings.update, el = $("#st-update");
  updateGearDot();
  if (!el) return;
  if (!u.available) {
    if (settings.updateProgress?.running) { el.innerHTML = updateProgressHtml(settings.updateProgress); return; }
    el.innerHTML = `<span class="ok-text">${icon("check")} ${t("You are on the latest version{version}.", { version: u.latest ? ` (${esc(u.latest)})` : "" })}</span>`;
    return;
  }
  const running = Boolean(settings.updateProgress?.running);
  el.innerHTML = `
    <div class="notice">
      <b>${t("Version {version} available", { version: esc(u.latest) })}</b>${u.date ? ` <span class="muted">&middot; ${esc(new Date(u.date).toLocaleDateString(LOCALE))}</span>` : ""}
      ${u.notes ? `<pre class="src" style="max-height:200px;margin:8px 0">${esc(u.notes)}</pre>` : ""}
      <div class="row" style="margin-top:6px">
        ${u.can_install ? `<button class="primary" id="st-install" ${running ? "disabled" : ""}>${icon("download")} ${t("Install & restart")}</button>` : `<button type="button" id="st-dl">${icon("external")} ${t("Download from GitHub")}</button>`}
      </div>
      <div class="small" id="st-install-msg">${running ? updateProgressHtml(settings.updateProgress) : ""}</div>
    </div>`;
  if ($("#st-dl")) $("#st-dl").onclick = () => openTarget("releases", u.url);
  if ($("#st-install")) {
    $("#st-install").onclick = async () => {
      applyUpdateProgress({ running: true, stage: "checking", version: u.latest || null, downloaded_bytes: 0, total_bytes: null, percent: 0, message: t("Preparing update download..."), error: null });
      try {
        const r = await api("/api/update/install", { method: "POST" });
        if (r.progress) applyUpdateProgress(r.progress);
        startUpdatePoll();
      } catch (e) {
        applyUpdateProgress({ running: false, stage: "error", version: u.latest || null, downloaded_bytes: 0, total_bytes: null, percent: null, message: null, error: e.message });
      }
    };
  }
}

// Formulir di dalam halaman; jangan pakai prompt()/confirm(): WebView aplikasi
// desktop tidak menampilkan dialog browser.
function newSubjectForm() {
  openDrawer(t("New subject"), `
    <p class="muted small">${t("A subject is the person whose memories are stored.")}</p>
    <form id="subject-form" class="card">
      <p><label>${t("Name")}<br><input type="text" id="sf-name" required placeholder="${t("e.g. Jane Doe")}" style="width:100%"></label></p>
      <p><label>${t("Short ID")}<br><input type="text" id="sf-id" required pattern="[a-z0-9][a-z0-9_-]*" placeholder="${t("lowercase letters, digits, hyphens")}" style="width:100%"></label></p>
      <p><label>${t("Date of birth (optional)")}<br><input type="date" id="sf-birth"></label></p>
      <p class="small muted">${t("The date of birth is used to turn phrases like \"at age 20\" or \"in primary school\" into years.")}</p>
      <p id="sf-err" class="small" style="color:var(--bad)"></p>
      <button class="primary" type="submit">${icon("check")} ${t("Save")}</button>
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
      toast(t("Subject created"));
    } catch (err) { $("#sf-err").textContent = err.message; }
  };
}

function askToken() {
  if (!$("#drawer").hidden && $("#token-form")) return;
  openDrawer(t("Access token"), `
    <form id="token-form" class="card">
      <p class="small muted">${t("This server is protected by a token (env MEMORY_TOKEN).")}</p>
      <p><input type="password" id="tk" required style="width:100%" placeholder="token"></p>
      <button class="primary" type="submit">${icon("check")} ${t("Save")}</button>
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
$("#project-select").onchange = (ev) => { state.project = ev.target.value; localStorage.setItem("project", state.project); route(); };
$("#palette-btn").onclick = openPalette;
$("#sidebar-toggle").onclick = () => document.body.classList.toggle("nav-open");
pollServices();
setInterval(pollServices, 5000);
setTimeout(async () => {
  try {
    const a = await api("/api/about");
    if (a.auto_update === false) return;
    settings.update = await api("/api/update/check", { method: "POST" });
    updateGearDot();
  } catch (e) { /* offline: abaikan, bisa dicek manual di Pengaturan */ }
}, 5000);
loadSubjects().then(async () => { await loadProjects(); route(); }).catch((e) => { $("#view").innerHTML = `<div class="card">${t("Cannot reach the server: {error}", { error: esc(e.message) })}</div>`; });
