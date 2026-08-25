import { detectOperatingSystem, detectPlatform } from './platform.js';
import { fetchReleaseManifest } from './release-client.js';

const PLATFORM_LABELS = {
  'darwin-aarch64': ['macOS', 'Apple Silicon', 'DMG'],
  'darwin-x86_64': ['macOS', 'Intel', 'DMG'],
  'windows-x86_64': ['Windows', '64 bits', 'EXE'],
  'linux-x86_64': ['Linux', 'x86_64', 'AppImage'],
  'linux-aarch64': ['Linux', 'ARM64', 'AppImage'],
};
let manifest = null;
let detectedPlatform = null;
const detectedOperatingSystem = detectOperatingSystem();

function bytes(value) {
  if (value > 1024 ** 3) return `${(value / 1024 ** 3).toFixed(1)} Go`;
  return `${(value / 1024 ** 2).toFixed(1)} Mo`;
}

function toast(message) {
  const node = document.querySelector('#toast');
  node.textContent = message;
  node.classList.add('show');
  window.setTimeout(() => node.classList.remove('show'), 2600);
}

async function loadManifest() {
  manifest = await fetchReleaseManifest();
  applyRecommendedDownload();
  const state = document.querySelector('#release-state');
  if (manifest.preview) {
    document.querySelector('#manifest-version').textContent = 'Aperçu local · les téléchargements seront activés après la prochaine release complète';
    state.lastChild.textContent = 'Prévisualisation locale du portail';
  } else {
    const publishedAt = new Date(manifest.publishedAt);
    document.querySelector('#manifest-version').textContent = `Version stable ${manifest.version} · publiée le ${publishedAt.toLocaleDateString('fr-FR')}`;
    state.classList.add('ready');
    state.lastChild.textContent = `Release ${manifest.version} vérifiée`;
  }
  renderPlatforms();
}

function applyRecommendedDownload() {
  const primary = document.querySelector('#download-primary');
  const recommended = detectedPlatform && manifest.platforms[detectedPlatform];
  if (recommended?.setup?.url) {
    primary.href = recommended.setup.url;
    primary.removeAttribute('aria-disabled');
    primary.dataset.sha256 = recommended.setup.sha256;
    document.querySelector('#download-detail').textContent = `${PLATFORM_LABELS[detectedPlatform].join(' · ')} · ${bytes(recommended.setup.size)}`;
  } else {
    primary.href = '#all-downloads';
    primary.setAttribute('aria-disabled', 'true');
    document.querySelector('#download-detail').textContent = 'Choisir précisément votre appareil';
  }
}

function showDetectedSystem() {
  const detail = document.querySelector('#download-detail');
  if (detectedPlatform) {
    detail.textContent = `${PLATFORM_LABELS[detectedPlatform][0]} · ${PLATFORM_LABELS[detectedPlatform][1]} détecté`;
  } else if (detectedOperatingSystem) {
    detail.textContent = `${detectedOperatingSystem} détecté · choisissez votre architecture`;
  } else {
    detail.textContent = 'Système non identifié · choisissez votre appareil';
  }
}

function renderPlatforms() {
  const grid = document.querySelector('#platform-grid');
  grid.replaceChildren();
  for (const [key, download] of Object.entries(manifest.platforms)) {
    const labels = PLATFORM_LABELS[key] || [key, '', 'Setup'];
    const card = document.createElement('article');
    card.className = `platform-card${key === detectedPlatform ? ' recommended' : ''}`;

    if (key === detectedPlatform) {
      const tag = document.createElement('span');
      tag.className = 'tag';
      tag.textContent = 'RECOMMANDÉ';
      card.append(tag);
    }
    const title = document.createElement('h3');
    title.textContent = labels[0];
    const description = document.createElement('p');
    description.textContent = `${labels[1]} · Setup guidé ${labels[2]}`;
    const row = document.createElement('div');
    row.className = 'download-row';
    const link = document.createElement('a');
    if (download?.setup?.url) {
      link.href = download.setup.url;
      link.textContent = 'Télécharger Setup';
    } else {
      link.removeAttribute('href');
      link.setAttribute('aria-disabled', 'true');
      link.textContent = 'Disponible après publication';
    }
    const copy = document.createElement('button');
    copy.type = 'button';
    copy.title = 'Copier le SHA-256';
    copy.setAttribute('aria-label', `Copier le SHA-256 du Setup ${labels[0]} ${labels[1]}`);
    copy.textContent = '#';
    copy.disabled = !download?.setup?.sha256;
    if (download?.setup?.sha256) {
      copy.addEventListener('click', () => copyText(download.setup.sha256, 'SHA‑256 copié'));
    }
    row.append(link, copy);
    const details = document.createElement('small');
    details.textContent = download?.setup
      ? `${bytes(download.setup.size)} · ${download.setup.sha256.slice(0, 16)}…`
      : 'Aucun installateur factice n’est proposé en mode local';
    card.append(title, description, row, details);
    grid.append(card);
  }
}

async function copyText(value, successMessage) {
  try {
    await navigator.clipboard.writeText(value);
    toast(successMessage);
  } catch {
    toast('Copie impossible : sélectionnez le texte manuellement.');
  }
}

async function verifyFile(file) {
  if (!file || !manifest) return;
  const drop = document.querySelector('.verify-drop');
  const result = document.querySelector('#verify-result');
  drop.classList.remove('success', 'error');
  result.textContent = 'Calcul SHA‑256 en cours…';
  if (manifest.preview) {
    drop.classList.add('error');
    result.textContent = 'La vérification sera active avec une release publique complète.';
    return;
  }
  try {
    const buffer = await file.arrayBuffer();
    const digest = await crypto.subtle.digest('SHA-256', buffer);
    const hash = [...new Uint8Array(digest)].map((value) => value.toString(16).padStart(2, '0')).join('');
    const artifacts = Object.values(manifest.platforms)
      .flatMap((item) => [item.application, item.setup, item.cli].filter(Boolean));
    const match = artifacts.find((item) => item.sha256.toLowerCase() === hash);
    drop.classList.add(match ? 'success' : 'error');
    result.textContent = match
      ? `✓ Authentique — ${match.name}`
      : `✕ Ce SHA‑256 ne correspond pas à la release ${manifest.version}`;
  } catch {
    drop.classList.add('error');
    result.textContent = '✕ Ce fichier n’a pas pu être vérifié.';
  }
}

document.querySelector('#copy-command').addEventListener('click', () => {
  const portal = globalThis.location?.origin || 'https://fileflow-downloads.pages.dev';
  const command = detectedPlatform === 'windows-x86_64'
    ? `irm ${portal}/install.ps1 | iex`
    : `curl -fsSL ${portal}/install.sh | sh`;
  copyText(command, 'Commande terminal copiée');
});

const fileInput = document.querySelector('#verify-file');
const dropZone = document.querySelector('.verify-drop');
fileInput.addEventListener('change', (event) => verifyFile(event.target.files?.[0]));
for (const eventName of ['dragenter', 'dragover']) {
  dropZone.addEventListener(eventName, (event) => {
    event.preventDefault();
    dropZone.classList.add('dragging');
  });
}
for (const eventName of ['dragleave', 'drop']) {
  dropZone.addEventListener(eventName, (event) => {
    event.preventDefault();
    dropZone.classList.remove('dragging');
  });
}
dropZone.addEventListener('drop', (event) => verifyFile(event.dataTransfer?.files?.[0]));

showDetectedSystem();
detectPlatform()
  .then((platform) => {
    detectedPlatform = platform;
    if (manifest) {
      applyRecommendedDownload();
      renderPlatforms();
    } else {
      showDetectedSystem();
    }
  })
  .catch(() => showDetectedSystem());

loadManifest().catch((error) => {
  document.querySelector('#release-state').lastChild.textContent = 'Release temporairement indisponible';
  document.querySelector('#manifest-version').textContent = error.message;
  manifest = {
    preview: true,
    version: null,
    platforms: Object.fromEntries(Object.keys(PLATFORM_LABELS).map((key) => [key, {}])),
  };
  renderPlatforms();
});
