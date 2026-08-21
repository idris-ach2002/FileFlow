#!/usr/bin/env bash
set -u
STRICT=0
[ "${1:-}" = '--strict' ] && STRICT=1

find_cmd() {
  local name="$1" dir
  command -v "$name" 2>/dev/null && return 0
  for dir in /opt/homebrew/bin /usr/local/bin /usr/bin /bin /snap/bin /home/linuxbrew/.linuxbrew/bin "$HOME/.local/bin"; do
    [ -x "$dir/$name" ] && { printf '%s\n' "$dir/$name"; return 0; }
  done
  return 1
}

probe_cmd() {
  local path="$1" name
  name="$(basename "$path")"
  case "$name" in
    ffmpeg) "$path" -version >/dev/null 2>&1 ;;
    vips) "$path" --version >/dev/null 2>&1 ;;
    magick|convert) "$path" -version >/dev/null 2>&1 ;;
    qpdf|img2pdf|tesseract|ocrmypdf|pandoc|zstd|lz4|libreoffice|soffice) "$path" --version >/dev/null 2>&1 ;;
    pdftoppm|pdftotext) "$path" -v >/dev/null 2>&1 ;;
    gs) "$path" --version >/dev/null 2>&1 ;;
    exiftool) "$path" -ver >/dev/null 2>&1 ;;
    7zz|7z) "$path" i >/dev/null 2>&1 ;;
    *) "$path" --version >/dev/null 2>&1 ;;
  esac
}

check() {
  local label="$1"; shift
  local binary path
  for binary in "$@"; do
    if path="$(find_cmd "$binary" 2>/dev/null)"; then
      if probe_cmd "$path"; then
        printf '[OK]     %-14s %s\n' "$label" "$path"
        FOUND=$((FOUND + 1)); return 0
      fi
      printf '[BROKEN] %-14s %s (runtime probe failed)\n' "$label" "$path"
      BROKEN=$((BROKEN + 1)); return 1
    fi
  done
  printf '[MISS]   %-14s not installed\n' "$label"
  MISSING=$((MISSING + 1)); return 1
}

check_office() {
  local path name
  for name in libreoffice soffice; do
    if path="$(find_cmd "$name" 2>/dev/null)"; then
      check LibreOffice "$path" && return 0
      return 1
    fi
  done
  for path in /Applications/LibreOffice.app/Contents/MacOS/soffice "$HOME/Applications/LibreOffice.app/Contents/MacOS/soffice"; do
    if [ -x "$path" ]; then
      if probe_cmd "$path"; then
        printf '[OK]     %-14s %s\n' LibreOffice "$path"; FOUND=$((FOUND + 1)); return 0
      fi
      printf '[BROKEN] %-14s %s (runtime probe failed)\n' LibreOffice "$path"; BROKEN=$((BROKEN + 1)); return 1
    fi
  done
  printf '[MISS]   %-14s not installed\n' LibreOffice
  MISSING=$((MISSING + 1)); return 1
}

FOUND=0; MISSING=0; BROKEN=0
printf 'FileFlow runtime doctor — %s / %s\n\n' "$(uname -s 2>/dev/null || echo unknown)" "$(uname -m 2>/dev/null || echo unknown)"
check FFmpeg ffmpeg || true
check libvips vips || true
check ImageMagick magick convert || true
check qpdf qpdf || true
check img2pdf img2pdf || true
check Poppler pdftoppm pdftotext || true
check Ghostscript gs || true
check Tesseract tesseract || true
check OCRmyPDF ocrmypdf || true
check_office || true
check Pandoc pandoc || true
check ExifTool exiftool || true
check 7-Zip 7zz 7z || true
check Zstandard zstd || true
check LZ4 lz4 || true
printf '\nResult: %s available, %s missing, %s broken.\n' "$FOUND" "$MISSING" "$BROKEN"
[ "$STRICT" -eq 1 ] && [ $((MISSING + BROKEN)) -gt 0 ] && exit 1
exit 0
