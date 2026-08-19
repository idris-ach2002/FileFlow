#!/bin/sh
set -u

find_command() {
  name=$1

  if command -v "$name" >/dev/null 2>&1; then
    command -v "$name"
    return 0
  fi

  for dir in /opt/homebrew/bin /usr/local/bin /usr/bin /bin /home/linuxbrew/.linuxbrew/bin; do
    if [ -x "$dir/$name" ]; then
      printf '%s\n' "$dir/$name"
      return 0
    fi
  done

  return 1
}

check() {
  label=$1
  shift

  for binary in "$@"; do
    if path=$(find_command "$binary" 2>/dev/null); then
      printf "[OK]   %-14s %s\n" "$label" "$path"
      return 0
    fi
  done

  printf "[MISS] %-14s %s\n" "$label" "not installed"
  return 0
}

check_office() {
  for binary in libreoffice soffice; do
    if path=$(find_command "$binary" 2>/dev/null); then
      printf "[OK]   %-14s %s\n" "LibreOffice" "$path"
      return 0
    fi
  done

  for path in \
    /Applications/LibreOffice.app/Contents/MacOS/soffice \
    "$HOME/Applications/LibreOffice.app/Contents/MacOS/soffice"
  do
    if [ -x "$path" ]; then
      printf "[OK]   %-14s %s\n" "LibreOffice" "$path"
      return 0
    fi
  done

  printf "[MISS] %-14s %s\n" "LibreOffice" "not installed"
}

printf 'Host: %s / %s\n\n' "$(uname -s 2>/dev/null || echo unknown)" "$(uname -m 2>/dev/null || echo unknown)"
check "FFmpeg" ffmpeg
check "libvips" vips
check "ImageMagick" magick convert
check "qpdf" qpdf
check "img2pdf" img2pdf
check "Poppler" pdftoppm
check "Ghostscript" gs
check "OCRmyPDF" ocrmypdf
check "Tesseract" tesseract
check_office
check "Pandoc" pandoc
check "ExifTool" exiftool
check "7-Zip" 7zz 7z
