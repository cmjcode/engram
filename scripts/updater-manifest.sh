#!/usr/bin/env bash
# Susun latest.json (manifest tauri-plugin-updater) dari aset GitHub Release
# lalu unggah ke release yang sama.
#
#   bash scripts/updater-manifest.sh v0.0.3 [owner/repo]
#
# Setiap paket update punya berkas <paket>.sig di release. Script ini dipanggil
# oleh CI (Windows/Linux) dan build.sh (macOS); siapa pun yang terakhir selesai
# menghasilkan manifest lengkap. Aman dijalankan ulang kapan saja.
set -euo pipefail

TAG="${1:?pemakaian: updater-manifest.sh <tag> [owner/repo]}"
REPO="${2:-${GITHUB_REPOSITORY:-cmjcode/engram}}"
VERSION="${TAG#v}"

command -v jq >/dev/null || { echo "jq belum terpasang" >&2; exit 1; }

# Retry perintah gh (mengatasi HTTP 502/503 sementara dari GitHub API, terutama
# saat --clobber menghapus latest.json lama sebelum mengunggah yang baru).
retry() {
  local n=1 max=5 delay=3
  until "$@"; do
    if (( n >= max )); then
      return 1
    fi
    echo "gagal (percobaan $n/$max), coba lagi dalam ${delay}d..." >&2
    sleep "$delay"
    (( n++ ))
    (( delay *= 2 ))
  done
}

RELEASE="$(retry gh release view "$TAG" -R "$REPO" --json assets,body)"
NOTES="$(jq -r '.body // ""' <<<"$RELEASE")"

# nama paket -> kunci platform (dipisah spasi). Plugin mencari
# "<os>-<arch>-<installer>" dulu, lalu "<os>-<arch>".
platforms_for() {
  case "$1" in
    *_aarch64.app.tar.gz) echo "darwin-aarch64 darwin-aarch64-app" ;;
    *_x64.app.tar.gz)     echo "darwin-x86_64 darwin-x86_64-app" ;;
    *_x64-setup.exe)      echo "windows-x86_64 windows-x86_64-nsis" ;;
    *_x64_*.msi)          echo "windows-x86_64-msi" ;;
    *_amd64.AppImage)     echo "linux-x86_64 linux-x86_64-appimage" ;;
    *_amd64.deb)          echo "linux-x86_64-deb" ;;
    *.x86_64.rpm)         echo "linux-x86_64-rpm" ;;
    *)                    echo "" ;;
  esac
}

PLATFORMS='{}'
while IFS= read -r sig_name; do
  pkg="${sig_name%.sig}"
  keys="$(platforms_for "$pkg")"
  if [ -z "$keys" ]; then
    echo "lewati $sig_name (platform tidak dikenal)" >&2
    continue
  fi
  url="$(jq -r --arg n "$pkg" '.assets[] | select(.name == $n) | .url' <<<"$RELEASE")"
  if [ -z "$url" ]; then
    echo "lewati $sig_name (paket $pkg tidak ada di release)" >&2
    continue
  fi
  signature="$(retry gh release download "$TAG" -R "$REPO" -p "$sig_name" -O -)"
  for key in $keys; do
    PLATFORMS="$(jq --arg k "$key" --arg u "$url" --arg s "$signature" \
      '. + {($k): {url: $u, signature: $s}}' <<<"$PLATFORMS")"
  done
  echo "$pkg -> $keys" >&2
done < <(jq -r '.assets[].name | select(endswith(".sig"))' <<<"$RELEASE")

if [ "$PLATFORMS" = '{}' ]; then
  echo "Belum ada paket update (.sig) di $TAG; latest.json tidak dibuat." >&2
  exit 0
fi

OUT="$(mktemp -d)/latest.json"
jq -n --arg v "$VERSION" --arg n "$NOTES" --arg d "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --argjson p "$PLATFORMS" '{version: $v, notes: $n, pub_date: $d, platforms: $p}' >"$OUT"
retry gh release upload "$TAG" "$OUT" -R "$REPO" --clobber
echo "latest.json diunggah ke $TAG ($(jq -r '.platforms | keys | join(", ")' "$OUT"))" >&2
