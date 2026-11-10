#!/usr/bin/env python3
"""
Hook SessionStart: menyuntik konteks proyek dari engram ke awal sesi agent
(Claude Code dan Gemini CLI), supaya keputusan arsitektur, konvensi, dan akar
masalah bug dari sesi sebelumnya langsung tersedia tanpa agent harus memanggil
tool dulu.

Cara kerja:
1. Membaca payload JSON dari stdin (kontrak SessionStart: `cwd`, `source`, ...).
2. Menentukan nama proyek dari folder kerja (akar git bila ada, selain itu `cwd`).
3. Memanggil GET /api/subjects/{subjek}/context/inject (indeks ber-ID, sama dengan
   `engram hook claude-code context`); bila endpoint itu tidak ada (server lama),
   jatuh ke tool MCP `project_context`. Alamat & token dari connection.json atau
   MEMORY_TOKEN. Skrip ini cadangan bila binary `engram` tidak terpasang.
4. Menulis `hookSpecificOutput.additionalContext` ke stdout. Bila server mati,
   proyek tidak dikenal, atau belum ada catatan: keluar diam-diam dengan `{}`.
5. Selalu exit 0 agar tidak pernah menghambat agent.

Argumen opsional: --max-chars N (bawaan 6000) membatasi panjang konteks.
"""

import json
import os
import subprocess
import sys
import urllib.parse
import urllib.request
from pathlib import Path

DEFAULT_MAX_CHARS = 6000


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


def load_connection() -> tuple[str, str]:
    data_dir = resolve_data_dir()
    mcp_url = "http://127.0.0.1:7878/mcp"
    token = os.environ.get("MEMORY_TOKEN", "").strip()
    conn_file = data_dir / "connection.json"
    if conn_file.exists():
        try:
            c = json.loads(conn_file.read_text(encoding="utf-8"))
            mcp_url = c.get("mcp_url") or mcp_url
            token = c.get("token") or token
        except Exception:
            pass
    if not token:
        token_file = data_dir / "mcp_token"
        if token_file.exists():
            try:
                token = token_file.read_text(encoding="utf-8").strip()
            except Exception:
                pass
    return mcp_url, token


def project_name(payload: dict) -> str:
    cwd = (payload.get("cwd") or payload.get("workspaceRoot") or os.getcwd() or "").strip()
    if not cwd:
        return ""
    root = cwd
    try:
        out = subprocess.run(
            ["git", "-C", cwd, "rev-parse", "--show-toplevel"],
            capture_output=True, text=True, timeout=2,
        )
        if out.returncode == 0 and out.stdout.strip():
            root = out.stdout.strip()
    except Exception:
        pass
    try:
        resolved = Path(root).expanduser().resolve()
    except Exception:
        return ""
    home = Path.home().resolve()
    if resolved in (home, Path("/")) or resolved.parent == Path("/"):
        return ""
    name = resolved.name.strip()
    if not name or name.startswith("."):
        return ""
    return name


def _headers(token: str) -> dict:
    headers = {"Content-Type": "application/json", "Accept": "application/json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    return headers


def fetch_inject(project: str, client: str, max_chars: int) -> str:
    """Jalur utama: indeks ber-ID dari GET /api/subjects/{s}/context/inject
    (sama dengan `engram hook claude-code context`), sudah dipotong ke max_chars
    di sisi server."""
    mcp_url, token = load_connection()
    base = mcp_url.rsplit("/mcp", 1)[0]
    headers = _headers(token)
    req = urllib.request.Request(f"{base}/api/settings", headers=headers)
    with urllib.request.urlopen(req, timeout=5) as resp:
        settings = json.loads(resp.read().decode("utf-8"))
    subject = ((settings.get("values") or {}).get("vault") or {}).get("default_subject") or ""
    if not subject:
        return ""
    q = urllib.parse.urlencode({"project": project, "client": client, "max_chars": str(max_chars)})
    req = urllib.request.Request(f"{base}/api/subjects/{urllib.parse.quote(subject)}/context/inject?{q}", headers=headers)
    with urllib.request.urlopen(req, timeout=5) as resp:
        data = json.loads(resp.read().decode("utf-8"))
    if not data.get("found"):
        return ""
    return (data.get("text") or "").strip()


def fetch_context(project: str, client: str) -> str:
    """Jalur cadangan: teks `project_context` lewat MCP (server lama tanpa endpoint inject)."""
    mcp_url, token = load_connection()
    headers = _headers(token)
    body = json.dumps(
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "project_context",
                "arguments": {"project": project, "task": f"awal sesi {client}"},
            },
        }
    ).encode("utf-8")
    req = urllib.request.Request(mcp_url, data=body, headers=headers, method="POST")
    with urllib.request.urlopen(req, timeout=5) as resp:
        data = json.loads(resp.read().decode("utf-8"))
    result = data.get("result") or {}
    if result.get("isError"):
        return ""
    structured = result.get("structuredContent") or {}
    if structured and structured.get("found") is False:
        return ""
    for item in result.get("content") or []:
        if item.get("type") == "text" and item.get("text", "").strip():
            return item["text"].strip()
    return ""


def main() -> None:
    max_chars = DEFAULT_MAX_CHARS
    args = sys.argv[1:]
    if "--max-chars" in args:
        try:
            max_chars = int(args[args.index("--max-chars") + 1])
        except Exception:
            pass

    try:
        raw = sys.stdin.read()
        payload = json.loads(raw) if raw.strip() else {}
    except Exception:
        payload = {}

    event = payload.get("hook_event_name") or "SessionStart"
    client = "gemini-cli" if "session_id" in payload and "prompt" in payload else "claude-code"

    context = ""
    try:
        project = project_name(payload)
        if project:
            try:
                context = fetch_inject(project, client, max_chars)
            except Exception:
                context = ""
            if not context:
                context = fetch_context(project, client)
                if len(context) > max_chars:
                    context = context[:max_chars].rstrip() + "\n[... dipotong; panggil project_context untuk selengkapnya]"
                if context:
                    context = (
                        f"Konteks proyek '{project}' dari engram (memori lintas-tool). "
                        "Fakta berlabel 'berlaku sekarang' vs 'dulu'; jangan sajikan fakta lama sebagai kondisi sekarang. "
                        "Simpan keputusan baru lewat tool `remember`.\n\n" + context
                    )
    except Exception:
        context = ""

    if context:
        out = {"hookSpecificOutput": {"hookEventName": event, "additionalContext": context}}
    else:
        out = {}
    sys.stdout.write(json.dumps(out, ensure_ascii=False) + "\n")
    sys.stdout.flush()


if __name__ == "__main__":
    main()
