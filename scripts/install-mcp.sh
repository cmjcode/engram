#!/usr/bin/env bash
# Pasang server MCP engram ke agent AI (Antigravity, Claude Code, Cursor).
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
#   scripts/install-mcp.sh --uninstall --client all
set -euo pipefail

NAME="engram"
CLIENT="antigravity"
MODE="http"
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

  --client NAMA   antigravity | gemini | claude | cursor | all   (bawaan: antigravity)
  --mode MODE     http | stdio                          (bawaan: http)
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
  antigravity|gemini|claude|cursor) CLIENTS=("$CLIENT") ;;
  all) CLIENTS=(antigravity gemini claude cursor) ;;
  *) echo "Client tidak dikenal: $CLIENT" >&2; exit 2 ;;
esac
case "$MODE" in http|stdio) ;; *) echo "Mode tidak dikenal: $MODE" >&2; exit 2 ;; esac

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

# entry_json STYLE -> JSON entri server untuk gaya client (antigravity | gemini | cursor)
entry_json() {
  python3 - "$1" "$MODE" "$URL" "$TOKEN" "$BIN" "$CONFIG" <<'PY'
import json, sys
style, mode, url, token, bin_, cfg = sys.argv[1:]
if mode == "http":
    if style == "antigravity":
        e = {"serverUrl": url, "headers": {"Authorization": f"Bearer {token}"}, "disabled": False}
    elif style == "gemini":
        e = {"httpUrl": url, "headers": {"Authorization": f"Bearer {token}"}, "trust": True}
    else:
        e = {"url": url, "headers": {"Authorization": f"Bearer {token}"}}
else:
    e = {"command": bin_, "args": ["--config", cfg, "mcp"]}
    if style == "antigravity":
        e["disabled"] = False
    elif style == "gemini":
        e["trust"] = True
print(json.dumps(e))
PY
}

HOOK_SRC="$ROOT/scripts/auto-remember-hook.py"
HOOK_BIN="$HOME/.local/bin/engram-auto-remember"

ensure_hook_bin() {
  if [[ $DRY_RUN -eq 1 ]]; then
    info "[dry-run] pasang $HOOK_BIN"
    return
  fi
  mkdir -p "$(dirname "$HOOK_BIN")"
  cp "$HOOK_SRC" "$HOOK_BIN"
  chmod 755 "$HOOK_BIN"
  rm -f "$HOME/.local/bin/mnemonic-auto-remember"
}

upsert_rule_section() {
  local file="$1"
  if [[ $DRY_RUN -eq 1 ]]; then
    info "[dry-run] perbarui rule engram di $file"
    return
  fi
  mkdir -p "$(dirname "$file")"
  python3 - "$file" "$UNINSTALL" <<'PY'
import os, re, sys
path, uninstall = sys.argv[1], sys.argv[2] == "1"
marker_start = "<!-- ENGRAM_START -->"
marker_end = "<!-- ENGRAM_END -->"
legacy_start = "<!-- MNEMONIC_MEMORY_START -->"
legacy_end = "<!-- MNEMONIC_MEMORY_END -->"
block = f"""{marker_start}
## Engram (`engram` MCP)

- **Simpan Otomatis (`remember`)**: Saat pengguna menceritakan kisah hidup, fakta pribadi/keluarga, pengalaman masa lalu, preferensi, atau keputusan penting (termasuk keputusan arsitektur/teknis di project), panggil tool MCP `remember` (dari server `engram`) untuk menyimpannya tanpa perlu diminta. Isi parameter `source` dengan format `<client>:<nama-project>` (misal `antigravity:LLM-MEMORY`) dan sebutkan nama project di dalam `text` bila konteksnya spesifik pada project tersebut.
- **Ambil Konteks (`recall`, `who_is`, `timeline`, `principles`, `family_tree`)**: Saat pengguna bertanya tentang masa lalu, anggota keluarga, relasi seseorang, kronologi peristiwa, prinsip hidup, atau konteks project sebelumnya, panggil tool MCP `engram` yang relevan terlebih dahulu sebelum menjawab. Untuk mencari memori spesifik project saat ini, sertakan nama project di `query` pada `recall` atau `entity` pada `timeline`. Selalu bedakan fakta yang masih berlaku sekarang dengan riwayat masa lalu.
{marker_end}"""

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
  step "Antigravity"
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
    python3 - "$hooks_f" "$HOOK_BIN" <<'PY'
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
with open(p, "w") as f:
    json.dump(d, f, indent=2)
    f.write("\n")
PY
    info "Hook Stop dipasang: $hooks_f"
  fi
  info "Di Antigravity: panel Agent > ... > MCP Servers > Manage MCP Servers > Refresh."
}

do_gemini() {
  step "Gemini CLI"
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
if "AfterAgent" in hooks:
    hooks["AfterAgent"] = [x for x in hooks["AfterAgent"] if not any(h.get("name") == "engram-auto-remember" for h in x.get("hooks", []))]
json.dump(d, open(p, "w"), indent=2)
PY
    fi
    return
  fi

  ensure_hook_bin
  edit_json "$f" "$NAME" "$(entry_json gemini)"
  upsert_rule_section "$HOME/.gemini/GEMINI.md"

  if [[ $DRY_RUN -eq 0 ]]; then
    python3 - "$f" "$HOOK_BIN" <<'PY'
import json, os, sys
p, hook_bin = sys.argv[1], sys.argv[2]
d = json.load(open(p)) if os.path.exists(p) and os.path.getsize(p) > 0 else {}
hooks = d.setdefault("hooks", {})
after_agent = hooks.setdefault("AfterAgent", [])
after_agent = [x for x in after_agent if not any(h.get("name") in ("engram-auto-remember", "mnemonic-auto-remember") for h in x.get("hooks", []))]
after_agent.append({
    "matcher": "*",
    "hooks": [
        {
            "name": "engram-auto-remember",
            "type": "command",
            "command": hook_bin,
            "description": "Simpan percakapan otomatis ke engram",
            "timeout": 10000
        }
    ]
})
hooks["AfterAgent"] = after_agent
with open(p, "w") as f:
    json.dump(d, f, indent=2)
    f.write("\n")
PY
    info "Hook AfterAgent dipasang: $f"
  fi
}

do_cursor() {
  step "Cursor"
  local f="$HOME/.cursor/mcp.json"
  if [[ $UNINSTALL -eq 1 ]]; then edit_json "$f" "$NAME" ""; return; fi
  edit_json "$f" "$NAME" "$(entry_json cursor)"
}

do_claude() {
  step "Claude Code"
  local settings_f="$HOME/.claude/settings.json"
  upsert_rule_section "$HOME/.claude/CLAUDE.md"

  if [[ $UNINSTALL -eq 1 ]]; then
    if command -v claude >/dev/null; then
      run claude mcp remove -s user "$NAME" >/dev/null 2>&1 || true
    run claude mcp remove -s user "memori" >/dev/null 2>&1 || true
    fi
    if [[ $DRY_RUN -eq 0 && -f "$settings_f" ]]; then
      python3 - "$settings_f" "$HOOK_BIN" <<'PY'
import json, os, sys
p, hook_bin = sys.argv[1], sys.argv[2]
d = json.load(open(p))
hooks = d.get("hooks", {})
if "Stop" in hooks:
    hooks["Stop"] = [x for x in hooks["Stop"] if not any(h.get("command") == hook_bin for h in x.get("hooks", []))]
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
      run claude mcp add -s user --transport http "$NAME" "$URL" --header "Authorization: Bearer $TOKEN"
    else
      run claude mcp add -s user "$NAME" -- "$BIN" --config "$CONFIG" mcp
    fi
  else
    info "CLI 'claude' tidak ditemukan, konfigurasi MCP dilewati."
  fi

  if [[ $DRY_RUN -eq 0 ]]; then
    python3 - "$settings_f" "$HOOK_BIN" <<'PY'
import json, os, sys
p, hook_bin = sys.argv[1], sys.argv[2]
os.makedirs(os.path.dirname(p), exist_ok=True)
d = json.load(open(p)) if os.path.exists(p) and os.path.getsize(p) > 0 else {}
perms = d.setdefault("permissions", {}).setdefault("allow", [])
if "mcp__memori__*" in perms:
    perms.remove("mcp__memori__*")
if "mcp__engram__*" not in perms:
    perms.append("mcp__engram__*")
hooks = d.setdefault("hooks", {})
stop_hooks = hooks.setdefault("Stop", [])
stop_hooks = [x for x in stop_hooks if not any(h.get("command") == hook_bin or "mnemonic-auto-remember" in str(h.get("command", "")) for h in x.get("hooks", []))]
stop_hooks.append({
    "hooks": [
        {
            "type": "command",
            "command": hook_bin,
            "timeout": 10
        }
    ]
})
hooks["Stop"] = stop_hooks
with open(p, "w") as f:
    json.dump(d, f, indent=2)
    f.write("\n")
PY
    info "Hook Stop & izin MCP dipasang: $settings_f"
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
