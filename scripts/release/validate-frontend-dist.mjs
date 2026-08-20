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
