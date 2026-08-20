#!/bin/sh
set -eu

if [ "$(uname -s 2>/dev/null || true)" != "Linux" ]; then
  echo "This helper is for Linux. Use scripts/setup.sh for automatic platform detection."
  exit 1
fi

if ! command -v apt-get >/dev/null 2>&1; then
  echo "Automatic Linux setup currently supports Debian/Ubuntu (apt-get)."
  echo "For another distribution, install the equivalent Tauri prerequisites and conversion engines."
  exit 1
fi

sudo apt-get update

# Tauri desktop build prerequisites for Debian/Ubuntu.
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev \
  build-essential \
  curl \
  wget \
  file \
  libxdo-dev \
  libssl-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev

install_optional() {
  package=$1
  if ! sudo apt-get install -y "$package"; then
    printf '[WARN] Optional package unavailable: %s\n' "$package"
  fi
}

for package in \
  ffmpeg \
  libvips-tools \
  imagemagick \
  qpdf \
  img2pdf \
  poppler-utils \
  ghostscript \
  tesseract-ocr \
  tesseract-ocr-fra \
  ocrmypdf \
  libreoffice \
  pandoc \
  libimage-exiftool-perl \
  zstd lz4
do
  install_optional "$package"
done

if ! command -v 7zz >/dev/null 2>&1 && ! command -v 7z >/dev/null 2>&1; then
  sudo apt-get install -y 7zip || sudo apt-get install -y p7zip-full || true
fi

if ! command -v rustup >/dev/null 2>&1; then
  echo
  echo "Rust is not installed yet. Install it with rustup:"
  echo "  curl --proto '=https' --tlsv1.2 https://sh.rustup.rs -sSf | sh"
fi

echo
echo "Linux prerequisites and optional engines are ready."
echo "Next: sh scripts/bootstrap.sh"
