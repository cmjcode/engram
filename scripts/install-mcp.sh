#!/usr/bin/env bash
# Pasang server MCP engram ke agent AI (Antigravity, Gemini CLI, Claude Code,
# Cursor, Codex CLI, Windsurf).
#
# Profil tool: client koding murni (cursor, codex, windsurf) memakai profil
# `coding` (hanya project_context, recall, remember, who_is, timeline, principles)
# agar konteks agent tidak dipenuhi tool memori hidup; antigravity, gemini, dan
# claude memakai profil `full`. Timpa dengan --profile.
#
# Mode http (bawaan): agent tersambung ke aplikasi desktop / `engram serve`
#   lewat http://127.0.0.1:7878/mcp dengan token tetap (MEMORY_TOKEN).
# Mode stdio: agent menjalankan binary `engram ... mcp` sendiri.
#   Hanya bisa dipakai saat aplikasi desktop TIDAK jalan (vault dikunci satu proses).
#
# Contoh:
#   scripts/install-mcp.sh                       # Antigravity, mode http
#   scripts/install-mcp.sh --client all
#   scripts/install-mcp.sh --client claude --mode stdio
#   scripts/install-mcp.sh --client codex                # ~/.codex/config.toml, profil coding
#   scripts/install-mcp.sh --uninstall --client all
set -euo pipefail

NAME="engram"
CLIENT="antigravity"
MODE="http"
PROFILE="auto"
TOKEN=""
UNINSTALL=0
DRY_RUN=0

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DATA_DIR="$HOME/Library/Application Support/id.engram.ai"
CONFIG="$DATA_DIR/memory.toml"
BIN="$ROOT/target/release/engram"
TOKEN_FILE="$DATA_DIR/mcp_token"
AGENT_LABEL="id.engram.ai.token"
AGENT_PLIST="$HOME/Library/LaunchAgents/$AGENT_LABEL.plist"
DEFAULT_URL="http://127.0.0.1:7878/mcp"

usage() {
  cat <<EOF
Pemakaian: $(basename "$0") [opsi]

  --client NAMA   antigravity | gemini | claude | cursor | codex | windsurf | all
                  (bawaan: antigravity; 'all' = yang terpasang di komputer ini)
  --mode MODE     http | stdio                          (bawaan: http)
  --profile P     auto | full | coding                  (bawaan: auto, per client)
  --name NAMA     nama server MCP                       (bawaan: engram)
  --token TOKEN   pakai token ini (mode http); bila kosong, pakai yang ada atau buat baru
  --config PATH   memory.toml untuk mode stdio          (bawaan: config aplikasi desktop)
  --bin PATH      binary engram untuk mode stdio        (bawaan: target/release/engram)
  --uninstall     hapus server dari client terpilih
  --dry-run       tampilkan rencana tanpa mengubah apa pun
  -h, --help      bantuan ini
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --client) CLIENT="$2"; shift 2 ;;
    --mode) MODE="$2"; shift 2 ;;
    --profile) PROFILE="$2"; shift 2 ;;
    --name) NAME="$2"; shift 2 ;;
    --token) TOKEN="$2"; shift 2 ;;
    --config) CONFIG="$2"; shift 2 ;;
    --bin) BIN="$2"; shift 2 ;;
    --uninstall) UNINSTALL=1; shift ;;
    --dry-run) DRY_RUN=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Opsi tidak dikenal: $1" >&2; usage; exit 2 ;;
  esac
done

case "$CLIENT" in
  antigravity|gemini|claude|cursor|codex|windsurf) CLIENTS=("$CLIENT") ;;
  all)
    CLIENTS=(antigravity gemini claude cursor)
    if [[ -d "$HOME/.codex" ]] || command -v codex >/dev/null; then CLIENTS+=(codex); fi
    if [[ -d "$HOME/.codeium/windsurf" || -d "/Applications/Windsurf.app" ]]; then CLIENTS+=(windsurf); fi
    ;;
  *) echo "Client tidak dikenal: $CLIENT" >&2; exit 2 ;;
esac
case "$MODE" in http|stdio) ;; *) echo "Mode tidak dikenal: $MODE" >&2; exit 2 ;; esac
case "$PROFILE" in auto|full|coding) ;; *) echo "Profile tidak dikenal: $PROFILE" >&2; exit 2 ;; esac

# profile_for CLIENT -> full | coding
profile_for() {
  if [[ "$PROFILE" != "auto" ]]; then echo "$PROFILE"; return; fi
  case "$1" in cursor|codex|windsurf) echo coding ;; *) echo full ;; esac
}

# url_for CLIENT -> URL server + ?profile=coding bila perlu
url_for() {
  local prof; prof="$(profile_for "$1")"
  if [[ "$prof" == "full" ]]; then echo "$URL"; else echo "$URL?profile=$prof"; fi
}

info() { printf '  %s\n' "$*"; }
step() { printf '\n==> %s\n' "$*"; }
run() { if [[ $DRY_RUN -eq 1 ]]; then echo "  [dry-run] $*"; else "$@"; fi; }

command -v python3 >/dev/null || { echo "python3 diperlukan untuk mengedit config JSON." >&2; exit 1; }

# ---------- token & URL (mode http) ----------

ensure_token() {
  [[ -n "$TOKEN" ]] || TOKEN="$(launchctl getenv MEMORY_TOKEN 2>/dev/null || true)"
  if [[ -z "$TOKEN" && -f "$TOKEN_FILE" ]]; then
    TOKEN="$(tr -d '[:space:]' < "$TOKEN_FILE")"
  fi
  if [[ -z "$TOKEN" ]]; then
    TOKEN="$(openssl rand -hex 24)"
    info "Token baru dibuat."
  fi

  step "Menyimpan token tetap MEMORY_TOKEN"
  if [[ $DRY_RUN -eq 1 ]]; then
    info "[dry-run] simpan token ke $TOKEN_FILE dan $AGENT_PLIST"
    return
  fi
  mkdir -p "$DATA_DIR" "$(dirname "$AGENT_PLIST")"
  (umask 077; printf '%s\n' "$TOKEN" > "$TOKEN_FILE")
  # Aplikasi GUI mewarisi env dari launchd. LaunchAgent ini mengisi ulang
  # MEMORY_TOKEN setiap login, karena `launchctl setenv` hilang saat reboot.
  (umask 077; cat > "$AGENT_PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>$AGENT_LABEL</string>
  <key>ProgramArguments</key>
  <array>
    <string>/bin/launchctl</string><string>setenv</string>
    <string>MEMORY_TOKEN</string><string>$TOKEN</string>
  </array>
  <key>RunAtLoad</key><true/>
</dict>
</plist>
EOF
  )
  launchctl setenv MEMORY_TOKEN "$TOKEN"
  launchctl bootout "gui/$(id -u)/$AGENT_LABEL" 2>/dev/null || true
  launchctl bootstrap "gui/$(id -u)" "$AGENT_PLIST" 2>/dev/null || true
  info "Token disimpan di $TOKEN_FILE"
  info "LaunchAgent: $AGENT_PLIST (token tetap setelah reboot)"
}

mcp_url() {
  local f="$DATA_DIR/connection.json"
  if [[ -f "$f" ]]; then
    python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("mcp_url",""))' "$f" 2>/dev/null || true
  fi
}

# ---------- penulis config JSON ----------

# edit_json FILE NAME ENTRY_JSON|"" -> tambah/ganti (atau hapus bila ENTRY kosong) mcpServers.NAME
edit_json() {
  local file="$1" name="$2" entry="$3"
  if [[ $DRY_RUN -eq 1 ]]; then
    if [[ -n "$entry" ]]; then info "[dry-run] $file: mcpServers.$name = $entry"
    else info "[dry-run] $file: hapus mcpServers.$name"; fi
    return
  fi
  mkdir -p "$(dirname "$file")"
  [[ -f "$file" ]] && cp -p "$file" "$file.bak.$(date +%Y%m%d%H%M%S)"
  python3 - "$file" "$name" "$entry" <<'PY'
import json, os, sys
path = os.path.realpath(sys.argv[1])
name, entry = sys.argv[2], sys.argv[3]
data = {}
if os.path.exists(path) and os.path.getsize(path) > 0:
    with open(path) as f:
        data = json.load(f)
servers = data.setdefault("mcpServers", {})
servers.pop("memori", None)
if entry:
    servers[name] = json.loads(entry)
else:
    servers.pop(name, None)
tmp = path + ".tmp"
with open(tmp, "w") as f:
    json.dump(data, f, indent=2, ensure_ascii=False)
    f.write("\n")
os.chmod(tmp, 0o600)  # berisi token
os.replace(tmp, path)
PY
  info "Diperbarui: $file"
}

# entry_json CLIENT -> JSON entri server untuk gaya client
# (antigravity & windsurf: serverUrl; gemini: httpUrl; cursor/claude: url)
entry_json() {
  python3 - "$1" "$MODE" "$(url_for "$1")" "$TOKEN" "$BIN" "$CONFIG" "$(profile_for "$1")" <<'PY'
import json, sys
style, mode, url, token, bin_, cfg, profile = sys.argv[1:]
if mode == "http":
    if style == "antigravity":
        e = {"serverUrl": url, "headers": {"Authorization": f"Bearer {token}"}, "disabled": False}
    elif style == "windsurf":
        e = {"serverUrl": url, "headers": {"Authorization": f"Bearer {token}"}}
    elif style == "gemini":
        e = {"httpUrl": url, "headers": {"Authorization": f"Bearer {token}"}, "trust": True}
    else:
        e = {"url": url, "headers": {"Authorization": f"Bearer {token}"}}
else:
    e = {"command": bin_, "args": ["--config", cfg, "mcp", "--profile", profile]}
    if style == "antigravity":
        e["disabled"] = False
    elif style == "gemini":
        e["trust"] = True
print(json.dumps(e))
PY
}

# edit_toml FILE NAME -> tambah/ganti (atau hapus bila UNINSTALL) [mcp_servers.NAME] di config Codex.
# Bagian lain berkas (komentar, urutan) tidak disentuh.
edit_toml() {
  local file="$1" name="$2"
  if [[ $DRY_RUN -eq 1 ]]; then
    if [[ $UNINSTALL -eq 1 ]]; then info "[dry-run] $file: hapus [mcp_servers.$name]"
    else info "[dry-run] $file: [mcp_servers.$name] url=$(url_for codex)"; fi
    return
  fi
  mkdir -p "$(dirname "$file")"
  [[ -f "$file" ]] && cp -p "$file" "$file.bak.$(date +%Y%m%d%H%M%S)"
  python3 - "$file" "$name" "$UNINSTALL" "$MODE" "$(url_for codex)" "$TOKEN" "$BIN" "$CONFIG" "$(profile_for codex)" <<'PY'
import json, os, sys, tomllib
path, name, uninstall, mode, url, token, bin_, cfg, profile = sys.argv[1:]
path = os.path.realpath(path)
text = open(path, encoding="utf-8").read() if os.path.exists(path) else ""
tomllib.loads(text)  # gagal = berkas rusak, jangan sentuh
def is_header(line):
    t = line.lstrip()
    return t.startswith("[") and t.rstrip().endswith("]")
def is_ours(line, n):
    t = line.strip().strip("[]").replace('"', "").replace("'", "").replace(" ", "")
    return t == f"mcp_servers.{n}" or t.startswith(f"mcp_servers.{n}.")
out, skip = [], False
for line in text.splitlines():
    if is_header(line):
        skip = is_ours(line, name) or is_ours(line, "memori")
    if not skip:
        out.append(line)
body = "\n".join(out).rstrip()
if uninstall != "1":
    q = json.dumps  # kutip string gaya TOML basic
    sec = [f"[mcp_servers.{name}]"]
    if mode == "http":
        sec.append(f"url = {q(url)}")
        if token:
            sec.append(f"http_headers = {{ Authorization = {q('Bearer ' + token)} }}")
    else:
        sec.append(f"command = {q(bin_)}")
        sec.append("args = [" + ", ".join(q(a) for a in ["--config", cfg, "mcp", "--profile", profile]) + "]")
    body = (body + "\n\n" if body else "") + "\n".join(sec)
body = body.rstrip() + "\n"
tomllib.loads(body)
tmp = path + ".tmp"
with open(tmp, "w", encoding="utf-8") as f:
    f.write(body)
os.chmod(tmp, 0o600)  # berisi token
os.replace(tmp, path)
PY
  info "Diperbarui: $file"
}

HOOK_SRC="$ROOT/scripts/auto-remember-hook.py"
HOOK_BIN="$HOME/.local/bin/engram-auto-remember"
CTX_SRC="$ROOT/scripts/session-context-hook.py"
CTX_BIN="$HOME/.local/bin/engram-session-context"

# Hook utama memakai binary `engram hook <client> <event>` (cepat, tanpa Python, merekam
# tiap pemanggilan tool + ringkasan sesi, menyuntik indeks konteks saat sesi mulai, dan
# menyimpan event ke spool bila server mati). Skrip Python di atas hanya cadangan bila
# binary belum terpasang. Timpa dengan env ENGRAM_BIN.
LOCAL_ENGRAM_BIN="$HOME/.local/bin/engram"
supports_hook() {
  [[ -n "${1:-}" && -x "$1" ]] && "$1" hook --help >/dev/null 2>&1
}

# Bila binary hasil build lokal ($BIN) mendukung `hook` dan lebih baru (atau binary di
# ~/.local/bin belum mendukung `hook`), perbarui ~/.local/bin/engram secara atomik.
if [[ $DRY_RUN -eq 0 && $UNINSTALL -eq 0 ]] && supports_hook "$BIN"; then
  if ! supports_hook "$LOCAL_ENGRAM_BIN" || [[ "$BIN" -nt "$LOCAL_ENGRAM_BIN" ]]; then
    mkdir -p "$(dirname "$LOCAL_ENGRAM_BIN")"
    rm -f "$LOCAL_ENGRAM_BIN.tmp"
    cp "$BIN" "$LOCAL_ENGRAM_BIN.tmp"
    chmod 755 "$LOCAL_ENGRAM_BIN.tmp"
    mv -f "$LOCAL_ENGRAM_BIN.tmp" "$LOCAL_ENGRAM_BIN"
    info "Binary CLI diperbarui: $LOCAL_ENGRAM_BIN ($("$LOCAL_ENGRAM_BIN" --version 2>/dev/null || true))"
  fi
fi

if [[ -z "${ENGRAM_BIN:-}" ]]; then
  for cand in "$LOCAL_ENGRAM_BIN" "$(command -v engram 2>/dev/null || true)" "$BIN"; do
    if supports_hook "$cand"; then
      ENGRAM_BIN="$cand"
      break
    fi
  done
elif ! supports_hook "$ENGRAM_BIN"; then
  info "Peringatan: $ENGRAM_BIN belum mendukung subcommand 'hook'; memakai fallback Python."
  ENGRAM_BIN=""
fi

ensure_hook_bin() {
  if [[ $DRY_RUN -eq 1 ]]; then
    info "[dry-run] pasang $HOOK_BIN dan $CTX_BIN"
    return
  fi
  mkdir -p "$(dirname "$HOOK_BIN")"
  cp "$HOOK_SRC" "$HOOK_BIN"
  cp "$CTX_SRC" "$CTX_BIN"
  chmod 755 "$HOOK_BIN" "$CTX_BIN"
  rm -f "$HOME/.local/bin/mnemonic-auto-remember"
}

# upsert_rule_section FILE [full|coding]
upsert_rule_section() {
  local file="$1" variant="${2:-full}"
  if [[ $DRY_RUN -eq 1 ]]; then
    info "[dry-run] perbarui rule engram ($variant) di $file"
    return
  fi
  mkdir -p "$(dirname "$file")"
  python3 - "$file" "$UNINSTALL" "$variant" <<'PY'
import os, re, sys
path, uninstall, variant = sys.argv[1], sys.argv[2] == "1", sys.argv[3]
marker_start = "<!-- ENGRAM_START -->"
marker_end = "<!-- ENGRAM_END -->"
legacy_start = "<!-- MNEMONIC_MEMORY_START -->"
legacy_end = "<!-- MNEMONIC_MEMORY_END -->"
coding_block = f"""{marker_start}
## Engram (`engram` MCP)

- **Awal tugas**: panggil `project_context` (parameter `project` = nama repositori/folder kerja, `task` = tugas saat ini) sebelum membaca kode, agar keputusan arsitektur, konvensi, dan akar masalah bug dari sesi sebelumnya ikut dipertimbangkan. Untuk pertanyaan spesifik pakai `recall` dengan parameter `project`.
- **Fakta berlabel waktu**: hasil tool membedakan fakta yang masih berlaku dan riwayat. Jangan menyajikan fakta lama sebagai kondisi sekarang; bila tool menjawab tidak ada catatan, katakan apa adanya.
- **Simpan keputusan (`remember`)**: setelah keputusan teknis, perubahan arsitektur, konvensi baru, atau akar masalah bug ditemukan, simpan ringkasannya lewat `remember` dengan `source` berformat `<client>:<nama-project>` (misal `codex:LLM-MEMORY`).
{marker_end}"""
block = f"""{marker_start}
## Engram (`engram` MCP)

- **Simpan Otomatis (`remember`)**: Saat pengguna menceritakan kisah hidup, fakta pribadi/keluarga, pengalaman masa lalu, preferensi, atau keputusan penting (termasuk keputusan arsitektur/teknis, konvensi kode, akar masalah bug, dan konfigurasi di project), panggil tool MCP `remember` (dari server `engram`) untuk menyimpannya tanpa perlu diminta. Isi parameter `source` dengan format `<client>:<nama-project>` (misal `antigravity:LLM-MEMORY`) dan `project` sesuai nama project.
- **Ambil Konteks (`project_context`, `recall`, `who_is`, `timeline`, `principles`, `family_tree`)**: Saat mulai mengerjakan tugas di suatu project atau saat pengguna bertanya tentang arsitektur/keputusan teknis/masa lalu/keluarga/prinsip kerja, panggil tool MCP `engram` yang relevan terlebih dahulu (`project_context` untuk ringkasan lengkap project, atau `recall` dengan parameter `project`). Selalu bedakan fakta yang masih berlaku sekarang dengan riwayat masa lalu.
- **Rangkum & Padatkan Memori (`compact`)**: Saat memori sudah cukup banyak atau pengguna meminta merangkum/memadatkan memori, panggil tool MCP `compact` dengan `mode="candidates"` untuk meninjau klaster episode serupa, fakta redundan/sesaat per entitas, dan review terbuka; lalu kirim rangkuman intinya lewat `compact` dengan `mode="apply"` dan `plan`.
{marker_end}"""
if variant == "coding":
    block = coding_block

text = ""
if os.path.exists(path):
    with open(path, "r", encoding="utf-8") as f:
        text = f.read()

legacy_pattern = re.compile(re.escape(legacy_start) + r".*?" + re.escape(legacy_end) + r"\n?", re.DOTALL)
text = legacy_pattern.sub("", text)

pattern = re.compile(re.escape(marker_start) + r".*?" + re.escape(marker_end) + r"\n?", re.DOTALL)
if uninstall:
    new_text = pattern.sub("", text).rstrip()
    if new_text:
        new_text += "\n"
else:
    if pattern.search(text):
        new_text = pattern.sub(block + "\n", text)
    else:
        sep = "\n\n" if text.strip() else ""
        new_text = text.rstrip() + sep + block + "\n"

with open(path, "w", encoding="utf-8") as f:
    f.write(new_text)
PY
  info "Rule diperbarui: $file"
}

# ---------- per client ----------

do_antigravity() {
  step "Antigravity (profil $(profile_for antigravity))"
  local cfg_main="$HOME/.gemini/config/mcp_config.json"
  local cfg_agy="$HOME/.gemini/antigravity/mcp_config.json"
  local hooks_f="$HOME/.gemini/config/hooks.json"

  if [[ $UNINSTALL -eq 1 ]]; then
    edit_json "$cfg_main" "$NAME" ""
    [[ -f "$cfg_agy" && ! -L "$cfg_agy" ]] && edit_json "$cfg_agy" "$NAME" ""
    upsert_rule_section "$HOME/.gemini/GEMINI.md"
    upsert_rule_section "$HOME/.gemini/config/GEMINI.md"
    if [[ $DRY_RUN -eq 0 && -f "$hooks_f" ]]; then
      python3 - "$hooks_f" <<'PY'
import json, os, sys
p = sys.argv[1]
d = json.load(open(p)) if os.path.exists(p) and os.path.getsize(p) > 0 else {}
d.pop("engram-auto-remember", None)
json.dump(d, open(p, "w"), indent=2)
PY
    fi
    return
  fi

  ensure_hook_bin
  edit_json "$cfg_main" "$NAME" "$(entry_json antigravity)"
  # Pastikan ~/.gemini/antigravity/mcp_config.json menunjuk ke ~/.gemini/config/mcp_config.json
  if [[ $DRY_RUN -eq 0 ]]; then
    mkdir -p "$(dirname "$cfg_agy")"
    ln -sf "$cfg_main" "$cfg_agy"
  fi
  upsert_rule_section "$HOME/.gemini/GEMINI.md"
  upsert_rule_section "$HOME/.gemini/config/GEMINI.md"

  if [[ $DRY_RUN -eq 0 ]]; then
    local a_turn="$HOOK_BIN"
    [[ -n "$ENGRAM_BIN" ]] && a_turn="$ENGRAM_BIN hook antigravity turn"
    python3 - "$hooks_f" "$a_turn" <<'PY'
import json, os, sys
p, hook_bin = sys.argv[1], sys.argv[2]
os.makedirs(os.path.dirname(p), exist_ok=True)
d = {}
if os.path.exists(p) and os.path.getsize(p) > 0:
    with open(p) as f:
        d = json.load(f)
d.pop("mnemonic-auto-remember", None)
d["engram-auto-remember"] = {
    "enabled": True,
    "Stop": [
        {
            "type": "command",
            "command": hook_bin,
            "timeout": 10
        }
    ]
}
# (hook_bin = `engram hook antigravity turn` bila binary tersedia, selain itu skrip Python)
with open(p, "w") as f:
    json.dump(d, f, indent=2)
    f.write("\n")
PY
    info "Hook Stop dipasang: $hooks_f"
  fi
  info "Di Antigravity: panel Agent > ... > MCP Servers > Manage MCP Servers > Refresh."
}

do_gemini() {
  step "Gemini CLI (profil $(profile_for gemini))"
  local f="$HOME/.gemini/settings.json"
  if [[ $UNINSTALL -eq 1 ]]; then
    edit_json "$f" "$NAME" ""
    upsert_rule_section "$HOME/.gemini/GEMINI.md"
    if [[ $DRY_RUN -eq 0 && -f "$f" ]]; then
      python3 - "$f" <<'PY'
import json, os, sys
p = sys.argv[1]
d = json.load(open(p))
hooks = d.get("hooks", {})
for ev, nm in (("AfterAgent", "engram-auto-remember"), ("SessionStart", "engram-session-context")):
    if ev in hooks:
        hooks[ev] = [x for x in hooks[ev] if not any(h.get("name") == nm for h in x.get("hooks", []))]
json.dump(d, open(p, "w"), indent=2)
PY
    fi
    return
  fi

  ensure_hook_bin
  edit_json "$f" "$NAME" "$(entry_json gemini)"
  upsert_rule_section "$HOME/.gemini/GEMINI.md"

  if [[ $DRY_RUN -eq 0 ]]; then
    local g_turn="$HOOK_BIN" g_ctx="$CTX_BIN"
    if [[ -n "$ENGRAM_BIN" ]]; then g_turn="$ENGRAM_BIN hook gemini-cli turn"; g_ctx="$ENGRAM_BIN hook gemini-cli context"; fi
    python3 - "$f" "$g_turn" "$g_ctx" <<'PY'
import json, os, sys
p, hook_bin, ctx_bin = sys.argv[1], sys.argv[2], sys.argv[3]
d = json.load(open(p)) if os.path.exists(p) and os.path.getsize(p) > 0 else {}
hooks = d.setdefault("hooks", {})
def upsert(event, name, command, description):
    lst = hooks.setdefault(event, [])
    lst = [x for x in lst if not any(h.get("name") in (name, "mnemonic-auto-remember") for h in x.get("hooks", []))]
    lst.append({
        "matcher": "*",
        "hooks": [{"name": name, "type": "command", "command": command, "description": description, "timeout": 10000}],
    })
    hooks[event] = lst
upsert("AfterAgent", "engram-auto-remember", hook_bin, "Simpan percakapan otomatis ke engram")
upsert("SessionStart", "engram-session-context", ctx_bin, "Suntik konteks proyek dari engram saat sesi mulai")
with open(p, "w") as f:
    json.dump(d, f, indent=2)
    f.write("\n")
PY
    info "Hook AfterAgent & SessionStart dipasang: $f"
  fi
}

do_cursor() {
  step "Cursor (profil $(profile_for cursor))"
  local f="$HOME/.cursor/mcp.json"
  if [[ $UNINSTALL -eq 1 ]]; then edit_json "$f" "$NAME" ""; return; fi
  edit_json "$f" "$NAME" "$(entry_json cursor)"
  info "Di Cursor: Settings > MCP, pastikan '$NAME' aktif."
}

do_codex() {
  step "Codex CLI (profil $(profile_for codex))"
  local f="$HOME/.codex/config.toml"
  upsert_rule_section "$HOME/.codex/AGENTS.md" "$(profile_for codex)"
  edit_toml "$f" "$NAME"
  [[ $UNINSTALL -eq 1 ]] || info "Mulai sesi Codex baru; cek dengan /mcp."
}

do_windsurf() {
  step "Windsurf (profil $(profile_for windsurf))"
  local f="$HOME/.codeium/windsurf/mcp_config.json"
  upsert_rule_section "$HOME/.codeium/windsurf/memories/global_rules.md" "$(profile_for windsurf)"
  if [[ $UNINSTALL -eq 1 ]]; then edit_json "$f" "$NAME" ""; return; fi
  edit_json "$f" "$NAME" "$(entry_json windsurf)"
  info "Di Windsurf: panel Cascade > MCP Servers > Refresh."
}

do_claude() {
  step "Claude Code (profil $(profile_for claude))"
  local settings_f="$HOME/.claude/settings.json"
  upsert_rule_section "$HOME/.claude/CLAUDE.md" "$(profile_for claude)"

  if [[ $UNINSTALL -eq 1 ]]; then
    if command -v claude >/dev/null; then
      run claude mcp remove -s user "$NAME" >/dev/null 2>&1 || true
    run claude mcp remove -s user "memori" >/dev/null 2>&1 || true
    fi
    if [[ $DRY_RUN -eq 0 && -f "$settings_f" ]]; then
      python3 - "$settings_f" "$HOOK_BIN" "$CTX_BIN" <<'PY'
import json, os, sys
p, hook_bin, ctx_bin = sys.argv[1], sys.argv[2], sys.argv[3]
d = json.load(open(p))
hooks = d.get("hooks", {})
def ours(h):
    c = str(h.get("command", ""))
    return c in (hook_bin, ctx_bin) or " hook claude-code " in c or "mnemonic-auto-remember" in c
for ev in list(hooks):
    hooks[ev] = [x for x in hooks[ev] if not any(ours(h) for h in x.get("hooks", []))]
    if not hooks[ev]:
        del hooks[ev]
json.dump(d, open(p, "w"), indent=2)
PY
    fi
    info "Dihapus (scope user)."
    return
  fi

  ensure_hook_bin
  if command -v claude >/dev/null; then
    run claude mcp remove -s user "$NAME" >/dev/null 2>&1 || true
    run claude mcp remove -s user "memori" >/dev/null 2>&1 || true
    if [[ "$MODE" == "http" ]]; then
      run claude mcp add -s user --transport http "$NAME" "$(url_for claude)" --header "Authorization: Bearer $TOKEN"
    else
      run claude mcp add -s user "$NAME" -- "$BIN" --config "$CONFIG" mcp --profile "$(profile_for claude)"
    fi
  else
    info "CLI 'claude' tidak ditemukan, konfigurasi MCP dilewati."
  fi

  if [[ $DRY_RUN -eq 0 ]]; then
    python3 - "$settings_f" "$HOOK_BIN" "$CTX_BIN" "$ENGRAM_BIN" <<'PY'
import json, os, sys
p, hook_bin, ctx_bin, engram_bin = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
os.makedirs(os.path.dirname(p), exist_ok=True)
d = json.load(open(p)) if os.path.exists(p) and os.path.getsize(p) > 0 else {}
perms = d.setdefault("permissions", {}).setdefault("allow", [])
if "mcp__memori__*" in perms:
    perms.remove("mcp__memori__*")
if "mcp__engram__*" not in perms:
    perms.append("mcp__engram__*")
hooks = d.setdefault("hooks", {})
def ours(h):
    c = str(h.get("command", ""))
    return c in (hook_bin, ctx_bin) or " hook claude-code " in c or "mnemonic-auto-remember" in c
for ev in list(hooks):
    hooks[ev] = [x for x in hooks[ev] if not any(ours(h) for h in x.get("hooks", []))]
    if not hooks[ev]:
        del hooks[ev]
def upsert(event, command, timeout, matcher=None, async_=False):
    h = {"type": "command", "command": command, "timeout": timeout}
    if async_:
        h["async"] = True
    entry = {"hooks": [h]}
    if matcher:
        entry["matcher"] = matcher
    hooks.setdefault(event, []).append(entry)
if engram_bin:
    q = json.dumps(engram_bin) if " " in engram_bin else engram_bin
    cmd = lambda ev: f"{q} hook claude-code {ev}"
    upsert("SessionStart", cmd("context"), 5, "startup|resume|clear|compact")
    upsert("UserPromptSubmit", cmd("session-init"), 3)
    upsert("PreToolUse", cmd("file-context"), 3, "Read|Edit|MultiEdit|Write|NotebookEdit")
    upsert("PostToolUse", cmd("observation"), 10, "*", True)
    upsert("PostToolUseFailure", cmd("observation"), 10, "*", True)
    upsert("Stop", cmd("summarize"), 15, None, True)
    upsert("PreCompact", cmd("pre-compact"), 15)
    upsert("SessionEnd", cmd("session-end"), 15, None, True)
else:
    upsert("Stop", hook_bin, 10)
    upsert("SessionStart", ctx_bin, 10)
with open(p, "w") as f:
    json.dump(d, f, indent=2)
    f.write("\n")
PY
    if [[ -n "$ENGRAM_BIN" ]]; then
      info "Hook engram (SessionStart, UserPromptSubmit, PreToolUse, PostToolUse, Stop, PreCompact, SessionEnd) & izin MCP dipasang: $settings_f"
      info "  memakai binary: $ENGRAM_BIN"
    else
      info "Binary engram tidak ditemukan; hook Python cadangan (Stop, SessionStart) dipasang: $settings_f"
      info "  Pasang CLI (install.sh) lalu jalankan ulang skrip ini untuk observasi per tool."
    fi
  fi
}

# ---------- cek koneksi ----------

verify_http() {
  step "Cek koneksi ke $URL"
  local body n
  body="$(curl -s -m 5 -X POST "$URL" -H 'Content-Type: application/json' \
    -H "Authorization: Bearer $TOKEN" \
    -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' || true)"
  if [[ "$body" == *'"tools"'* ]]; then
    n="$(python3 -c 'import json,sys; print(len(json.loads(sys.argv[1])["result"]["tools"]))' "$body" 2>/dev/null || echo "?")"
    info "OK: server menjawab, $n tool tersedia."
  elif [[ -z "$body" ]]; then
    info "Server belum jalan. Buka aplikasi engram (atau 'engram serve'), lalu jalankan script ini lagi untuk cek."
  else
    info "Server jalan tapi token ditolak. Keluar dari aplikasi lewat ikon menu bar > Keluar,"
    info "lalu buka lagi agar aplikasi memakai MEMORY_TOKEN yang baru."
  fi
}

# ---------- utama ----------

URL=""
if [[ $UNINSTALL -eq 0 ]]; then
  if [[ "$MODE" == "http" ]]; then
    ensure_token
    URL="$(mcp_url)"; URL="${URL:-$DEFAULT_URL}"
    if [[ "$URL" != "$DEFAULT_URL" ]]; then
      info "Catatan: aplikasi sedang memakai $URL (port 7878 terpakai?). Alamat ini bisa berubah."
    fi
  else
    if [[ ! -x "$BIN" ]]; then
      step "Membangun binary engram"
      run cargo build --release -p memory-cli --manifest-path "$ROOT/Cargo.toml"
    fi
    [[ -f "$CONFIG" ]] || info "Peringatan: config $CONFIG belum ada."
    info "Mode stdio: tutup aplikasi desktop dulu sebelum agent memakai server ini."
  fi
fi

for c in "${CLIENTS[@]}"; do "do_$c"; done

if [[ $UNINSTALL -eq 1 ]]; then
  step "Token"
  info "Token & LaunchAgent dibiarkan (mungkin dipakai client lain). Untuk menghapus:"
  info "  launchctl bootout gui/$(id -u)/$AGENT_LABEL; rm \"$AGENT_PLIST\" \"$TOKEN_FILE\"; launchctl unsetenv MEMORY_TOKEN"
elif [[ "$MODE" == "http" && $DRY_RUN -eq 0 ]]; then
  verify_http
  info "Untuk 'engram serve' dari terminal: export MEMORY_TOKEN=\$(cat \"$TOKEN_FILE\")"
fi

printf '\nSelesai.\n'
