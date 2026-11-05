#!/bin/sh
# Buat memory.toml pertama kali dari contoh (vault di /data/vault), lalu jalankan engram.
set -eu
if [ ! -f "$MEMORY_CONFIG" ]; then
  sed 's#^path = "vault"#path = "/data/vault"#' /data/memory.toml.example > "$MEMORY_CONFIG"
  echo "engram: membuat $MEMORY_CONFIG dari contoh; sesuaikan [llm] dan [embedding]." >&2
fi
exec engram "$@"
