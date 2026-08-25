import { createHash } from 'node:crypto';
import { cpSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { dirname, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const source = resolve(root, 'public');
const output = resolve(root, 'dist');

function walk(directory) {
  return readdirSync(directory)
    .flatMap((name) => {
      const path = resolve(directory, name);
      return statSync(path).isDirectory() ? walk(path) : [path];
    })
    .sort();
}

function sourceFingerprint() {
  const hash = createHash('sha256');
  for (const path of walk(source)) {
    hash.update(relative(source, path));
    hash.update('\0');
    hash.update(readFileSync(path));
    hash.update('\0');
  }
  return hash.digest('hex').slice(0, 16);
}

function versionAssets(text, version) {
  return text.replace(/(["'])\/assets\/([^"'?]+)\1/g, (_match, quote, asset) => (
    `${quote}/assets/${asset}?v=${version}${quote}`
  ));
}

const version = sourceFingerprint();

rmSync(output, { recursive: true, force: true });
mkdirSync(output, { recursive: true });
cpSync(source, output, { recursive: true });

const indexPath = resolve(output, 'index.html');
let html = readFileSync(indexPath, 'utf8');
for (const asset of ['styles.css', 'interactions.css', 'app.js']) {
  html = html.replaceAll(`/${asset}`, `/${asset}?v=${version}`);
}
html = versionAssets(html, version);
writeFileSync(indexPath, html);

const appPath = resolve(output, 'app.js');
let app = readFileSync(appPath, 'utf8');
app = app
  .replace("from './platform.js';", `from './platform.js?v=${version}';`)
  .replace("from './release-client.js';", `from './release-client.js?v=${version}';`);
app = versionAssets(app, version);
writeFileSync(appPath, app);

const builtHtml = readFileSync(indexPath, 'utf8');
const builtApp = readFileSync(appPath, 'utf8');
for (const required of [
  `/styles.css?v=${version}`,
  `/interactions.css?v=${version}`,
  `/app.js?v=${version}`,
  `/assets/app/home.webp?v=${version}`,
]) {
  if (!builtHtml.includes(required)) throw new Error(`[site] missing cache-busted reference: ${required}`);
}
for (const required of [
  `./platform.js?v=${version}`,
  `./release-client.js?v=${version}`,
  `/assets/app/home.webp?v=${version}`,
  `/assets/guides/windows/01.svg?v=${version}`,
  `/assets/guides/linux/01.svg?v=${version}`,
]) {
  if (!builtApp.includes(required)) throw new Error(`[site] missing cache-busted module/asset: ${required}`);
}

console.log(`[site] Cloudflare Pages bundle -> ${output}`);
console.log(`[site] cache fingerprint -> ${version}`);
