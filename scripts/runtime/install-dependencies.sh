#!/usr/bin/env bash
set -u

# Installs FileFlow runtime engines on the host machine. This script is
# deliberately best-effort: one unavailable package/repository must never abort
# the whole installation. Missing capabilities are reported at the end.

QUIET=0
NO_UPDATE=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --quiet) QUIET=1; shift ;;
    --no-update) NO_UPDATE=1; shift ;;
    -h|--help)
      cat <<'HELP'
Usage: scripts/runtime/install-dependencies.sh [--quiet] [--no-update]

Installs FileFlow conversion engines using the native package manager first,
then safe fallbacks (Homebrew/pipx/Flatpak when already available). Failures are
collected and reported instead of stopping the script.
HELP
      exit 0
      ;;
    *) printf '[WARN] Unknown option ignored: %s\n' "$1"; shift ;;
  esac
done

OS="$(uname -s 2>/dev/null || printf unknown)"
ARCH="$(uname -m 2>/dev/null || printf unknown)"
USER_BIN="${HOME}/.local/bin"
mkdir -p "$USER_BIN" 2>/dev/null || true

ok=0
missing=0
warnings=0
BREW_SETUP_ATTEMPTED=0
FLATPAK_SETUP_ATTEMPTED=0

say() { [ "$QUIET" -eq 1 ] || printf '%s\n' "$*"; }
warn() { warnings=$((warnings + 1)); printf '[WARN] %s\n' "$*" >&2; }
has() { command -v "$1" >/dev/null 2>&1; }
has_any() {
  local item
  for item in "$@"; do
    has "$item" && return 0
  done
  return 1
}

run_root() {
  if [ "$(id -u)" -eq 0 ]; then
    "$@"
  elif has sudo; then
    sudo "$@"
  else
    return 127
  fi
}

refresh_brew_path() {
  if has brew; then return 0; fi
  if [ -x /opt/homebrew/bin/brew ]; then eval "$(/opt/homebrew/bin/brew shellenv)"; fi
  if [ -x /usr/local/bin/brew ]; then eval "$(/usr/local/bin/brew shellenv)"; fi
  if [ -x /home/linuxbrew/.linuxbrew/bin/brew ]; then eval "$(/home/linuxbrew/.linuxbrew/bin/brew shellenv)"; fi
}

ensure_homebrew() {
  refresh_brew_path
  has brew && return 0
  [ "$BREW_SETUP_ATTEMPTED" -eq 0 ] || return 1
  BREW_SETUP_ATTEMPTED=1
  case "$OS" in Darwin|Linux) ;; *) return 1 ;; esac
  has curl || return 1
  say '[SETUP] Homebrew not found; trying the official Homebrew installer as fallback.'
  NONINTERACTIVE=1 /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)" || return 1
  refresh_brew_path
  has brew
}

prepare_native_manager() {
  [ "$NO_UPDATE" -eq 1 ] && return 0
  case "${PKG_MANAGER:-none}" in
    apt) run_root apt-get update || warn 'apt update failed; package installs will still be attempted.' ;;
    dnf) run_root dnf -y makecache || warn 'dnf metadata refresh failed; continuing.' ;;
    zypper) run_root zypper --non-interactive refresh || warn 'zypper refresh failed; continuing.' ;;
    pacman) run_root pacman -Sy --noconfirm || warn 'pacman sync failed; continuing.' ;;
    brew) brew update >/dev/null 2>&1 || warn 'brew update failed; existing metadata will be used.' ;;
  esac
}

native_install() {
  local package="$1"
  case "${PKG_MANAGER:-none}" in
    apt) DEBIAN_FRONTEND=noninteractive run_root apt-get install -y "$package" ;;
    dnf) run_root dnf install -y "$package" ;;
    zypper) run_root zypper --non-interactive install -y "$package" ;;
    pacman) run_root pacman -S --needed --noconfirm "$package" ;;
    brew) brew install "$package" ;;
    *) return 127 ;;
  esac
}

brew_install() {
  local package="$1"
  ensure_homebrew || return 127
  refresh_brew_path
  brew list --formula "$package" >/dev/null 2>&1 && return 0
  brew install "$package"
}

brew_cask_install() {
  local package="$1"
  ensure_homebrew || return 127
  refresh_brew_path
  brew list --cask "$package" >/dev/null 2>&1 && return 0
  brew install --cask "$package"
}

ensure_pipx() {
  has pipx && return 0
  native_install pipx >/dev/null 2>&1 || true
  has pipx && return 0
  if ! has python3; then native_install python3 >/dev/null 2>&1 || true; fi
  if has python3; then
    python3 -m pip install --user pipx >/dev/null 2>&1 || true
    export PATH="$USER_BIN:$PATH"
  fi
  has pipx && return 0
  brew_install pipx >/dev/null 2>&1 || true
  refresh_brew_path
  has pipx
}

pipx_install() {
  local package="$1"
  ensure_pipx || return 127
  pipx install "$package" >/dev/null 2>&1 || pipx upgrade "$package" >/dev/null 2>&1
}

ensure_flatpak() {
  has flatpak && return 0
  [ "$FLATPAK_SETUP_ATTEMPTED" -eq 0 ] || return 1
  FLATPAK_SETUP_ATTEMPTED=1
  [ "$OS" = Linux ] || return 1
  native_install flatpak >/dev/null 2>&1 || return 1
  has flatpak || return 1
  flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo >/dev/null 2>&1 || true
  return 0
}

flatpak_libreoffice() {
  ensure_flatpak || return 127
  flatpak install -y flathub org.libreoffice.LibreOffice || return 1
  cat > "$USER_BIN/libreoffice" <<'WRAP'
#!/usr/bin/env bash
exec flatpak run org.libreoffice.LibreOffice "$@"
WRAP
  chmod 0755 "$USER_BIN/libreoffice"
}

probe_office() {
  has_any libreoffice soffice && return 0
  [ -x /Applications/LibreOffice.app/Contents/MacOS/soffice ] && return 0
  [ -x "$HOME/Applications/LibreOffice.app/Contents/MacOS/soffice" ] && return 0
  return 1
}

probe_spec() {
  local spec="$1" cmd
  IFS='|' read -r -a cmds <<< "$spec"
  for cmd in "${cmds[@]}"; do
    [ "$cmd" = '@office' ] && { probe_office && return 0; continue; }
    has "$cmd" && return 0
  done
  return 1
}

attempt() {
  local kind="$1" package="$2"
  case "$kind" in
    native) native_install "$package" ;;
    brew) brew_install "$package" ;;
    brew-cask) brew_cask_install "$package" ;;
    pipx) pipx_install "$package" ;;
    flatpak-lo) flatpak_libreoffice ;;
    *) return 127 ;;
  esac
}

ensure_engine() {
  local label="$1" probe="$2"; shift 2
  if probe_spec "$probe"; then
    printf '[OK]   %-14s already available\n' "$label"
    ok=$((ok + 1))
    return 0
  fi

  local candidate kind package
  for candidate in "$@"; do
    kind="${candidate%%:*}"
    package="${candidate#*:}"
    say "[TRY]  $label via $kind:$package"
    if attempt "$kind" "$package" >/dev/null 2>&1; then
      export PATH="$USER_BIN:$PATH"
      refresh_brew_path
      if probe_spec "$probe"; then
        printf '[OK]   %-14s installed via %s\n' "$label" "$kind"
        ok=$((ok + 1))
        return 0
      fi
    fi
    warn "$label: $kind:$package unavailable or installation failed; trying next source."
  done

  printf '[MISS] %-14s not installed; related features will be disabled.\n' "$label" >&2
  missing=$((missing + 1))
  return 0
}

install_linux_app_runtime() {
  [ "$OS" = Linux ] || return 0
  say "[SETUP] Installing Tauri/WebKit runtime libraries when available."
  local package
  case "${PKG_MANAGER:-none}" in
    apt)
      for package in libwebkit2gtk-4.1-0 libgtk-3-0 libayatana-appindicator3-1 librsvg2-2 xdg-utils; do
        native_install "$package" >/dev/null 2>&1 || warn "runtime package $package unavailable; continuing."
      done
      ;;
    dnf)
      for package in webkit2gtk4.1 gtk3 libappindicator-gtk3 librsvg2 xdg-utils; do
        native_install "$package" >/dev/null 2>&1 || warn "runtime package $package unavailable; continuing."
      done
      ;;
    pacman)
      for package in webkit2gtk-4.1 gtk3 libappindicator-gtk3 librsvg xdg-utils; do
        native_install "$package" >/dev/null 2>&1 || warn "runtime package $package unavailable; continuing."
      done
      ;;
    zypper)
      for package in libwebkit2gtk-4_1-0 gtk3 libayatana-appindicator3-1 librsvg-2-2 xdg-utils; do
        native_install "$package" >/dev/null 2>&1 || warn "runtime package $package unavailable; continuing."
      done
      ;;
    brew|none) : ;;
  esac
}

case "$OS" in
  Darwin)
    ensure_homebrew || warn 'Homebrew could not be installed; FileFlow will continue and report missing engines.'
    refresh_brew_path
    PKG_MANAGER=brew
    ;;
  Linux)
    if has apt-get; then PKG_MANAGER=apt
    elif has dnf; then PKG_MANAGER=dnf
    elif has zypper; then PKG_MANAGER=zypper
    elif has pacman; then PKG_MANAGER=pacman
    elif has brew; then PKG_MANAGER=brew
    else PKG_MANAGER=none; warn 'No supported package manager found (apt/dnf/zypper/pacman/brew).'; fi
    ;;
  *)
    printf '[WARN] Runtime dependency installer does not support %s.\n' "$OS" >&2
    exit 0
    ;;
esac

say "FileFlow runtime dependency setup — $OS / $ARCH (${PKG_MANAGER})"
prepare_native_manager
install_linux_app_runtime

# Each engine is installed independently. Package-name alternatives are kept in
# order so a missing package cannot abort the remaining engine installation.
case "$PKG_MANAGER" in
  apt)
    ensure_engine FFmpeg ffmpeg native:ffmpeg brew:ffmpeg
    ensure_engine libvips vips native:libvips-tools brew:vips
    ensure_engine ImageMagick 'magick|convert' native:imagemagick brew:imagemagick
    ensure_engine qpdf qpdf native:qpdf brew:qpdf
    ensure_engine img2pdf img2pdf native:img2pdf native:python3-img2pdf pipx:img2pdf brew:img2pdf
    ensure_engine Poppler 'pdftoppm|pdftotext' native:poppler-utils brew:poppler
    ensure_engine Ghostscript gs native:ghostscript brew:ghostscript
    ensure_engine Tesseract tesseract native:tesseract-ocr brew:tesseract
    ensure_engine OCRmyPDF ocrmypdf native:ocrmypdf pipx:ocrmypdf brew:ocrmypdf
    ensure_engine LibreOffice @office native:libreoffice flatpak-lo:libreoffice
    ensure_engine Pandoc pandoc native:pandoc brew:pandoc
    ensure_engine ExifTool exiftool native:libimage-exiftool-perl brew:exiftool
    ensure_engine 7-Zip '7zz|7z' native:7zip native:p7zip-full brew:sevenzip
    ensure_engine Zstandard zstd native:zstd brew:zstd
    ensure_engine LZ4 lz4 native:lz4 brew:lz4
    # Language data is additive; never blocks the installation.
    native_install tesseract-ocr-eng >/dev/null 2>&1 || true
    native_install tesseract-ocr-fra >/dev/null 2>&1 || true
    ;;
  dnf)
    ensure_engine FFmpeg ffmpeg native:ffmpeg-free native:ffmpeg brew:ffmpeg
    ensure_engine libvips vips native:vips-tools native:vips brew:vips
    ensure_engine ImageMagick 'magick|convert' native:ImageMagick brew:imagemagick
    ensure_engine qpdf qpdf native:qpdf brew:qpdf
    ensure_engine img2pdf img2pdf native:python3-img2pdf pipx:img2pdf brew:img2pdf
    ensure_engine Poppler 'pdftoppm|pdftotext' native:poppler-utils brew:poppler
    ensure_engine Ghostscript gs native:ghostscript brew:ghostscript
    ensure_engine Tesseract tesseract native:tesseract brew:tesseract
    ensure_engine OCRmyPDF ocrmypdf native:ocrmypdf pipx:ocrmypdf brew:ocrmypdf
    ensure_engine LibreOffice @office native:libreoffice flatpak-lo:libreoffice
    ensure_engine Pandoc pandoc native:pandoc brew:pandoc
    ensure_engine ExifTool exiftool native:perl-Image-ExifTool brew:exiftool
    ensure_engine 7-Zip '7zz|7z' native:7zip native:p7zip brew:sevenzip
    ensure_engine Zstandard zstd native:zstd brew:zstd
    ensure_engine LZ4 lz4 native:lz4 brew:lz4
    ;;
  pacman)
    ensure_engine FFmpeg ffmpeg native:ffmpeg brew:ffmpeg
    ensure_engine libvips vips native:libvips brew:vips
    ensure_engine ImageMagick 'magick|convert' native:imagemagick brew:imagemagick
    ensure_engine qpdf qpdf native:qpdf brew:qpdf
    ensure_engine img2pdf img2pdf native:python-img2pdf pipx:img2pdf brew:img2pdf
    ensure_engine Poppler 'pdftoppm|pdftotext' native:poppler brew:poppler
    ensure_engine Ghostscript gs native:ghostscript brew:ghostscript
    ensure_engine Tesseract tesseract native:tesseract brew:tesseract
    ensure_engine OCRmyPDF ocrmypdf native:ocrmypdf pipx:ocrmypdf brew:ocrmypdf
    ensure_engine LibreOffice @office native:libreoffice-fresh native:libreoffice-still flatpak-lo:libreoffice
    ensure_engine Pandoc pandoc native:pandoc brew:pandoc
    ensure_engine ExifTool exiftool native:perl-image-exiftool brew:exiftool
    ensure_engine 7-Zip '7zz|7z' native:7zip native:p7zip brew:sevenzip
    ensure_engine Zstandard zstd native:zstd brew:zstd
    ensure_engine LZ4 lz4 native:lz4 brew:lz4
    ;;
  zypper)
    ensure_engine FFmpeg ffmpeg native:ffmpeg brew:ffmpeg
    ensure_engine libvips vips native:vips-tools native:libvips-tools brew:vips
    ensure_engine ImageMagick 'magick|convert' native:ImageMagick brew:imagemagick
    ensure_engine qpdf qpdf native:qpdf brew:qpdf
    ensure_engine img2pdf img2pdf native:python3-img2pdf pipx:img2pdf brew:img2pdf
    ensure_engine Poppler 'pdftoppm|pdftotext' native:poppler-tools brew:poppler
    ensure_engine Ghostscript gs native:ghostscript brew:ghostscript
    ensure_engine Tesseract tesseract native:tesseract-ocr brew:tesseract
    ensure_engine OCRmyPDF ocrmypdf native:ocrmypdf pipx:ocrmypdf brew:ocrmypdf
    ensure_engine LibreOffice @office native:libreoffice flatpak-lo:libreoffice
    ensure_engine Pandoc pandoc native:pandoc brew:pandoc
    ensure_engine ExifTool exiftool native:perl-Image-ExifTool brew:exiftool
    ensure_engine 7-Zip '7zz|7z' native:7zip native:p7zip-full brew:sevenzip
    ensure_engine Zstandard zstd native:zstd brew:zstd
    ensure_engine LZ4 lz4 native:lz4 brew:lz4
    ;;
  brew)
    ensure_engine FFmpeg ffmpeg brew:ffmpeg
    ensure_engine libvips vips brew:vips
    ensure_engine ImageMagick 'magick|convert' brew:imagemagick
    ensure_engine qpdf qpdf brew:qpdf
    ensure_engine img2pdf img2pdf brew:img2pdf pipx:img2pdf
    ensure_engine Poppler 'pdftoppm|pdftotext' brew:poppler
    ensure_engine Ghostscript gs brew:ghostscript
    ensure_engine Tesseract tesseract brew:tesseract
    ensure_engine OCRmyPDF ocrmypdf brew:ocrmypdf pipx:ocrmypdf
    if [ "$OS" = Darwin ]; then ensure_engine LibreOffice @office brew-cask:libreoffice; else ensure_engine LibreOffice @office flatpak-lo:libreoffice; fi
    ensure_engine Pandoc pandoc brew:pandoc
    ensure_engine ExifTool exiftool brew:exiftool
    ensure_engine 7-Zip '7zz|7z' brew:sevenzip
    ensure_engine Zstandard zstd brew:zstd
    ensure_engine LZ4 lz4 brew:lz4
    ;;
  none)
    for spec in 'FFmpeg:ffmpeg' 'libvips:vips' 'ImageMagick:magick|convert' 'qpdf:qpdf' 'img2pdf:img2pdf' 'Poppler:pdftoppm|pdftotext' 'Ghostscript:gs' 'Tesseract:tesseract' 'OCRmyPDF:ocrmypdf' 'LibreOffice:@office' 'Pandoc:pandoc' 'ExifTool:exiftool' '7-Zip:7zz|7z' 'Zstandard:zstd' 'LZ4:lz4'; do
      label="${spec%%:*}"; probe="${spec#*:}"; ensure_engine "$label" "$probe"
    done
    ;;
esac

printf '\nRuntime dependencies: %s available, %s missing, %s fallback warnings.\n' "$ok" "$missing" "$warnings"
printf 'Missing engines do not prevent FileFlow from being installed; only their related actions are unavailable.\n'
exit 0
