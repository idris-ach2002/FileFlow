import fs from 'node:fs';
import path from 'node:path';

const root = process.cwd();
const dist = path.join(root, 'frontend', 'dist', 'fileflow', 'browser');
const indexPath = path.join(dist, 'index.html');

function fail(message) {
  console.error(`[FAIL] ${message}`);
  process.exit(1);
}

if (!fs.existsSync(indexPath)) {
  fail(`index.html introuvable : ${indexPath}`);
}

const html = fs.readFileSync(indexPath, 'utf8');

const base = html.match(/<base\s+href=["']([^"']+)["']/i)?.[1];

if (base !== './') {
  fail(`base href release invalide : ${JSON.stringify(base)} ; attendu "./"`);
}

console.log(`[OK] base href : ${base}`);

const refs = [];

for (const match of html.matchAll(/<link[^>]+href=["']([^"']+)["']/gi)) {
  refs.push(match[1]);
}

for (const match of html.matchAll(/<script[^>]+src=["']([^"']+)["']/gi)) {
  refs.push(match[1]);
}

if (!refs.length) {
  fail('aucun asset CSS/JS trouvé dans index.html');
}

let cssCount = 0;
let jsCount = 0;

for (const ref of refs) {
  if (
    ref.startsWith('http://') ||
    ref.startsWith('https://') ||
    ref.startsWith('//')
  ) {
    continue;
  }

  if (ref.startsWith('/')) {
    fail(`asset absolu interdit pour desktop : ${ref}`);
  }

  const normalized = ref.replace(/^\.\//, '');
  const file = path.join(dist, normalized);

  if (!fs.existsSync(file)) {
    fail(`asset référencé mais absent : ${ref}`);
  }

  const stat = fs.statSync(file);

  if (stat.size === 0) {
    fail(`asset vide : ${ref}`);
  }

  if (ref.endsWith('.css')) {
    cssCount++;

    if (stat.size < 500) {
      fail(`CSS anormalement petit : ${ref} (${stat.size} octets)`);
    }

    console.log(`[OK] CSS : ${ref} — ${stat.size} octets`);
  }

  if (ref.endsWith('.js')) {
    jsCount++;
    console.log(`[OK] JS  : ${ref} — ${stat.size} octets`);
  }
}

if (cssCount === 0) {
  fail('aucune feuille CSS générée');
}

if (jsCount === 0) {
  fail('aucun bundle JavaScript généré');
}

// Vérifie aussi les lazy chunks générés.
const files = fs.readdirSync(dist);

const lazyChunks = files.filter(
  file => file.startsWith('chunk-') && file.endsWith('.js')
);

console.log(`[OK] lazy chunks : ${lazyChunks.length}`);
console.log('[OK] frontend desktop assets valides');

// Desktop CSP regression guard. Angular component styles require inline style
// support in the WebView, while scripts remain self-only.
const tauriConfig = JSON.parse(fs.readFileSync(path.join(root, 'src-tauri', 'tauri.conf.json'), 'utf8'));
const csp = tauriConfig?.app?.security?.csp ?? '';
if (!/style-src[^;]*'unsafe-inline'/.test(csp)) fail('CSP desktop doit autoriser les styles Angular inline');
if (!/script-src\s+'self'/.test(csp)) fail('CSP desktop doit conserver script-src self uniquement');
const disabled = tauriConfig?.app?.security?.dangerousDisableAssetCspModification ?? [];
if (!Array.isArray(disabled) || !disabled.includes('style-src')) fail('Tauri doit préserver explicitement style-src pour les composants Angular');
console.log('[OK] CSP Angular/Tauri compatible');
