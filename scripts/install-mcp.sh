#!/usr/bin/env bash
# Pasang server MCP Memori Hidup ke agent AI (Antigravity, Claude Code, Cursor).
#
# Mode http (bawaan): agent tersambung ke aplikasi desktop / `memory serve`
#   lewat http://127.0.0.1:7878/mcp dengan token tetap (MEMORY_TOKEN).
# Mode stdio: agent menjalankan binary `memory ... mcp` sendiri.
#   Hanya bisa dipakai saat aplikasi desktop TIDAK jalan (vault dikunci satu proses).
#
# Contoh:
#   scripts/install-mcp.sh                       # Antigravity, mode http
#   scripts/install-mcp.sh --client all
#   scripts/install-mcp.sh --client claude --mode stdio
#   scripts/install-mcp.sh --uninstall --client all
set -euo pipefail

NAME="memori"
CLIENT="antigravity"
MODE="http"
TOKEN=""
UNINSTALL=0
DRY_RUN=0

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DATA_DIR="$HOME/Library/Application Support/id.memorihidup.desktop"
CONFIG="$DATA_DIR/memory.toml"
BIN="$ROOT/target/release/memory"
TOKEN_FILE="$DATA_DIR/mcp_token"
AGENT_LABEL="id.memorihidup.token"
AGENT_PLIST="$HOME/Library/LaunchAgents/$AGENT_LABEL.plist"
DEFAULT_URL="http://127.0.0.1:7878/mcp"

usage() {
  cat <<EOF
Pemakaian: $(basename "$0") [opsi]

  --client NAMA   antigravity | claude | cursor | all   (bawaan: antigravity)
  --mode MODE     http | stdio                          (bawaan: http)
  --name NAMA     nama server MCP                       (bawaan: memori)
  --token TOKEN   pakai token ini (mode http); bila kosong, pakai yang ada atau buat baru
  --config PATH   memory.toml untuk mode stdio          (bawaan: config aplikasi desktop)
  --bin PATH      binary memory untuk mode stdio        (bawaan: target/release/memory)
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
  antigravity|claude|cursor) CLIENTS=("$CLIENT") ;;
  all) CLIENTS=(antigravity claude cursor) ;;
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
path, name, entry = sys.argv[1], sys.argv[2], sys.argv[3]
data = {}
if os.path.exists(path) and os.path.getsize(path) > 0:
    with open(path) as f:
        data = json.load(f)
servers = data.setdefault("mcpServers", {})
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

# entry_json STYLE -> JSON entri server untuk gaya client (antigravity | cursor)
entry_json() {
  python3 - "$1" "$MODE" "$URL" "$TOKEN" "$BIN" "$CONFIG" <<'PY'
import json, sys
style, mode, url, token, bin_, cfg = sys.argv[1:]
if mode == "http":
    key = "serverUrl" if style == "antigravity" else "url"
    e = {key: url, "headers": {"Authorization": f"Bearer {token}"}}
else:
    e = {"command": bin_, "args": ["--config", cfg, "mcp"]}
if style == "antigravity":
    e["disabled"] = False
print(json.dumps(e))
PY
}

# ---------- per client ----------

do_antigravity() {
  step "Antigravity"
  local f="$HOME/.gemini/antigravity/mcp_config.json"
  if [[ $UNINSTALL -eq 1 ]]; then edit_json "$f" "$NAME" ""; return; fi
  edit_json "$f" "$NAME" "$(entry_json antigravity)"
  info "Di Antigravity: panel Agent > ... > MCP Servers > Manage MCP Servers > Refresh."
}

do_cursor() {
  step "Cursor"
  local f="$HOME/.cursor/mcp.json"
  if [[ $UNINSTALL -eq 1 ]]; then edit_json "$f" "$NAME" ""; return; fi
  edit_json "$f" "$NAME" "$(entry_json cursor)"
}

do_claude() {
  step "Claude Code"
  if ! command -v claude >/dev/null; then info "CLI 'claude' tidak ditemukan, dilewati."; return; fi
  run claude mcp remove -s user "$NAME" >/dev/null 2>&1 || true
  if [[ $UNINSTALL -eq 1 ]]; then info "Dihapus (scope user)."; return; fi
  if [[ "$MODE" == "http" ]]; then
    run claude mcp add -s user --transport http "$NAME" "$URL" --header "Authorization: Bearer $TOKEN"
  else
    run claude mcp add -s user "$NAME" -- "$BIN" --config "$CONFIG" mcp
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
    info "Server belum jalan. Buka aplikasi Memori Hidup (atau 'memory serve'), lalu jalankan script ini lagi untuk cek."
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
      step "Membangun binary memory"
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
  info "Untuk 'memory serve' dari terminal: export MEMORY_TOKEN=\$(cat \"$TOKEN_FILE\")"
fi

printf '\nSelesai.\n'
