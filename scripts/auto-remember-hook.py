#!/usr/bin/env python3
"""
Hook otomatis untuk menyimpan percakapan dari Antigravity, Gemini CLI, dan Claude Code
ke platform engram.

Cara kerja:
1. Membaca payload JSON dari stdin (kontrak Stop / AfterAgent).
2. Mengekstrak pesan terakhir Pengguna & Asisten dari payload atau file transkrip JSONL.
3. Mengirimkannya ke endpoint MCP `remember` (http://127.0.0.1:7878/mcp).
4. Jika server sedang mati, menyimpan berkas .md ke folder `vault/inbox/` agar
   otomatis diimpor saat aplikasi dibuka kembali.
5. Selalu mengeluarkan output JSON yang valid ke stdout dan exit 0 agar tidak
   pernah menghambat jalannya agent.
"""

import datetime
import hashlib
import json
import os
import re
import sys
import urllib.error
import urllib.request
from pathlib import Path


def clean_user_text(raw: str) -> str:
    if not raw:
        return ""
    m = re.search(r"<USER_REQUEST>\s*(.*?)\s*</USER_REQUEST>", raw, flags=re.DOTALL)
    if m:
        return m.group(1).strip()
    # Bersihkan tag metadata sistem bila ada
    cleaned = re.sub(r"<ADDITIONAL_METADATA>.*?</ADDITIONAL_METADATA>", "", raw, flags=re.DOTALL)
    cleaned = re.sub(r"<USER_SETTINGS_CHANGE>.*?</USER_SETTINGS_CHANGE>", "", cleaned, flags=re.DOTALL)
    return cleaned.strip()


def extract_antigravity_turn(payload: dict) -> tuple[str, str]:
    user_text = clean_user_text(payload.get("lastUserInput") or "")
    assistant_text = ""

    tpath = payload.get("transcriptPath") or ""
    if tpath:
        p = Path(tpath)
        full_p = p.with_name("transcript_full.jsonl")
        target = full_p if full_p.exists() else p
        if target.exists():
            try:
                lines = target.read_text(encoding="utf-8", errors="replace").splitlines()
                for line in reversed(lines):
                    if not line.strip():
                        continue
                    obj = json.loads(line)
                    stype = obj.get("type")
                    content = (obj.get("content") or "").strip()
                    if not assistant_text and stype == "PLANNER_RESPONSE" and content:
                        assistant_text = content
                    elif not user_text and stype == "USER_INPUT" and content:
                        user_text = clean_user_text(content)
                    if user_text and assistant_text:
                        break
            except Exception:
                pass

    return user_text, assistant_text


def extract_claude_content(content) -> str:
    if isinstance(content, str):
        return content.strip()
    if isinstance(content, list):
        parts = []
        for item in content:
            if isinstance(item, dict) and item.get("type") == "text":
                txt = (item.get("text") or "").strip()
                if txt:
                    parts.append(txt)
        return "\n".join(parts).strip()
    return ""


def extract_claude_turn(payload: dict) -> tuple[str, str]:
    user_text = ""
    assistant_text = (payload.get("last_assistant_message") or "").strip()

    tpath = payload.get("transcript_path") or ""
    if tpath and os.path.exists(tpath):
        try:
            with open(tpath, "r", encoding="utf-8", errors="replace") as f:
                lines = f.read().splitlines()
            for line in reversed(lines):
                if not line.strip():
                    continue
                obj = json.loads(line)
                mtype = obj.get("type")
                msg = obj.get("message") or {}
                content = msg.get("content")
                if not assistant_text and mtype == "assistant":
                    txt = extract_claude_content(content)
                    if txt:
                        assistant_text = txt
                elif not user_text and mtype == "user":
                    # Lewati tool_result
                    if isinstance(content, list) and any(
                        isinstance(x, dict) and x.get("type") == "tool_result" for x in content
                    ):
                        continue
                    txt = extract_claude_content(content)
                    if txt:
                        user_text = clean_user_text(txt)
                if user_text and assistant_text:
                    break
        except Exception:
            pass

    return user_text, assistant_text


def extract_gemini_turn(payload: dict) -> tuple[str, str]:
    user_text = clean_user_text(payload.get("prompt") or "")
    assistant_text = (payload.get("prompt_response") or "").strip()
    return user_text, assistant_text


def extract_project_name(payload: dict) -> str:
    home = Path.home().resolve()
    ignored_dirs = {
        home,
        (home / ".gemini").resolve(),
        (home / ".gemini" / "config").resolve(),
        (home / ".claude").resolve(),
    }

    candidates: list[str] = []
    ws_paths = payload.get("workspacePaths")
    if isinstance(ws_paths, list):
        for p in ws_paths:
            if isinstance(p, str) and p.strip():
                candidates.append(p.strip())

    for key in ("cwd", "workspaceRoot", "projectDir"):
        val = payload.get(key)
        if isinstance(val, str) and val.strip():
            candidates.append(val.strip())

    for raw_path in candidates:
        try:
            resolved = Path(raw_path).expanduser().resolve()
            if resolved in ignored_dirs or resolved.parent == Path("/"):
                continue
            name = resolved.name.strip()
            if name and not name.startswith("."):
                return name
        except Exception:
            continue

    return ""


def resolve_data_dir() -> Path:
    if os.environ.get("ENGRAM_DATA_DIR"):
        return Path(os.environ["ENGRAM_DATA_DIR"])
    home = Path.home()
    if sys.platform == "darwin":
        return home / "Library" / "Application Support" / "id.engram.ai"
    if sys.platform == "win32":
        appdata = os.environ.get("APPDATA")
        return (Path(appdata) if appdata else home) / "id.engram.ai"
    xdg_data = os.environ.get("XDG_DATA_HOME")
    base = Path(xdg_data) if xdg_data else (home / ".local" / "share")
    return base / "id.engram.ai"


def load_connection() -> tuple[str, str, Path]:
    data_dir = resolve_data_dir()
    conn_file = data_dir / "connection.json"
    token_file = data_dir / "mcp_token"

    mcp_url = "http://127.0.0.1:7878/mcp"
    token = os.environ.get("MEMORY_TOKEN", "").strip()
    vault_dir = data_dir / "vault"

    if conn_file.exists():
        try:
            c = json.loads(conn_file.read_text(encoding="utf-8"))
            mcp_url = c.get("mcp_url") or mcp_url
            token = c.get("token") or token
            if c.get("vault"):
                vault_dir = Path(c["vault"])
        except Exception:
            pass

    if not token and token_file.exists():
        try:
            token = token_file.read_text(encoding="utf-8").strip()
        except Exception:
            pass

    return mcp_url, token, vault_dir


def is_duplicate(turn_hash: str) -> bool:
    cache_dir = Path.home() / ".cache" / "engram"
    cache_file = cache_dir / "seen_turns.txt"
    try:
        cache_dir.mkdir(parents=True, exist_ok=True)
        seen = set()
        if cache_file.exists():
            seen = set(cache_file.read_text(encoding="utf-8").splitlines()[-500:])
        if turn_hash in seen:
            return True
        with open(cache_file, "a", encoding="utf-8") as f:
            f.write(turn_hash + "\n")
    except Exception:
        pass
    return False


def send_to_memory(text: str, source: str) -> None:
    mcp_url, token, vault_dir = load_connection()
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"

    rpc_body = json.dumps(
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "remember",
                "arguments": {
                    "text": text,
                    "source": source,
                },
            },
        }
    ).encode("utf-8")

    req = urllib.request.Request(mcp_url, data=rpc_body, headers=headers, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=3) as resp:
            if 200 <= resp.status < 300:
                return
    except Exception:
        pass

    # Fallback: simpan ke vault/inbox/ bila server sedang tidak aktif
    try:
        inbox_dir = vault_dir / "inbox"
        inbox_dir.mkdir(parents=True, exist_ok=True)
        ts = datetime.datetime.now().strftime("%Y%m%d-%H%M%S")
        short_hash = hashlib.sha256(text.encode("utf-8")).hexdigest()[:8]
        safe_source = re.sub(r"[^A-Za-z0-9._-]+", "-", source).strip("-") or "chat"
        fname = inbox_dir / f"chat-{safe_source}-{ts}-{short_hash}.md"
        fname.write_text(text + "\n", encoding="utf-8")
    except Exception:
        pass


def main() -> None:
    raw_in = ""
    try:
        raw_in = sys.stdin.read()
        payload = json.loads(raw_in) if raw_in.strip() else {}
    except Exception:
        payload = {}

    is_antigravity = "conversationId" in payload or "terminationReason" in payload
    event_name = payload.get("hook_event_name") or ""

    try:
        if is_antigravity:
            client = "antigravity"
            user_text, assistant_text = extract_antigravity_turn(payload)
        elif event_name == "AfterAgent" or "prompt_response" in payload:
            client = "gemini-cli"
            user_text, assistant_text = extract_gemini_turn(payload)
        else:
            client = "claude-code"
            user_text, assistant_text = extract_claude_turn(payload)

        project = extract_project_name(payload)
        source = f"{client}:{project}" if project else client

        if user_text and len(user_text.strip()) >= 10:
            now_str = datetime.datetime.now().strftime("%Y-%m-%d %H:%M")
            if project:
                header = f"[{client} | project: {project} | {now_str}]"
            else:
                header = f"[{client} | {now_str}]"
            combined = f"{header}\nPengguna: {user_text.strip()}"
            if assistant_text:
                # Batasi panjang balasan asisten agar ringkas dan fokus pada konteks
                trimmed_ast = assistant_text.strip()
                if len(trimmed_ast) > 2000:
                    trimmed_ast = trimmed_ast[:2000] + "..."
                combined += f"\n\nAsisten: {trimmed_ast}"

            h = hashlib.sha256(f"{source}:{user_text.strip()}".encode("utf-8")).hexdigest()
            if not is_duplicate(h):
                send_to_memory(combined, source)
    except Exception:
        pass

    # Output sesuai kontrak masing-masing client
    if is_antigravity:
        sys.stdout.write(json.dumps({"decision": "allow"}) + "\n")
    else:
        sys.stdout.write("{}\n")
    sys.stdout.flush()


if __name__ == "__main__":
    main()
