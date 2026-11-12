// UI web hub tim engram (/hub). Tanpa framework; semua data di-escape.
// Token akses & refresh disimpan di localStorage (diisi halaman sukses login), diperbarui
// otomatis saat 401. Rute: #hub/login, #hub/repos, #hub/repo/<id>[/<tab>], #hub/tokens.
"use strict";

// Ikon SVG inline (stroke) dan logo provider. Jangan pakai glyph Unicode sebagai ikon.
const HICONS = {
  key: '<circle cx="8" cy="15" r="4"/><path d="m11 12 9-9M17 6l3 3M14 9l2 2"/>',
  git: '<circle cx="6" cy="6" r="2"/><circle cx="6" cy="18" r="2"/><circle cx="18" cy="8" r="2"/><path d="M6 8v8M18 10a6 6 0 0 1-6 6H8"/>',
  logout: '<path d="M15 4h4v16h-4M10 8l-4 4 4 4M6 12h10"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  copy: '<rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V4H4v12h4"/>',
  refresh: '<path d="M20 11a8 8 0 0 0-14.5-4.5L4 8M4 4v4h4"/><path d="M4 13a8 8 0 0 0 14.5 4.5L20 16M20 20v-4h-4"/>',
  trash: '<path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13"/>',
  alert: '<path d="M12 4 2.5 20h19z"/><path d="M12 10v4M12 17v.5"/>',
};
const PROVIDER_SVG = {
  google: '<svg class="i brand-i" viewBox="0 0 24 24" aria-hidden="true"><path fill="#4285F4" d="M23 12.3c0-.8-.1-1.6-.2-2.3H12v4.4h6.2a5.3 5.3 0 0 1-2.3 3.5v2.9h3.7c2.2-2 3.4-5 3.4-8.5z"/><path fill="#34A853" d="M12 23.5c3.1 0 5.7-1 7.6-2.8l-3.7-2.9c-1 .7-2.4 1.1-3.9 1.1-3 0-5.5-2-6.4-4.7H1.8v3A11.5 11.5 0 0 0 12 23.5z"/><path fill="#FBBC05" d="M5.6 14.2a6.9 6.9 0 0 1 0-4.4v-3H1.8a11.5 11.5 0 0 0 0 10.4z"/><path fill="#EA4335" d="M12 4.6c1.7 0 3.2.6 4.4 1.7l3.3-3.3A11.5 11.5 0 0 0 1.8 6.8l3.8 3C6.5 6.6 9 4.6 12 4.6z"/></svg>',
  github: '<svg class="i brand-i" viewBox="0 0 24 24" aria-hidden="true"><path fill="currentColor" d="M12 .5a11.5 11.5 0 0 0-3.6 22.4c.6.1.8-.3.8-.6v-2c-3.2.7-3.9-1.5-3.9-1.5-.5-1.3-1.3-1.7-1.3-1.7-1-.7.1-.7.1-.7 1.2.1 1.8 1.2 1.8 1.2 1 1.8 2.8 1.3 3.5 1 .1-.8.4-1.3.7-1.6-2.6-.3-5.3-1.3-5.3-5.7 0-1.3.4-2.3 1.2-3.1-.1-.3-.5-1.5.1-3.1 0 0 1-.3 3.2 1.2a11 11 0 0 1 5.8 0c2.2-1.5 3.2-1.2 3.2-1.2.6 1.6.2 2.8.1 3.1.8.8 1.2 1.9 1.2 3.1 0 4.4-2.7 5.4-5.3 5.7.4.4.8 1.1.8 2.2v3.2c0 .3.2.7.8.6A11.5 11.5 0 0 0 12 .5z"/></svg>',
};
const hicon = (n) => `<svg class="i" viewBox="0 0 24 24" aria-hidden="true">${HICONS[n] || ""}</svg>`;
const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const $ = (sel, el = document) => el.querySelector(sel);
const when = (s) => (s || "").slice(0, 16).replace("T", " ");
const store = {
  get: (k) => { try { return localStorage.getItem(k); } catch (e) { return null; } },
  set: (k, v) => { try { v == null ? localStorage.removeItem(k) : localStorage.setItem(k, v); } catch (e) { /* storage diblokir */ } },
};
let INFO = { providers: [], name: "engram hub" };
let seq = 0;

function toast(msg) {
  const el = $("#toast");
  el.textContent = msg;
  el.classList.add("show");
  clearTimeout(el._h);
  el._h = setTimeout(() => el.classList.remove("show"), 2800);
}

async function copyText(text) {
  try { await navigator.clipboard.writeText(text); toast(t("Copied")); } catch (e) { toast(t("Could not copy; select the text manually")); }
}

function signOutLocal() {
  ["hub_token", "hub_refresh", "hub_user"].forEach((k) => store.set(k, null));
}

async function refreshToken() {
  const rt = store.get("hub_refresh");
  if (!rt) return false;
  const res = await fetch("/hub/auth/refresh", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ refresh_token: rt }) });
  if (!res.ok) { signOutLocal(); return false; }
  const tok = await res.json();
  store.set("hub_token", tok.access_token);
  store.set("hub_refresh", tok.refresh_token);
  store.set("hub_user", JSON.stringify(tok.user));
  return true;
}

async function hubApi(path, opts = {}, retried = false) {
  const headers = Object.assign({}, opts.headers || {});
  const tok = store.get("hub_token");
  if (tok) headers.Authorization = "Bearer " + tok;
  const init = Object.assign({}, opts, { headers });
  if (opts.json !== undefined) { headers["Content-Type"] = "application/json"; init.body = JSON.stringify(opts.json); }
  const res = await fetch(path, init);
  if (res.status === 401 && !retried && (await refreshToken())) return hubApi(path, opts, true);
  if (res.status === 401) { signOutLocal(); location.hash = "#hub/login"; throw new Error(t("Please sign in again")); }
  const body = res.headers.get("content-type")?.includes("json") ? await res.json() : await res.text();
  if (!res.ok) throw new Error(body?.error || res.statusText);
  return body;
}

const me = () => { try { return JSON.parse(store.get("hub_user") || "null"); } catch (e) { return null; } };
const roleBadge = (r) => `<span class="badge ${r === "owner" || r === "maintainer" ? "now" : ""}">${esc(t(r))}</span>`;
const canManage = (r) => r === "owner" || r === "maintainer";

function renderNav() {
  const u = me();
  $("#hub-name").textContent = INFO.name || "engram hub";
  $("#hub-nav").innerHTML = u && store.get("hub_token")
    ? `<a href="#hub/repos">${hicon("git")} ${t("Repositories")}</a><a href="#hub/tokens">${hicon("key")} ${t("Access tokens")}</a>
       <span class="muted small">${esc(u.name)} &lt;${esc(u.email)}&gt;</span><button class="ghost" id="hub-signout">${hicon("logout")} ${t("Sign out")}</button>`
    : "";
  const so = $("#hub-signout");
  if (so) so.onclick = async () => {
    try { await fetch("/hub/auth/logout", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ refresh_token: store.get("hub_refresh") || "" }) }); } catch (e) { /* tetap keluar lokal */ }
    signOutLocal();
    location.hash = "#hub/login";
  };
}

// ---------------------------------------------------------------- halaman

function viewLogin(el) {
  const btns = INFO.providers.map((p) => `<a class="button primary provider-btn" href="/hub/auth/login/${esc(p)}">${PROVIDER_SVG[p] || ""} ${p === "google" ? t("Sign in with Google") : t("Sign in with GitHub")}</a>`).join("");
  el.innerHTML = `<div class="card hub-login">
    <h1>${esc(INFO.name)}</h1>
    <p class="muted">${t("Shared project memory for AI agents. Anyone with access to a repo can read and add to that repo's memory: conventions, decisions, facts, and session summaries. Raw transcripts and embeddings never leave each member's own vault.")}</p>
    <div class="row provider-row">${btns || `<p class="bad">${t("No sign-in provider is configured on this hub. The operator must set [hub] google_client_id or github_client_id.")}</p>`}</div>
    <p class="small muted">${t("From a terminal: <code>engram login --hub {url}</code>", { url: esc(location.origin) })}</p>
  </div>`;
}

async function viewRepos(el) {
  const repos = await hubApi("/hub/api/repos");
  const list = repos.length
    ? `<div class="hub-repos">${repos.map((r) => `<a class="card hub-repo" href="#hub/repo/${esc(r.id)}">
        <div class="row" style="justify-content:space-between"><b>${esc(r.name)}</b>${roleBadge(r.role)}</div>
        <div class="small muted">${t("project")} <code>${esc(r.project)}</code>${r.github_full_name ? ` · ${hicon("git")} ${esc(r.github_full_name)}` : ""}</div>
        <div class="small">${t("{m} members · {l} lessons · {d} decisions · {f} facts · {e} episodes", { m: r.members, l: r.counts.lessons || 0, d: r.counts.decisions || 0, f: r.counts.facts || 0, e: r.counts.episodes || 0 })}</div>
      </a>`).join("")}</div>`
    : `<div class="card muted">${t("You are not a member of any shared repository yet. Run <code>engram repo init</code> inside a git repo, or <code>engram repo join</code> in a repo that already has <code>.engram/repo.toml</code>.")}</div>`;
  el.innerHTML = `<h1>${t("Repositories")}</h1>${list}
    <h2>${t("New shared repository")}</h2>
    <form class="card" id="new-repo">
      <label>${t("Project name")} <input id="nr-project" required placeholder="my-service" autocomplete="off"></label>
      <label>${t("Git remote (optional, links GitHub permission checks)")} <input id="nr-remote" placeholder="git@github.com:team/my-service.git" autocomplete="off"></label>
      <button class="primary" type="submit">${hicon("plus")} ${t("Create")}</button>
      <p class="small muted">${t("Usually easier from the repo itself: <code>engram repo init</code> creates the hub repository and writes <code>.engram/repo.toml</code> for you.")}</p>
    </form><div id="nr-out"></div>`;
  $("#new-repo", el).onsubmit = async (e) => {
    e.preventDefault();
    try {
      const r = await hubApi("/hub/api/repos", { method: "POST", json: { project: $("#nr-project", el).value.trim(), name: "", remote: $("#nr-remote", el).value.trim() || null } });
      $("#nr-out", el).innerHTML = repoTomlCard(r.repo, r.join_key);
      wireCopy(el);
    } catch (err) { toast(err.message); }
  };
}

function repoTomlCard(repo, key) {
  const toml = `format = 1\nrepo_id = "${repo.id}"\nhub = "${location.origin}"\njoin_key = "${key}"\nproject = "${repo.project}"\n\n[share]\nkinds = ${JSON.stringify(repo.share.kinds)}\nmax_records = ${repo.share.max_records}\nexclude = []\n`;
  return `<div class="card hub-key">
    <p>${hicon("alert")} ${t("The join key is shown only once. Save it as <code>.engram/repo.toml</code> in the repository and commit it; anyone who can clone the repo can then join.")}</p>
    <pre id="toml-text">${esc(toml)}</pre>
    <button type="button" data-copy="toml-text">${hicon("copy")} ${t("Copy")}</button>
  </div>`;
}

function wireCopy(el) {
  el.querySelectorAll("[data-copy]").forEach((b) => { b.onclick = () => copyText($("#" + b.dataset.copy, el).textContent); });
}

const KIND_LABEL = { lessons: "Lessons", decisions: "Decisions", facts: "Facts", episodes: "Episodes" };

function recordText(r) {
  const b = r.body || {};
  if (r.kind === "decisions") return `${esc(b.situation)} <span class="muted">-&gt;</span> <b>${esc(b.action)}</b>${b.reason ? `<div class="small muted">${esc(b.reason)}</div>` : ""}`;
  if (r.kind === "facts") return esc(b.statement);
  if (r.kind === "episodes") return `<b>${esc(b.title)}</b><div class="small muted">${esc(b.summary)}</div>`;
  return `<b>${esc(b.title)}</b>${b.detail ? `<div class="small muted">${esc(b.detail)}</div>` : ""}`;
}

async function viewRepo(el, id, tab = "records") {
  const repo = await hubApi(`/hub/api/repos/${encodeURIComponent(id)}`);
  const tabs = [["records", t("Memory")], ["members", t("Members")], ["activity", t("Activity")]];
  if (canManage(repo.role)) tabs.push(["settings", t("Settings")]);
  el.innerHTML = `<p class="small"><a href="#hub/repos">${t("Repositories")}</a></p>
    <h1>${esc(repo.name)} ${roleBadge(repo.role)}</h1>
    <p class="small muted">${t("project")} <code>${esc(repo.project)}</code> · ${t("{n} changes", { n: repo.head_seq })}${repo.github_full_name ? ` · ${hicon("git")} ${esc(repo.github_full_name)}` : ""}${repo.archived ? ` · <span class="badge warn">${t("archived")}</span>` : ""}</p>
    <div class="tabs">${tabs.map(([k, l]) => `<a href="#hub/repo/${esc(id)}/${k}" class="${k === tab ? "active" : ""}">${esc(l)}</a>`).join("")}</div>
    <div id="tab-body"></div>`;
  const body = $("#tab-body", el);
  if (tab === "members") return tabMembers(body, repo);
  if (tab === "activity") return tabActivity(body, repo);
  if (tab === "settings" && canManage(repo.role)) return tabSettings(body, repo);
  return tabRecords(body, repo);
}

async function tabRecords(body, repo) {
  const kinds = Object.keys(KIND_LABEL);
  const sel = store.get("hub-kind") || "decisions";
  const recs = await hubApi(`/hub/api/repos/${encodeURIComponent(repo.id)}/records?kind=${encodeURIComponent(sel)}&limit=300`);
  body.innerHTML = `<div class="tag-filter">${kinds.map((k) => `<button type="button" data-kind="${k}" class="${k === sel ? "active" : ""}">${esc(t(KIND_LABEL[k]))} <span class="small muted">${repo.counts[k] || 0}</span></button>`).join("")}</div>
    ${recs.length ? `<ul class="hub-records">${recs.map((r) => `<li class="card">${recordText(r)}<div class="small muted">${esc(r.body?.author || r.author)} · ${esc(r.device)} · ${esc(when(r.updated_at))}</div></li>`).join("")}</ul>`
      : `<div class="card muted">${t("Nothing shared yet. Members push automatically at the end of agent sessions and every 15 minutes, or with <code>engram repo push</code>.")}</div>`}`;
  body.querySelectorAll("[data-kind]").forEach((b) => { b.onclick = () => { store.set("hub-kind", b.dataset.kind); tabRecords(body, repo); }; });
}

async function tabMembers(body, repo) {
  const ms = await hubApi(`/hub/api/repos/${encodeURIComponent(repo.id)}/members`);
  const u = me();
  const roles = ["reader", "member", "maintainer", "owner"];
  body.innerHTML = `<table class="hub-table"><thead><tr><th>${t("Name")}</th><th>${t("Role")}</th><th>${t("Joined via")}</th><th>${t("Devices")}</th><th>${t("Last push")}</th><th></th></tr></thead><tbody>
    ${ms.map((m) => `<tr>
      <td><b>${esc(m.name)}</b>${m.email ? `<div class="small muted">${esc(m.email)}</div>` : ""}</td>
      <td>${canManage(repo.role) && m.user_id !== u?.id ? `<select data-role="${esc(m.user_id)}">${roles.map((r) => `<option value="${r}" ${r === m.role ? "selected" : ""}>${esc(t(r))}</option>`).join("")}</select>` : roleBadge(m.role)}</td>
      <td class="small">${esc(m.joined_via)}${m.github_verified_at ? ` · ${t("GitHub verified")}` : ""}</td>
      <td class="small">${esc((m.devices || []).join(", "))}</td>
      <td class="small">${esc(when(m.last_push_at)) || "-"}</td>
      <td>${m.user_id === u?.id ? `<button class="ghost" data-leave="${esc(m.user_id)}">${t("Leave")}</button>` : canManage(repo.role) ? `<button class="ghost" data-revoke="${esc(m.user_id)}" title="${t("Remove member; their shared records are withdrawn")}">${hicon("trash")}</button>` : ""}</td>
    </tr>`).join("")}</tbody></table>
    <p class="small muted">${t("New teammates join automatically: they clone the repo (which contains <code>.engram/repo.toml</code>), sign in with <code>engram login</code>, and start an agent session.")}</p>`;
  const base = `/hub/api/repos/${encodeURIComponent(repo.id)}/members/`;
  body.querySelectorAll("[data-role]").forEach((s) => { s.onchange = async () => { try { await hubApi(base + encodeURIComponent(s.dataset.role), { method: "PUT", json: { role: s.value } }); toast(t("Role updated")); } catch (e) { toast(e.message); tabMembers(body, repo); } }; });
  body.querySelectorAll("[data-revoke],[data-leave]").forEach((b) => {
    b.onclick = async () => {
      const uid = b.dataset.revoke || b.dataset.leave;
      if (!confirm(b.dataset.leave ? t("Leave this repository? Your shared records will be withdrawn.") : t("Remove this member? Their shared records will be withdrawn."))) return;
      try { await hubApi(base + encodeURIComponent(uid), { method: "DELETE" }); toast(t("Done")); if (b.dataset.leave) location.hash = "#hub/repos"; else tabMembers(body, repo); } catch (e) { toast(e.message); }
    };
  });
}

const ACTION_LABEL = { push: "pushed", join: "joined", "join.fail": "failed to join", "key.rotate": "rotated the join key", "role.set": "changed a role", "member.revoke": "removed a member", "member.leave": "left", "repo.create": "created the repository", "repo.update": "updated settings" };

async function tabActivity(body, repo) {
  const rows = await hubApi(`/hub/api/repos/${encodeURIComponent(repo.id)}/activity?limit=200`);
  body.innerHTML = rows.length ? `<ul class="hub-activity">${rows.map((a) => {
    const d = a.detail || {};
    const extra = a.action === "push" ? t("{a} records, {w} withdrawn", { a: d.accepted || 0, w: d.withdrawn || 0 }) : a.action === "role.set" ? `${esc(d.from)} -&gt; ${esc(d.to)}` : "";
    return `<li><span class="small muted">${esc(when(a.at))}</span> <b>${esc(a.actor_name || a.actor)}</b> ${esc(t(ACTION_LABEL[a.action] || a.action))} <span class="small muted">${extra}</span></li>`;
  }).join("")}</ul>` : `<div class="card muted">${t("No activity yet.")}</div>`;
}

function tabSettings(body, repo) {
  const kinds = Object.keys(KIND_LABEL);
  body.innerHTML = `<form class="card" id="rs-form">
      <label>${t("Name")} <input id="rs-name" value="${esc(repo.name)}"></label>
      <fieldset><legend>${t("What members share")}</legend>${kinds.map((k) => `<label class="inline"><input type="checkbox" data-k="${k}" ${repo.share.kinds.includes(k) ? "checked" : ""}> ${esc(t(KIND_LABEL[k]))}</label>`).join("")}</fieldset>
      <label>${t("GitHub repository (owner/name)")} <input id="rs-gh" value="${esc(repo.github_full_name || "")}" placeholder="team/my-service"></label>
      <label class="inline"><input type="checkbox" id="rs-join" ${repo.join_enabled ? "checked" : ""}> ${t("Allow joining with the join key from repo.toml")}</label>
      <label class="inline"><input type="checkbox" id="rs-ghv" ${repo.require_github_verify ? "checked" : ""}> ${t("Require GitHub permission check to join")}</label>
      <label class="inline"><input type="checkbox" id="rs-arch" ${repo.archived ? "checked" : ""}> ${t("Archived (read-only)")}</label>
      <button class="primary" type="submit">${t("Save")}</button>
    </form>
    <div class="card"><b>${hicon("key")} ${t("Join key")}</b> <span class="small muted">${t("version {v}", { v: repo.join_key_version })}</span>
      <p class="small muted">${t("Rotate if the key leaked (for example a fork of a private repo). The old key stops working immediately; commit the new repo.toml. From a terminal: <code>engram repo rotate-key</code>.")}</p>
      <button type="button" id="rs-rotate">${hicon("refresh")} ${t("Rotate join key")}</button><div id="rs-key"></div></div>`;
  $("#rs-form", body).onsubmit = async (e) => {
    e.preventDefault();
    const share = { kinds: kinds.filter((k) => $(`[data-k="${k}"]`, body).checked), max_records: repo.share.max_records, exclude: repo.share.exclude };
    try {
      await hubApi(`/hub/api/repos/${encodeURIComponent(repo.id)}`, { method: "PATCH", json: { name: $("#rs-name", body).value.trim(), share, join_enabled: $("#rs-join", body).checked, require_github_verify: $("#rs-ghv", body).checked, archived: $("#rs-arch", body).checked, github_full_name: $("#rs-gh", body).value.trim() } });
      toast(t("Saved"));
    } catch (err) { toast(err.message); }
  };
  $("#rs-rotate", body).onclick = async () => {
    if (!confirm(t("Rotate the join key? Members who already joined are not affected."))) return;
    try {
      const r = await hubApi(`/hub/api/repos/${encodeURIComponent(repo.id)}/key/rotate`, { method: "POST", json: {} });
      $("#rs-key", body).innerHTML = repoTomlCard(repo, r.join_key);
      wireCopy(body);
    } catch (err) { toast(err.message); }
  };
}

async function viewTokens(el) {
  const rows = await hubApi("/hub/api/me/tokens");
  el.innerHTML = `<h1>${t("Access tokens")}</h1>
    <p class="muted">${t("Personal access tokens let CI jobs and cloud agents read or push repo memory without a browser: set <code>ENGRAM_HUB_URL</code> and <code>ENGRAM_HUB_TOKEN</code>.")}</p>
    ${rows.length ? `<table class="hub-table"><thead><tr><th>${t("Name")}</th><th>${t("Scopes")}</th><th>${t("Expires")}</th><th>${t("Last used")}</th><th></th></tr></thead><tbody>
      ${rows.map((r) => `<tr><td><b>${esc(r.name)}</b> <code class="small">${esc(r.prefix)}</code></td><td class="small">${esc(r.scopes.join(", "))}</td><td class="small">${esc(when(r.expires_at)) || t("never")}</td><td class="small">${esc(when(r.last_used_at)) || "-"}</td><td><button class="ghost" data-del="${esc(r.id)}" title="${t("Revoke")}">${hicon("trash")}</button></td></tr>`).join("")}
    </tbody></table>` : ""}
    <form class="card" id="tk-form">
      <label>${t("Name")} <input id="tk-name" required placeholder="ci-main"></label>
      <fieldset><legend>${t("Scopes")}</legend>${["repo:read", "repo:write", "mcp"].map((s) => `<label class="inline"><input type="checkbox" data-s="${s}" ${s !== "mcp" ? "checked" : ""}> ${s}</label>`).join("")}</fieldset>
      <label>${t("Expires after (days, empty = never)")} <input id="tk-days" type="number" min="1" value="90"></label>
      <button class="primary" type="submit">${hicon("plus")} ${t("Create token")}</button>
    </form><div id="tk-out"></div>`;
  el.querySelectorAll("[data-del]").forEach((b) => { b.onclick = async () => { if (!confirm(t("Revoke this token?"))) return; try { await hubApi(`/hub/api/me/tokens/${encodeURIComponent(b.dataset.del)}`, { method: "DELETE" }); viewTokens(el); } catch (e) { toast(e.message); } }; });
  $("#tk-form", el).onsubmit = async (e) => {
    e.preventDefault();
    const scopes = ["repo:read", "repo:write", "mcp"].filter((s) => $(`[data-s="${s}"]`, el).checked);
    const days = parseInt($("#tk-days", el).value, 10);
    try {
      const r = await hubApi("/hub/api/me/tokens", { method: "POST", json: { name: $("#tk-name", el).value.trim(), scopes, expires_days: days > 0 ? days : null } });
      $("#tk-out", el).innerHTML = `<div class="card hub-key"><p>${hicon("alert")} ${t("Copy the token now; it is shown only once.")}</p><pre id="tk-secret">${esc(r.secret)}</pre><button type="button" data-copy="tk-secret">${hicon("copy")} ${t("Copy")}</button></div>`;
      wireCopy(el);
    } catch (err) { toast(err.message); }
  };
}

async function route() {
  const my = ++seq;
  renderNav();
  const parts = location.hash.replace(/^#hub\/?/, "").split("/").filter(Boolean).map(decodeURIComponent);
  const view = $("#view");
  const page = document.createElement("div");
  page.className = "page";
  try {
    if (!store.get("hub_token") || parts[0] === "login") viewLogin(page);
    else if (parts[0] === "repo" && parts[1]) await viewRepo(page, parts[1], parts[2]);
    else if (parts[0] === "tokens") await viewTokens(page);
    else await viewRepos(page);
    if (my !== seq) return;
    view.replaceChildren(page);
  } catch (e) {
    if (my !== seq) return;
    view.innerHTML = `<div class="card">${t("Failed to load: {error}", { error: esc(e.message) })}</div>`;
  }
}

(async () => {
  try { INFO = await (await fetch("/hub/api/info")).json(); } catch (e) { /* hub tidak terjangkau: halaman login menampilkan galat */ }
  document.title = INFO.name || "engram hub";
  window.addEventListener("hashchange", route);
  if (!location.hash.startsWith("#hub")) location.hash = store.get("hub_token") ? "#hub/repos" : "#hub/login";
  route();
})();
