import {
  detectDeviceProfile,
  detectOperatingSystem,
  platformAccessState,
} from './platform.js';
import { fetchReleaseManifest } from './release-client.js';

const PLATFORM_LABELS = {
  'darwin-aarch64': ['macOS', 'Apple Silicon', 'ARM64'],
  'darwin-x86_64': ['macOS', 'Intel', 'x86_64'],
  'windows-x86_64': ['Windows', 'x64', '64 bits'],
  'linux-x86_64': ['Linux', 'x64', '64 bits'],
  'linux-aarch64': ['Linux', 'ARM64', '64 bits'],
};
const PLATFORM_OS = {
  'darwin-aarch64': 'macOS',
  'darwin-x86_64': 'macOS',
  'windows-x86_64': 'Windows',
  'linux-x86_64': 'Linux',
  'linux-aarch64': 'Linux',
};
const PLATFORM_MARKS = {
  'darwin-aarch64': '',
  'darwin-x86_64': '',
  'windows-x86_64': '⊞',
  'linux-x86_64': 'L',
  'linux-aarch64': 'L',
};

const DEMO_SLIDES = [
  {
    label: 'ACCUEIL',
    title: 'Commencez avec vos fichiers.',
    copy: 'Déposez un fichier ou un dossier. FileFlow reconnaît le contexte et vous propose les actions compatibles.',
    bullets: ['Glisser-déposer', 'Fichiers ou dossiers', 'PDF, images, vidéo, archives', 'Parcours guidé'],
    images: ['/assets/app/home.webp'],
    alt: 'Accueil FileFlow avec zone de dépôt de fichiers',
  },
  {
    label: 'CONVERSION',
    title: 'Une action. Plusieurs fichiers.',
    copy: 'Convertir, compresser, extraire du texte, organiser, renommer ou protéger : les opérations restent lisibles même sur un lot.',
    bullets: ['Conversion', 'Compression', 'Extraction de texte', 'Organisation'],
    images: ['/assets/app/conversion.webp'],
    alt: 'Choix d’actions après ajout de plusieurs fichiers dans FileFlow',
  },
  {
    label: 'OUTILS AVANCÉS',
    title: '90 actions quand vous en avez besoin.',
    copy: 'L’espace expert rassemble les outils précis sans compliquer l’accueil. Recherchez une action ou parcourez les familles.',
    bullets: ['PDF & OCR', 'Images', 'Documents', 'Archives'],
    images: ['/assets/app/advanced-overview.webp'],
    alt: 'Catalogue avancé des opérations FileFlow',
  },
  {
    label: 'IMAGES',
    title: 'Beaucoup plus que convertir en JPG.',
    copy: 'Conversion en lot, optimisation, redimensionnement, rotation et formats modernes sont réunis dans la même vue.',
    bullets: ['HEIC / WebP / AVIF', 'RAW', 'Optimisation', 'Traitement en lot'],
    images: ['/assets/app/advanced-images.webp'],
    alt: 'Outils avancés FileFlow pour les images',
  },
  {
    label: 'PDF, DOCUMENTS & OCR',
    title: 'Le format ne dicte plus le workflow.',
    copy: 'FileFlow distingue ce qui peut être lu, écrit, transformé ou extrait, puis ouvre les actions réellement disponibles.',
    bullets: ['OCR', 'PDF recherchable', 'Office / HTML / EML → PDF', 'Protection & métadonnées'],
    images: ['/assets/app/formats.webp'],
    alt: 'Matrice des formats et possibilités dans FileFlow',
  },
  {
    label: 'AUDIO & VIDÉO',
    title: 'FFmpeg, sans avoir à apprendre FFmpeg.',
    copy: 'Rendre compatible, changer la résolution, découper, extraire l’audio ou normaliser le volume depuis une interface claire.',
    bullets: ['Réencodage', 'Découpe', 'Résolution', 'Normalisation audio'],
    images: ['/assets/app/advanced-media.webp'],
    alt: 'Outils audio et vidéo de FileFlow',
  },
  {
    label: 'ARCHIVES & COMPRESSION',
    title: 'ZIP, 7Z, TAR… et Zstandard.',
    copy: 'Créer, extraire et recompresser des archives, avec des profils qui peuvent privilégier la vitesse ou la compatibilité.',
    bullets: ['ZIP / 7Z', 'TAR.ZST', 'TAR.LZ4', 'Compression intelligente'],
    images: ['/assets/app/advanced-archives.webp'],
    alt: 'Outils d’archives et de compression dans FileFlow',
  },
  {
    label: 'CONTRÔLE & MAINTENANCE',
    title: 'Une vraie application, pas une page jetable.',
    copy: 'Historique local, mises à jour, moteurs disponibles et réglages restent accessibles après l’installation.',
    bullets: ['Historique local', 'Mise à jour intégrée', '16 moteurs', 'Clair / sombre'],
    images: ['/assets/app/history.webp', '/assets/app/updates.webp', '/assets/app/engines.webp'],
    alt: 'Historique, mise à jour et moteurs locaux de FileFlow',
  },
];

const FEATURE_CATEGORIES = [
  {
    id: 'pdf', title: 'PDF & OCR', count: '17 actions',
    lead: 'Créer, rendre recherchable, protéger et transformer les documents PDF.',
    tags: ['OCRmyPDF', 'Tesseract', 'qpdf', 'Ghostscript'],
    highlights: [
      ['Scan → PDF recherchable', 'OCR puis génération d’un document dans lequel le texte peut être recherché.'],
      ['Créer / fusionner un PDF', 'Images, documents, dossiers ou lots peuvent rejoindre un même PDF.'],
      ['Office, HTML, EML → PDF', 'Passer de documents éditables ou de pages web vers PDF.'],
      ['Protéger & alléger', 'Protection par mot de passe, compression et nettoyage selon le besoin.'],
    ],
  },
  {
    id: 'images', title: 'Images', count: '37 actions',
    lead: 'Formats modernes, lots et optimisation sans multiplier les outils.',
    tags: ['HEIC', 'WebP', 'AVIF', 'RAW', 'ImageMagick', 'libvips'],
    highlights: [
      ['Convertir un lot', 'Uniformiser plusieurs images vers un format de sortie choisi.'],
      ['Optimiser', 'Réduire dimensions et poids avec un profil d’usage.'],
      ['Redimensionner', 'Changer les dimensions sans toucher aux originaux.'],
      ['Métadonnées & GPS', 'Inspecter ou retirer les informations sensibles lorsque nécessaire.'],
    ],
  },
  {
    id: 'documents', title: 'Documents', count: '9 actions',
    lead: 'Transformer les formats de travail en sorties faciles à partager ou archiver.',
    tags: ['LibreOffice', 'Pandoc', 'Chromium'],
    highlights: [
      ['Office → PDF', 'Word, Excel, PowerPoint et formats OpenDocument vers PDF.'],
      ['HTML → PDF', 'Rendu via navigateur pour conserver les pages modernes.'],
      ['EML → PDF', 'Archiver un e-mail avec ses en-têtes sans exécuter ses scripts.'],
      ['Texte & Markdown', 'Convertir des contenus texte vers des formats de diffusion.'],
    ],
  },
  {
    id: 'media', title: 'Audio & vidéo', count: '8 actions',
    lead: 'Les opérations FFmpeg courantes deviennent des actions guidées.',
    tags: ['FFmpeg', 'AAC', 'FLAC', 'MP4', 'MKV'],
    highlights: [
      ['Rendre compatible', 'Réencoder vers des formats faciles à lire sur téléphone, TV ou web.'],
      ['Découper un extrait', 'Choisir une portion audio ou vidéo sans construire une commande.'],
      ['Résolution & rotation', 'Adapter dimensions et orientation avec réencodage contrôlé.'],
      ['Audio', 'Extraire, retirer ou normaliser une piste.'],
    ],
  },
  {
    id: 'archives', title: 'Archives & compression', count: '10 actions',
    lead: 'Créer, extraire et recompresser avec des formats rapides ou très compatibles.',
    tags: ['ZIP', '7Z', 'TAR', 'TAR.ZST', 'TAR.LZ4', 'Zstandard', 'LZ4'],
    highlights: [
      ['TAR.ZST · priorité vitesse', 'Zstandard offre un excellent compromis vitesse / compression pour les gros lots.'],
      ['TAR.LZ4 · décompression rapide', 'Un profil utile lorsque la vitesse de lecture est prioritaire.'],
      ['ZIP / 7Z', 'Formats courants lorsque la compatibilité ou la compression maximale compte davantage.'],
      ['Compression intelligente', 'Le profil peut orienter le choix vers vitesse, compatibilité ou usage du lot.'],
    ],
    featured: 0,
  },
  {
    id: 'organisation', title: 'Organisation', count: '3 actions',
    lead: 'Nettoyer un lot et maîtriser sa destination sans bricoler des commandes.',
    tags: ['Renommer', 'Fusionner', 'Diviser', 'Dossiers'],
    highlights: [
      ['Renommage propre', 'Appliquer une logique cohérente à plusieurs fichiers.'],
      ['Fusion / séparation', 'Assembler ou diviser selon le type de document.'],
      ['Dossiers & sorties', 'Choisir où vont les résultats et conserver les originaux.'],
      ['Traitement en lot', 'Une seule configuration peut s’appliquer à toute une sélection.'],
    ],
  },
  {
    id: 'privacy', title: 'Confidentialité', count: '2 actions dédiées',
    lead: 'Le traitement reste local et les métadonnées peuvent être inspectées ou nettoyées.',
    tags: ['Local', 'ExifTool', 'Protection PDF', 'GPS'],
    highlights: [
      ['Traitement local', 'Les fichiers de travail restent sur votre machine.'],
      ['Métadonnées', 'Inspecter les informations intégrées aux images et documents.'],
      ['GPS', 'Retirer les coordonnées des photos avant partage.'],
      ['Protection', 'Créer des copies protégées sans modifier les originaux.'],
    ],
  },
  {
    id: 'automation', title: 'Automatisations', count: 'À venir',
    lead: 'La prochaine couche de FileFlow : lancer automatiquement des chaînes d’actions.',
    tags: ['Dossiers surveillés', 'Chaînes', 'Exclusions', 'Planification'],
    highlights: [
      ['Surveiller un dossier', 'Déclencher une action lorsqu’un nouveau fichier arrive.'],
      ['Chaîner des actions', 'Convertir, compresser puis organiser dans un même parcours.'],
      ['Exclusions & horaires', 'Limiter ce qui doit être traité et quand.'],
      ['Historique & notifications', 'Suivre ce qui a été exécuté et ce qui demande votre attention.'],
    ],
    coming: true,
  },
];

const ENGINES = ['7-Zip','ExifTool','FFmpeg','Ghostscript','ImageMagick','LZ4','LibreOffice','Chromium','OCRmyPDF','Pandoc','Poppler','Tesseract','Zstandard','img2pdf','libvips','qpdf'];

const GUIDES = {
  macos: {
    title: 'macOS', label: 'Apple Silicon & Intel',
    steps: [
      ['Télécharger FileFlow Setup','Téléchargez la version recommandée pour votre Mac. Le portail choisit Apple Silicon ou Intel lorsqu’il peut le déterminer.','/assets/guides/macos/01-telecharger.png','Le téléchargement reste lié à la release vérifiée.'],
      ['Ouvrir le fichier DMG','Ouvrez le DMG téléchargé pour afficher FileFlowSetup.','/assets/guides/macos/02-ouvrir-dmg.png','Le DMG contient le Setup correspondant à votre architecture.'],
      ['Clic droit → Ouvrir','Pour la toute première ouverture, faites un clic droit sur FileFlowSetup puis choisissez Ouvrir.','/assets/guides/macos/03-clic-droit-ouvrir.png','Cette étape évite le blocage du double-clic sur une application non notarifiée.'],
      ['Confirmer l’ouverture','Confirmez Ouvrir. Si macOS bloque encore : Réglages Système → Confidentialité et sécurité → Ouvrir quand même.','/assets/guides/macos/04-confirmer-ouverture.png','Après cette première autorisation, FileFlow peut être lancé normalement.'],
    ],
  },
  windows: {
    title: 'Windows', label: 'Windows 10 / 11 · x64',
    steps: [
      ['Télécharger FileFlow Setup','Le portail recommande automatiquement l’installateur Windows x64.','/assets/guides/windows/01.svg','Le Setup Windows est produit avec NSIS et MSI dans la chaîne de release.'],
      ['Ouvrir FileFlowSetup.exe','Ouvrez le fichier téléchargé. Si Windows affiche un contrôle, vérifiez le nom FileFlowSetup.exe puis continuez.','/assets/guides/windows/02.svg','Aucun contournement de sécurité n’est demandé.'],
      ['Suivre l’assistant FileFlow','Diagnostic, application, moteurs et post-contrôles restent visibles pendant l’installation.','/assets/guides/windows/03.svg','Précédent, Suivant et Annuler gardent le parcours sous contrôle.'],
      ['FileFlow est prêt','Le Setup termine ses contrôles puis peut ouvrir FileFlow. Il reste aussi disponible pour réparer ou désinstaller.','/assets/guides/windows/04.svg','La maintenance utilise le même Setup que l’installation.'],
    ],
  },
  linux: {
    title: 'Linux', label: 'x64 & ARM64',
    steps: [
      ['Choisir votre Linux','Le portail recommande x64 ou ARM64. L’AppImage est le chemin le plus direct pour lancer le Setup.','/assets/guides/linux/01.svg','DEB et RPM restent également produits par la release.'],
      ['Autoriser l’exécution','Selon votre distribution, activez l’autorisation d’exécuter le fichier. La méthode graphique reste prioritaire.','/assets/guides/linux/02.svg','Alternative terminal : chmod +x FileFlowSetup*.AppImage'],
      ['Lancer FileFlow Setup','Le Setup Linux reprend la même interface : diagnostic, application, moteurs locaux et post-contrôles.','/assets/guides/linux/03.svg','Le terminal n’est pas l’interface principale.'],
      ['Installation terminée','FileFlow et ses moteurs sont contrôlés avant la fin. Le Setup peut ensuite réparer ou désinstaller.','/assets/guides/linux/04.svg','Le même parcours est disponible sur Linux x64 et ARM64.'],
    ],
  },
};

let manifest = null;
let showAllPlatforms = false;
let deviceProfile = {
  operatingSystem: detectOperatingSystem(), architecture: null, platform: null,
  processor: null, confidence: 'partial', detectionSource: null,
};
let demoCarousel;
let featureCarousel;
let guideCarousel;
let currentGuide = 'macos';

function bytes(value) {
  if (!value) return 'taille inconnue';
  if (value > 1024 ** 3) return `${(value / 1024 ** 3).toFixed(1)} Go`;
  return `${(value / 1024 ** 2).toFixed(1)} Mo`;
}
function architectureLabel(value) { return value === 'arm64' ? 'ARM64' : value === 'x64' ? 'x86_64' : 'architecture inconnue'; }
function toast(message) {
  const node = document.querySelector('#toast');
  node.textContent = message;
  node.classList.add('show');
  window.setTimeout(() => node.classList.remove('show'), 2400);
}
function iconForPlatform(platform) { return PLATFORM_MARKS[platform] || 'F'; }
function defaultGuidePlatform() {
  if (deviceProfile.operatingSystem === 'Windows') return 'windows';
  if (deviceProfile.operatingSystem === 'Linux') return 'linux';
  return 'macos';
}

function createCarousel({ viewport, track, prev, next, dots, counter, count, counterText, onIndex }) {
  let index = 0;
  let settleTimer = null;
  const dotButtons = [];
  if (dots) {
    dots.replaceChildren();
    for (let i = 0; i < count; i += 1) {
      const dot = document.createElement('button');
      dot.type = 'button';
      dot.setAttribute('aria-label', `Aller à la vue ${i + 1}`);
      dot.addEventListener('click', () => goTo(i));
      dots.append(dot);
      dotButtons.push(dot);
    }
  }
  function update(nextIndex) {
    index = Math.max(0, Math.min(nextIndex, count - 1));
    if (prev) prev.disabled = index === 0;
    if (next) next.disabled = index === count - 1;
    dotButtons.forEach((dot, i) => dot.classList.toggle('active', i === index));
    if (counter) counter.textContent = counterText(index, count);
    if (onIndex) onIndex(index);
  }
  function goTo(nextIndex, behavior = 'smooth') {
    const width = viewport.clientWidth || 1;
    viewport.scrollTo({ left: Math.max(0, Math.min(nextIndex, count - 1)) * width, behavior });
    update(nextIndex);
  }
  prev?.addEventListener('click', () => goTo(index - 1));
  next?.addEventListener('click', () => goTo(index + 1));
  viewport.addEventListener('keydown', (event) => {
    if (event.key === 'ArrowLeft') { event.preventDefault(); goTo(index - 1); }
    if (event.key === 'ArrowRight') { event.preventDefault(); goTo(index + 1); }
  });
  viewport.addEventListener('scroll', () => {
    window.clearTimeout(settleTimer);
    settleTimer = window.setTimeout(() => {
      const width = viewport.clientWidth || 1;
      update(Math.round(viewport.scrollLeft / width));
    }, 70);
  }, { passive: true });
  let pointerStart = null;
  let scrollStart = 0;
  viewport.addEventListener('pointerdown', (event) => {
    if (event.pointerType === 'touch') return;
    pointerStart = event.clientX;
    scrollStart = viewport.scrollLeft;
    viewport.classList.add('dragging');
    viewport.setPointerCapture?.(event.pointerId);
  });
  viewport.addEventListener('pointermove', (event) => {
    if (pointerStart === null) return;
    viewport.scrollLeft = scrollStart - (event.clientX - pointerStart);
  });
  const endDrag = () => { pointerStart = null; viewport.classList.remove('dragging'); };
  viewport.addEventListener('pointerup', endDrag);
  viewport.addEventListener('pointercancel', endDrag);
  window.addEventListener('resize', () => goTo(index, 'auto'));
  update(0);
  return { goTo, get index() { return index; } };
}

function renderDemo() {
  const track = document.querySelector('#demo-track');
  track.replaceChildren();
  DEMO_SLIDES.forEach((slide, index) => {
    const article = document.createElement('article');
    article.className = 'carousel-slide demo-slide';
    const media = slide.images.length === 1
      ? `<div class="demo-media"><img src="${slide.images[0]}" alt="${slide.alt}"></div>`
      : `<div class="demo-media"><div class="demo-gallery">${slide.images.map((src, i) => `<img src="${src}" alt="${slide.alt} — vue ${i + 1}">`).join('')}</div></div>`;
    article.innerHTML = `${media}<div class="demo-copy"><span class="slide-label">${String(index + 1).padStart(2,'0')} · ${slide.label}</span><h3>${slide.title}</h3><p>${slide.copy}</p><ul class="demo-bullets">${slide.bullets.map((item) => `<li>${item}</li>`).join('')}</ul></div>`;
    track.append(article);
  });
  demoCarousel = createCarousel({
    viewport: document.querySelector('#demo-viewport'), track,
    prev: document.querySelector('#demo-prev'), next: document.querySelector('#demo-next'),
    dots: document.querySelector('#demo-dots'), counter: document.querySelector('#demo-counter'), count: DEMO_SLIDES.length,
    counterText: (i, total) => `${String(i + 1).padStart(2,'0')} / ${String(total).padStart(2,'0')}`,
  });
}

function renderFeatures() {
  const tabs = document.querySelector('#feature-tabs');
  const track = document.querySelector('#feature-track');
  tabs.replaceChildren(); track.replaceChildren();
  FEATURE_CATEGORIES.forEach((feature, index) => {
    const tab = document.createElement('button');
    tab.type = 'button'; tab.role = 'tab'; tab.textContent = feature.title;
    tab.addEventListener('click', () => featureCarousel.goTo(index));
    tabs.append(tab);
    const slide = document.createElement('article');
    slide.className = 'carousel-slide feature-slide';
    const highlights = feature.highlights.map(([title, copy], itemIndex) => `<article class="${feature.featured === itemIndex ? 'featured' : ''}"><strong>${title}</strong><p>${copy}</p></article>`).join('');
    slide.innerHTML = `<div class="feature-intro"><span class="feature-count">${feature.count}</span><h3>${feature.title}</h3><p>${feature.lead}</p><div class="feature-tags">${feature.tags.map((tag) => `<span>${tag}</span>`).join('')}</div>${feature.coming ? '<span class="feature-coming">À VENIR</span>' : ''}</div><div class="feature-highlights">${highlights}</div>`;
    track.append(slide);
  });
  featureCarousel = createCarousel({
    viewport: document.querySelector('#feature-viewport'), track,
    prev: document.querySelector('#feature-prev'), next: document.querySelector('#feature-next'),
    counter: document.querySelector('#feature-counter'), count: FEATURE_CATEGORIES.length,
    counterText: (i, total) => `${String(i + 1).padStart(2,'0')} / ${String(total).padStart(2,'0')}`,
    onIndex: (index) => [...tabs.children].forEach((tab, i) => { tab.classList.toggle('active', i === index); tab.setAttribute('aria-selected', String(i === index)); if (i === index) tab.scrollIntoView({ behavior: 'smooth', inline: 'center', block: 'nearest' }); }),
  });
}

function renderEngines() {
  const node = document.querySelector('#engine-list');
  node.replaceChildren(...ENGINES.map((name) => { const span = document.createElement('span'); span.textContent = name; return span; }));
  document.querySelector('#engine-toggle').addEventListener('click', (event) => {
    const open = event.currentTarget.getAttribute('aria-expanded') === 'true';
    event.currentTarget.setAttribute('aria-expanded', String(!open));
    event.currentTarget.textContent = open ? 'Voir les 16' : 'Réduire';
    node.hidden = open;
  });
}

function renderGuideTabs() {
  const tabs = document.querySelector('#guide-platform-tabs');
  tabs.replaceChildren();
  Object.entries(GUIDES).forEach(([key, guide]) => {
    const button = document.createElement('button');
    button.type = 'button'; button.role = 'tab';
    button.innerHTML = `${guide.title} <small>${guide.label}</small>`;
    button.classList.toggle('active', key === currentGuide);
    button.setAttribute('aria-selected', String(key === currentGuide));
    button.addEventListener('click', () => { currentGuide = key; renderGuideTabs(); renderGuide(); });
    tabs.append(button);
  });
}

function renderGuide() {
  const guide = GUIDES[currentGuide];
  const track = document.querySelector('#guide-track');
  track.replaceChildren();
  guide.steps.forEach(([title, copy, image, note], index) => {
    const slide = document.createElement('article');
    slide.className = 'carousel-slide guide-slide';
    slide.innerHTML = `<div class="guide-image"><img src="${image}" alt="${guide.title} — étape ${index + 1} : ${title}"></div><div class="guide-copy"><span class="slide-label">${guide.title.toUpperCase()} · ÉTAPE ${index + 1}</span><h3>${title}</h3><p>${copy}</p><div class="guide-note">${note}</div></div>`;
    track.append(slide);
  });
  guideCarousel = createCarousel({
    viewport: document.querySelector('#guide-viewport'), track,
    prev: document.querySelector('#guide-step-prev'), next: document.querySelector('#guide-step-next'),
    dots: document.querySelector('#guide-dots'), counter: document.querySelector('#guide-step-counter'), count: guide.steps.length,
    counterText: (i, total) => `Étape ${i + 1} / ${total}`,
  });
  document.querySelector('#guide-step-next').textContent = 'Suivant';
}

async function loadManifest() {
  manifest = await fetchReleaseManifest();
  const state = document.querySelector('#release-state');
  if (manifest.preview) {
    document.querySelector('#manifest-version').textContent = 'Aperçu local · les téléchargements sont désactivés';
    state.lastChild.textContent = 'Prévisualisation locale';
  } else {
    const publishedAt = new Date(manifest.publishedAt);
    document.querySelector('#manifest-version').textContent = `Version ${manifest.version} · ${publishedAt.toLocaleDateString('fr-FR')}`;
    state.classList.add('ready');
    state.lastChild.textContent = `Release ${manifest.version} vérifiée`;
  }
  updateDownloadExperience();
  renderPlatforms();
}

function updateDownloadExperience() {
  const title = document.querySelector('#device-title');
  const subtitle = document.querySelector('#device-subtitle');
  const summary = document.querySelector('#device-summary');
  const macChoice = document.querySelector('#mac-architecture-choice');
  const override = document.querySelector('#platform-override');
  const detected = deviceProfile.platform;
  summary.dataset.state = detected ? 'detected' : deviceProfile.operatingSystem ? 'partial' : 'unknown';
  if (detected) {
    const labels = PLATFORM_LABELS[detected];
    title.textContent = `${labels[0]} ${labels[1]} détecté`;
    subtitle.textContent = `${labels[2]} · version recommandée sélectionnée`;
  } else if (deviceProfile.operatingSystem === 'macOS') {
    title.textContent = 'macOS détecté'; subtitle.textContent = 'Choisissez Apple Silicon ou Intel.';
  } else if (deviceProfile.operatingSystem) {
    title.textContent = `${deviceProfile.operatingSystem} détecté`;
    subtitle.textContent = deviceProfile.architecture ? `${architectureLabel(deviceProfile.architecture)} détecté` : 'Choisissez l’architecture correspondante.';
  } else {
    title.textContent = 'Appareil non identifié'; subtitle.textContent = 'Toutes les plateformes restent accessibles.';
  }
  macChoice.hidden = !(deviceProfile.operatingSystem === 'macOS' && !detected);
  override.hidden = !deviceProfile.operatingSystem;
  override.textContent = showAllPlatforms ? 'Reverrouiller selon cet appareil' : 'Télécharger pour un autre appareil';
  override.setAttribute('aria-pressed', String(showAllPlatforms));
  updateRecommendedDownload();
  if (manifest) renderPlatforms();
}

function updateRecommendedDownload() {
  const platform = deviceProfile.platform;
  const primary = document.querySelector('#download-primary');
  const labels = platform ? PLATFORM_LABELS[platform] : null;
  const setup = platform ? manifest?.platforms?.[platform]?.setup : null;
  document.querySelector('#hero-platform-icon').textContent = platform ? iconForPlatform(platform) : 'F';
  document.querySelector('#hero-platform-os').textContent = labels?.[0] || 'FileFlow Setup';
  document.querySelector('#hero-platform-name').textContent = labels ? `${labels[1]} (${labels[2]})` : deviceProfile.operatingSystem === 'macOS' ? 'Apple Silicon ou Intel' : 'Choisissez votre plateforme';
  document.querySelector('#hero-platform-badge').textContent = platform ? 'Recommandé pour cet appareil' : deviceProfile.operatingSystem === 'macOS' ? 'Sélection manuelle' : 'Toutes les versions disponibles';
  if (setup?.url) {
    primary.href = setup.url; primary.removeAttribute('aria-disabled'); primary.dataset.sha256 = setup.sha256;
    document.querySelector('#download-primary-label').textContent = 'Télécharger FileFlow Setup';
    document.querySelector('#download-detail').textContent = `${labels.join(' · ')} · ${bytes(setup.size)}`;
    document.querySelector('#hero-support-note').textContent = `Release ${manifest.version} · SHA-256 publié`;
  } else {
    primary.href = '#platform-grid'; primary.setAttribute('aria-disabled','true');
    document.querySelector('#download-primary-label').textContent = deviceProfile.operatingSystem === 'macOS' ? 'Choisir Apple Silicon ou Intel' : 'Choisir une plateforme';
    document.querySelector('#download-detail').textContent = manifest?.preview ? 'Aperçu local' : 'Sélectionnez votre appareil ci-contre';
    document.querySelector('#hero-support-note').textContent = 'macOS · Windows · Linux';
  }
}

function manualMacSelection(platform) {
  deviceProfile = {
    ...deviceProfile, operatingSystem: 'macOS', architecture: platform === 'darwin-aarch64' ? 'arm64' : 'x64',
    platform, processor: platform === 'darwin-aarch64' ? 'Apple Silicon' : null,
    confidence: 'manual', detectionSource: 'manual', conflictingEvidence: false,
  };
  currentGuide = 'macos'; showAllPlatforms = false;
  renderGuideTabs(); renderGuide(); updateDownloadExperience();
  toast(platform === 'darwin-aarch64' ? 'Apple Silicon sélectionné' : 'Mac Intel sélectionné');
}

function lockReason(platform) {
  const labels = PLATFORM_LABELS[platform];
  if (PLATFORM_OS[platform] !== deviceProfile.operatingSystem) return `${labels[0]} uniquement`;
  if (deviceProfile.platform && platform !== deviceProfile.platform) return `Votre appareil utilise ${PLATFORM_LABELS[deviceProfile.platform][1]}`;
  return 'Version non recommandée pour cet appareil';
}

function renderPlatforms() {
  if (!manifest) return;
  const grid = document.querySelector('#platform-grid');
  grid.replaceChildren();
  for (const [key, download] of Object.entries(manifest.platforms)) {
    const labels = PLATFORM_LABELS[key] || [key,'',''];
    const access = platformAccessState(key, deviceProfile, showAllPlatforms);
    const locked = access === 'locked';
    const card = document.createElement('article');
    card.className = `platform-card ${access}`;
    if (access === 'recommended' || locked) {
      const tag = document.createElement('span'); tag.className = `tag ${locked ? 'lock-tag' : ''}`; tag.textContent = locked ? 'VERROUILLÉ' : 'RECOMMANDÉ'; card.append(tag);
    }
    const mark = document.createElement('div'); mark.className = 'platform-card__mark'; mark.textContent = iconForPlatform(key);
    const title = document.createElement('h3'); title.textContent = `${labels[0]} · ${labels[1]}`;
    const desc = document.createElement('p'); desc.textContent = locked ? lockReason(key) : `${labels[2]} · Setup guidé`;
    const link = document.createElement('a');
    if (download?.setup?.url && !locked) { link.href = download.setup.url; link.textContent = access === 'recommended' ? 'Télécharger' : 'Setup'; }
    else { link.setAttribute('aria-disabled','true'); link.textContent = locked ? 'Verrouillé' : 'Indisponible'; }
    const copy = document.createElement('button'); copy.type = 'button'; copy.textContent = 'SHA-256'; copy.disabled = locked || !download?.setup?.sha256;
    if (!copy.disabled) copy.addEventListener('click', () => copyText(download.setup.sha256, 'SHA-256 copié'));
    const details = document.createElement('small'); details.textContent = !locked && download?.setup ? `${bytes(download.setup.size)} · ${download.setup.sha256.slice(0,12)}…` : locked ? lockReason(key) : 'Aucun artefact disponible';
    card.append(mark,title,desc,link,copy,details); grid.append(card);
  }
}

async function copyText(value, successMessage) {
  try { await navigator.clipboard.writeText(value); toast(successMessage); }
  catch { toast('Copie impossible : sélectionnez le texte manuellement.'); }
}

async function verifyFile(file) {
  if (!file || !manifest) return;
  const drop = document.querySelector('.verify-drop'); const result = document.querySelector('#verify-result');
  drop.classList.remove('success','error'); result.textContent = 'Calcul SHA-256 en cours…';
  if (manifest.preview) { drop.classList.add('error'); result.textContent = 'Vérification active uniquement avec une release publique.'; return; }
  try {
    const digest = await crypto.subtle.digest('SHA-256', await file.arrayBuffer());
    const hash = [...new Uint8Array(digest)].map((value) => value.toString(16).padStart(2,'0')).join('');
    const artifacts = Object.values(manifest.platforms).flatMap((item) => [item.application,item.setup,item.cli].filter(Boolean));
    const match = artifacts.find((item) => item.sha256.toLowerCase() === hash);
    drop.classList.add(match ? 'success' : 'error');
    result.textContent = match ? `✓ Authentique — ${match.name}` : `✕ Aucun artefact de la release ${manifest.version} ne correspond`;
  } catch { drop.classList.add('error'); result.textContent = '✕ Ce fichier n’a pas pu être vérifié.'; }
}

renderDemo();
renderFeatures();
renderEngines();
currentGuide = defaultGuidePlatform();
renderGuideTabs();
renderGuide();

document.querySelector('#engine-toggle').setAttribute('aria-controls','engine-list');
document.querySelector('#platform-override').addEventListener('click', () => { showAllPlatforms = !showAllPlatforms; updateDownloadExperience(); toast(showAllPlatforms ? 'Toutes les plateformes sont accessibles' : 'Filtrage automatique réactivé'); });
document.querySelectorAll('[data-mac-platform]').forEach((button) => button.addEventListener('click', () => manualMacSelection(button.dataset.macPlatform)));
document.querySelector('#copy-command').addEventListener('click', () => {
  const portal = globalThis.location?.origin || 'https://fileflow.idris-achabou.fit';
  const command = deviceProfile.platform === 'windows-x86_64' ? `irm ${portal}/install.ps1 | iex` : `curl -fsSL ${portal}/install.sh | sh`;
  copyText(command,'Commande terminal copiée');
});
document.querySelector('#guide-step-cancel').addEventListener('click', () => { guideCarousel.goTo(0); toast('Guide réinitialisé'); });
const fileInput = document.querySelector('#verify-file'); const dropZone = document.querySelector('.verify-drop');
fileInput.addEventListener('change', (event) => verifyFile(event.target.files?.[0]));
for (const name of ['dragenter','dragover']) dropZone.addEventListener(name, (event) => { event.preventDefault(); dropZone.classList.add('dragging'); });
for (const name of ['dragleave','drop']) dropZone.addEventListener(name, (event) => { event.preventDefault(); dropZone.classList.remove('dragging'); });
dropZone.addEventListener('drop', (event) => verifyFile(event.dataTransfer?.files?.[0]));

detectDeviceProfile().then((profile) => {
  deviceProfile = profile;
  currentGuide = defaultGuidePlatform();
  renderGuideTabs(); renderGuide(); updateDownloadExperience();
}).catch(() => updateDownloadExperience());

loadManifest().catch((error) => {
  document.querySelector('#release-state').lastChild.textContent = 'Release temporairement indisponible';
  document.querySelector('#manifest-version').textContent = error.message;
  manifest = { preview:true, version:null, platforms:Object.fromEntries(Object.keys(PLATFORM_LABELS).map((key) => [key,{}])) };
  updateDownloadExperience(); renderPlatforms();
});
