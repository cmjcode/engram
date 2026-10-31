#!/bin/sh
# Installer mnemonic-memory: CLI `memory` + aplikasi desktop.
#
#   curl -fsSL https://raw.githubusercontent.com/cmjcode/mnemonic-memory/main/install.sh | sh
#
# Opsi (lewat `sh -s -- <opsi>`):
#   curl -fsSL .../install.sh | sh -s -- --cli-only
#   curl -fsSL .../install.sh | sh -s -- --desktop-only --version 0.0.1
#
#   --cli-only        pasang CLI saja
#   --desktop-only    pasang aplikasi desktop saja
#   --version X.Y.Z   versi tertentu (bawaan: rilis terbaru)
#   --bin-dir PATH    folder CLI (bawaan: ~/.local/bin)
#   --appimage        Linux: pakai AppImage walau ada dpkg/rpm
#
# Variabel lingkungan: MNEMONIC_VERSION, MNEMONIC_BIN_DIR, GITHUB_TOKEN (opsional,
# menghindari batas rate API GitHub).
set -eu

REPO="cmjcode/mnemonic-memory"
APP_NAME="mnemonic-memory"
CLI_NAME="memory"

WANT_CLI=1
WANT_DESKTOP=1
FORCE_APPIMAGE=0
INSTALLED=""
VERSION="${MNEMONIC_VERSION:-}"
BIN_DIR="${MNEMONIC_BIN_DIR:-$HOME/.local/bin}"

info() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mPeringatan:\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31mGagal:\033[0m %s\n' "$*" >&2; exit 1; }
has()  { command -v "$1" >/dev/null 2>&1; }

usage() { cat <<'EOF'
Installer mnemonic-memory: CLI `memory` + aplikasi desktop.

  curl -fsSL https://raw.githubusercontent.com/cmjcode/mnemonic-memory/main/install.sh | sh
  curl -fsSL .../install.sh | sh -s -- [opsi]

  --cli-only        pasang CLI saja
  --desktop-only    pasang aplikasi desktop saja
  --version X.Y.Z   versi tertentu (bawaan: rilis terbaru)
  --bin-dir PATH    folder CLI (bawaan: ~/.local/bin)
  --appimage        Linux: pakai AppImage walau ada dpkg/rpm
EOF
}

while [ $# -gt 0 ]; do
  case "$1" in
    --cli-only) WANT_DESKTOP=0 ;;
    --desktop-only) WANT_CLI=0 ;;
    --version) [ $# -ge 2 ] || die "--version butuh nilai"; VERSION="$2"; shift ;;
    --bin-dir) [ $# -ge 2 ] || die "--bin-dir butuh nilai"; BIN_DIR="$2"; shift ;;
    --appimage) FORCE_APPIMAGE=1 ;;
    -h|--help) usage; exit 0 ;;
    *) die "opsi tidak dikenal: $1" ;;
  esac
  shift
done
VERSION="${VERSION#v}"
[ "$WANT_CLI" = 1 ] || [ "$WANT_DESKTOP" = 1 ] || die "--cli-only dan --desktop-only tidak bisa dipakai bersamaan"

# ---- Alat unduh ------------------------------------------------------------
if has curl; then
  fetch() {
    if [ -n "${GITHUB_TOKEN:-}" ]; then curl -fsSL -H "Authorization: Bearer $GITHUB_TOKEN" "$1"
    else curl -fsSL "$1"; fi
  }
  download() { curl -fL --progress-bar -o "$2" "$1"; }
elif has wget; then
  fetch() {
    if [ -n "${GITHUB_TOKEN:-}" ]; then wget -qO- --header="Authorization: Bearer $GITHUB_TOKEN" "$1"
    else wget -qO- "$1"; fi
  }
  download() { wget -q -O "$2" "$1"; }
else
  die "butuh curl atau wget"
fi

# ---- Platform --------------------------------------------------------------
OS="$(uname -s)"
ARCH="$(uname -m)"
case "$ARCH" in
  arm64|aarch64) ARCH=aarch64 ;;
  x86_64|amd64) ARCH=x86_64 ;;
  *) die "arsitektur $ARCH belum didukung" ;;
esac
case "$OS" in
  Darwin) TARGET="$ARCH-apple-darwin" ;;
  Linux)  TARGET="$ARCH-unknown-linux-gnu" ;;
  MINGW*|MSYS*|CYGWIN*) die "Windows: unduh installer .exe/.msi dari https://github.com/$REPO/releases" ;;
  *) die "sistem operasi $OS belum didukung" ;;
esac

TMP="$(mktemp -d)"
MOUNT=""
cleanup() {
  if [ -n "$MOUNT" ]; then hdiutil detach "$MOUNT" -quiet >/dev/null 2>&1 || true; fi
  rm -rf "$TMP"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

# ---- Info rilis ------------------------------------------------------------
if [ -n "$VERSION" ]; then
  API="https://api.github.com/repos/$REPO/releases/tags/v$VERSION"
else
  API="https://api.github.com/repos/$REPO/releases/latest"
fi
info "Mengambil info rilis dari GitHub"
RELEASE_JSON="$(fetch "$API")" || die "tidak bisa membaca $API (rilis tidak ada atau kena rate limit; coba set GITHUB_TOKEN)"
TAG="$(printf '%s\n' "$RELEASE_JSON" | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -1)"
[ -n "$TAG" ] || die "rilis tidak ditemukan"
VERSION="${TAG#v}"
ASSETS="$(printf '%s\n' "$RELEASE_JSON" | sed -n 's/.*"browser_download_url": *"\([^"]*\)".*/\1/p')"
info "Versi $VERSION ($TARGET)"

# asset_url <regex akhir nama file> -> URL pertama yang cocok, atau kosong
asset_url() { printf '%s\n' "$ASSETS" | grep -E "/[^/]*$1\$" | head -1 || true; }

# verify <file> <url>: cek sha256 bila <url>.sha256 ada di rilis
verify() {
  sum_url="$(printf '%s\n' "$ASSETS" | grep -Fx "$2.sha256" || true)"
  [ -n "$sum_url" ] || return 0
  want="$(fetch "$sum_url" | awk '{print $1}')"
  if has shasum; then have="$(shasum -a 256 "$1" | awk '{print $1}')"
  elif has sha256sum; then have="$(sha256sum "$1" | awk '{print $1}')"
  else warn "shasum/sha256sum tidak ada, checksum dilewati"; return 0; fi
  [ "$want" = "$have" ] || die "checksum $(basename "$1") tidak cocok"
}

# ---- CLI -------------------------------------------------------------------
install_cli() {
  url="$(asset_url "$CLI_NAME-$VERSION-$TARGET\\.tar\\.gz")"
  if [ -z "$url" ]; then
    warn "CLI untuk $TARGET belum ada di rilis $TAG, dilewati"
    return 0
  fi
  info "Mengunduh CLI: $(basename "$url")"
  download "$url" "$TMP/cli.tar.gz"
  verify "$TMP/cli.tar.gz" "$url"
  mkdir -p "$TMP/cli" "$BIN_DIR"
  tar -xzf "$TMP/cli.tar.gz" -C "$TMP/cli"
  bin="$(find "$TMP/cli" -type f -name "$CLI_NAME" | head -1)"
  [ -n "$bin" ] || die "binary $CLI_NAME tidak ada di arsip"
  cp "$bin" "$BIN_DIR/$CLI_NAME.tmp"
  chmod 755 "$BIN_DIR/$CLI_NAME.tmp"
  mv -f "$BIN_DIR/$CLI_NAME.tmp" "$BIN_DIR/$CLI_NAME"
  info "CLI terpasang: $BIN_DIR/$CLI_NAME"
  INSTALLED="$INSTALLED cli"
  case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) warn "$BIN_DIR belum ada di PATH. Tambahkan ke ~/.zshrc atau ~/.bashrc:
    export PATH=\"$BIN_DIR:\$PATH\"" ;;
  esac
}

# ---- Desktop: macOS --------------------------------------------------------
install_desktop_macos() {
  case "$ARCH" in aarch64) dmg_arch=aarch64 ;; *) dmg_arch=x64 ;; esac
  url="$(asset_url "_${VERSION}_${dmg_arch}\\.dmg")"
  [ -n "$url" ] || url="$(asset_url "_${VERSION}_universal\\.dmg")"
  if [ -z "$url" ]; then
    warn "DMG untuk $dmg_arch belum ada di rilis $TAG, aplikasi desktop dilewati"
    return 0
  fi
  info "Mengunduh aplikasi: $(basename "$url")"
  download "$url" "$TMP/app.dmg"
  verify "$TMP/app.dmg" "$url"

  MOUNT="$TMP/mnt"
  mkdir -p "$MOUNT"
  hdiutil attach "$TMP/app.dmg" -nobrowse -readonly -quiet -mountpoint "$MOUNT"
  app="$(find "$MOUNT" -maxdepth 1 -name '*.app' | head -1)"
  [ -n "$app" ] || die "file .app tidak ada di DMG"

  dest="/Applications"
  [ -w "$dest" ] || { dest="$HOME/Applications"; mkdir -p "$dest"; }
  target="$dest/$(basename "$app")"
  if pgrep -xq "$APP_NAME"; then
    warn "$APP_NAME sedang berjalan; tutup lalu buka lagi agar versi baru dipakai"
  fi
  rm -rf "$target"
  ditto "$app" "$target"
  hdiutil detach "$MOUNT" -quiet && MOUNT=""
  info "Aplikasi terpasang: $target"
  INSTALLED="$INSTALLED desktop"
}

# ---- Desktop: Linux --------------------------------------------------------
as_root() {
  if [ "$(id -u)" = 0 ]; then "$@"
  elif has sudo; then sudo "$@"
  else return 1; fi
}

install_appimage() {
  url="$(asset_url "_${VERSION}_amd64\\.AppImage")"
  [ -n "$url" ] || { warn "AppImage belum ada di rilis $TAG, aplikasi desktop dilewati"; return 0; }
  info "Mengunduh aplikasi: $(basename "$url")"
  mkdir -p "$BIN_DIR"
  download "$url" "$TMP/app.AppImage"
  verify "$TMP/app.AppImage" "$url"
  cp "$TMP/app.AppImage" "$BIN_DIR/$APP_NAME"
  chmod 755 "$BIN_DIR/$APP_NAME"

  apps="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
  mkdir -p "$apps"
  cat > "$apps/$APP_NAME.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=$APP_NAME
Comment=Arsip memori hidup pribadi
Exec=$BIN_DIR/$APP_NAME
Terminal=false
Categories=Office;Utility;
EOF
  info "Aplikasi terpasang (AppImage): $BIN_DIR/$APP_NAME"
  INSTALLED="$INSTALLED desktop"
  has fusermount || has fusermount3 || warn "AppImage butuh FUSE (mis. sudo apt install libfuse2)"
}

install_desktop_linux() {
  if [ "$ARCH" != x86_64 ]; then
    warn "aplikasi desktop Linux baru tersedia untuk x86_64, dilewati"
    return 0
  fi
  if [ "$FORCE_APPIMAGE" = 0 ]; then
    deb="$(asset_url "_${VERSION}_amd64\\.deb")"
    rpm="$(asset_url "-${VERSION}-[0-9]+\\.x86_64\\.rpm")"
    if [ -n "$deb" ] && has apt-get; then
      info "Mengunduh aplikasi: $(basename "$deb")"
      download "$deb" "$TMP/$APP_NAME.deb"
      verify "$TMP/$APP_NAME.deb" "$deb"
      if as_root apt-get install -y "$TMP/$APP_NAME.deb"; then
        info "Aplikasi terpasang lewat apt"; INSTALLED="$INSTALLED desktop"; return 0
      fi
      warn "apt gagal (atau tanpa sudo), beralih ke AppImage"
    elif [ -n "$rpm" ] && { has dnf || has zypper || has yum; }; then
      info "Mengunduh aplikasi: $(basename "$rpm")"
      download "$rpm" "$TMP/$APP_NAME.rpm"
      verify "$TMP/$APP_NAME.rpm" "$rpm"
      if has dnf; then set -- dnf install -y
      elif has zypper; then set -- zypper --non-interactive install --allow-unsigned-rpm
      else set -- yum install -y; fi
      if as_root "$@" "$TMP/$APP_NAME.rpm"; then
        info "Aplikasi terpasang lewat $1"; INSTALLED="$INSTALLED desktop"; return 0
      fi
      warn "pemasangan rpm gagal (atau tanpa sudo), beralih ke AppImage"
    fi
  fi
  install_appimage
}

# ---- Jalankan --------------------------------------------------------------
if [ "$WANT_CLI" = 1 ]; then install_cli; fi
if [ "$WANT_DESKTOP" = 1 ]; then
  case "$OS" in
    Darwin) install_desktop_macos ;;
    Linux)  install_desktop_linux ;;
  esac
fi

[ -n "$INSTALLED" ] || die "tidak ada yang terpasang: paket untuk $TARGET belum tersedia di rilis $TAG (lihat https://github.com/$REPO/releases/tag/$TAG)"
info "Selesai."
case "$INSTALLED" in *cli*) info "Coba: $CLI_NAME --help" ;; esac
