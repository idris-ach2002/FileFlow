#!/bin/sh
set -eu

if [ "$(uname -s 2>/dev/null || true)" != "Darwin" ]; then
  echo "This helper is for macOS. Use scripts/setup.sh for automatic platform detection."
  exit 1
fi

printf '%s\n' "== FileFlow macOS setup =="

if ! xcode-select -p >/dev/null 2>&1; then
  echo "Xcode Command Line Tools are required by Tauri."
  echo "Starting Apple's installer..."
  xcode-select --install || true
  echo
  echo "Finish the Command Line Tools installation, then run this script again."
  exit 1
fi

echo "[OK] Xcode Command Line Tools"

if ! command -v brew >/dev/null 2>&1; then
  echo
  echo "Homebrew is not installed. It is used only to install FileFlow's local conversion engines."
  echo "Install Homebrew from https://brew.sh, then run this script again."
  echo "Tauri itself does not require Homebrew on macOS."
  exit 1
fi

echo "[OK] Homebrew: $(command -v brew)"

install_formula() {
  formula=$1
  if brew list --formula "$formula" >/dev/null 2>&1; then
    printf '[OK] %-18s already installed\n' "$formula"
    return 0
  fi

  printf '[INSTALL] %s\n' "$formula"
  if ! brew install "$formula"; then
    printf '[WARN] Optional Homebrew formula failed: %s\n' "$formula"
  fi
}

# Conversion engines. All are optional at runtime: missing engines only disable
# the related FileFlow capabilities.
for formula in \
  ffmpeg \
  vips \
  imagemagick \
  qpdf \
  poppler \
  ghostscript \
  tesseract \
  tesseract-lang \
  ocrmypdf \
  pandoc \
  exiftool \
  sevenzip
do
  install_formula "$formula"
done

if brew list --cask libreoffice >/dev/null 2>&1; then
  echo "[OK] libreoffice        already installed"
else
  echo "[INSTALL] libreoffice"
  if ! brew install --cask libreoffice; then
    echo "[WARN] LibreOffice installation failed; Office conversion will stay disabled."
  fi
fi

if ! command -v rustup >/dev/null 2>&1; then
  echo
  echo "Rust is not installed yet. Install it with rustup:"
  echo "  curl --proto '=https' --tlsv1.2 https://sh.rustup.rs -sSf | sh"
fi

echo
echo "macOS prerequisites and optional engines are ready."
echo "Next: sh scripts/bootstrap.sh"
