import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { onRequestGet } from '../functions/api/downloads.js';
import {
  classifyGraphicsRenderer,
  detectDeviceProfile,
  detectOperatingSystem,
  detectPlatform,
  platformAccessState,
} from '../public/platform.js';
import { fetchReleaseManifest } from '../public/release-client.js';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repositoryRoot = resolve(root, '..');
const required = [
  'index.html', 'styles.css', 'interactions.css', 'app.js', 'platform.js',
  'install.sh', 'install.ps1', 'release-client.js', '_headers', '_redirects',
];
for (const name of required) {
  if (!existsSync(resolve(root, 'public', name))) throw new Error(`missing website/public/${name}`);
}

const html = readFileSync(resolve(root, 'public/index.html'), 'utf8');
for (const marker of [
  'download-primary', 'platform-grid', 'verify-file', 'installation-guide',
  'device-summary', 'mac-architecture-choice', 'platform-override',
  'demo-viewport', 'demo-track', 'feature-viewport', 'feature-track',
  'guide-viewport', 'guide-track', 'mobile-download-cta',
]) {
  if (!html.includes(marker)) throw new Error(`download portal is missing #${marker}`);
}
for (const asset of [
  'assets/app/home.webp', 'assets/app/conversion.webp', 'assets/app/advanced-overview.webp',
  'assets/app/advanced-images.webp', 'assets/app/formats.webp', 'assets/app/advanced-media.webp',
  'assets/app/advanced-archives.webp', 'assets/app/history.webp', 'assets/app/updates.webp',
  'assets/app/engines.webp',
  'assets/guides/windows/01.svg', 'assets/guides/windows/02.svg',
  'assets/guides/windows/03.svg', 'assets/guides/windows/04.svg',
  'assets/guides/linux/01.svg', 'assets/guides/linux/02.svg',
  'assets/guides/linux/03.svg', 'assets/guides/linux/04.svg',
]) {
  if (!existsSync(resolve(root, 'public', asset))) throw new Error(`missing website/public/${asset}`);
}
const script = readFileSync(resolve(root, 'public/app.js'), 'utf8');
if (!script.includes('fetchReleaseManifest()')) throw new Error('portal must use the guarded release client');
if (!script.includes('crypto.subtle.digest')) throw new Error('portal must verify SHA-256 locally');
if (!script.includes('platformAccessState')) throw new Error('portal must lock incompatible platforms conservatively');
if (!script.includes('showAllPlatforms')) throw new Error('portal must allow an explicit cross-device override');
if (!script.includes("scroll-snap") && !readFileSync(resolve(root, 'public/styles.css'), 'utf8').includes('scroll-snap-type:x mandatory')) {
  throw new Error('product and installation demos must move horizontally');
}
if (!script.includes('TAR.ZST') || !script.includes('Zstandard')) throw new Error('archive feature carousel must surface Zstandard/TAR.ZST');
assert.doesNotMatch(html, /iLovePDF|Smallpdf|CloudConvert|TinyPNG|FreeConvert/i, 'portal must not compare FileFlow with named competitor sites');
assert.ok(!/<script(?![^>]*\bsrc=)[^>]*>/iu.test(html), 'CSP forbids inline scripts');
assert.ok(!/\son[a-z]+\s*=/iu.test(html), 'CSP forbids inline event handlers');
const headersPolicy = readFileSync(resolve(root, 'public/_headers'), 'utf8');
assert.match(headersPolicy, /script-src-elem 'self'/);
assert.match(headersPolicy, /script-src-attr 'none'/);
assert.match(headersPolicy, /\/\n\s+Cache-Control: no-cache, must-revalidate/, 'HTML must always revalidate so it can advertise the newest asset fingerprint');
const buildScript = readFileSync(resolve(root, 'scripts/build.mjs'), 'utf8');
for (const marker of ['sourceFingerprint', 'versionAssets', 'cache fingerprint', "./platform.js?v=", "./release-client.js?v="]) {
  assert.ok(buildScript.includes(marker), `site build must cache-bust ${marker}`);
}
assert.equal(detectOperatingSystem({ userAgent: 'Mozilla/5.0 (Macintosh)', platform: 'MacIntel' }), 'macOS');

assert.deepEqual(
  classifyGraphicsRenderer('ANGLE (Apple, Apple M2 Pro, Metal)'),
  { architecture: 'arm64', processor: 'Apple Silicon' },
  'GPU renderer names prove Apple Silicon architecture, not the exact SoC generation',
);
assert.deepEqual(
  classifyGraphicsRenderer('ANGLE (Apple, Apple M1, Metal)'),
  { architecture: 'arm64', processor: 'Apple Silicon' },
  'privacy-reduced Apple M1 renderers must never be presented as an exact M1 CPU',
);
assert.deepEqual(
  classifyGraphicsRenderer('Intel Iris Plus Graphics 655'),
  { architecture: 'x64', processor: null },
);
const appleSiliconProfile = await detectDeviceProfile({
  userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X) AppleWebKit/605.1.15',
  platform: 'MacIntel',
}, { graphicsRenderer: 'ANGLE (Apple, Apple M2 Pro, Metal)' });
assert.equal(appleSiliconProfile.platform, 'darwin-aarch64');
assert.equal(appleSiliconProfile.processor, 'Apple Silicon');
assert.equal(platformAccessState('darwin-aarch64', appleSiliconProfile), 'recommended');
assert.equal(platformAccessState('darwin-x86_64', appleSiliconProfile), 'locked');
assert.equal(platformAccessState('windows-x86_64', appleSiliconProfile), 'locked');
assert.equal(platformAccessState('windows-x86_64', appleSiliconProfile, true), 'available');
const maskedMacProfile = { operatingSystem: 'macOS', architecture: null, platform: null };
assert.equal(platformAccessState('darwin-aarch64', maskedMacProfile), 'compatible');
assert.equal(platformAccessState('darwin-x86_64', maskedMacProfile), 'compatible');
assert.equal(platformAccessState('linux-x86_64', maskedMacProfile), 'locked');

assert.equal(await detectPlatform({
  userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X) AppleWebKit/605.1.15',
  platform: 'MacIntel',
}), null, 'AppleWebKit must never be treated as Apple Silicon');
assert.equal(await detectPlatform({ userAgent: 'FileFlow x86_64', platform: 'MacIntel' }), 'darwin-x86_64');
assert.equal(await detectPlatform({
  userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X) AppleWebKit/605.1.15',
  platform: 'MacIntel',
  userAgentData: { getHighEntropyValues: async () => ({ architecture: 'arm', bitness: '64' }) },
}), 'darwin-aarch64');
assert.equal(await detectPlatform({ userAgent: 'Mozilla/5.0 (X11; Linux aarch64)', platform: 'Linux armv8l' }), 'linux-aarch64');
assert.equal(await detectPlatform({ userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)', platform: 'Win32' }), 'windows-x86_64');
const stalledHintsStarted = Date.now();
assert.equal(await detectPlatform({
  userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X)',
  platform: 'MacIntel',
  userAgentData: { getHighEntropyValues: () => new Promise(() => {}) },
}, 15), null);
assert.ok(Date.now() - stalledHintsStarted < 250, 'platform hints must never block the portal');

const repository = 'idris-ach2002/FileFlow';
const version = '1.2.3';
const platforms = ['darwin-aarch64', 'darwin-x86_64', 'windows-x86_64', 'linux-x86_64', 'linux-aarch64'];
const manifest = {
  schemaVersion: 1,
  version,
  publishedAt: '2026-08-24T12:00:00.000Z',
  repository,
  platforms: Object.fromEntries(platforms.map((platform) => [platform, {
    application: artifact(`FileFlow-${platform}`),
    setup: artifact(`FileFlow-Setup-${platform}`),
  }])),
};

function artifact(name) {
  return {
    name,
    url: `https://github.com/${repository}/releases/download/v${version}/${name}`,
    sha256: 'a'.repeat(64),
    size: 42,
  };
}

function context() {
  return {
    env: { FILEFLOW_REPOSITORY: repository },
    request: new Request('https://fileflow-downloads.pages.dev/api/downloads'),
    waitUntil: () => {},
  };
}

function localContext() {
  return {
    env: { FILEFLOW_REPOSITORY: repository },
    request: new Request('http://localhost:8788/api/downloads'),
    waitUntil: () => {},
  };
}

globalThis.caches = { default: { match: async () => null, put: async () => {} } };
let fetchCalled = false;
globalThis.fetch = async () => { fetchCalled = true; throw new Error('must not fetch'); };
let response = await onRequestGet(localContext());
assert.equal(response.status, 200);
const preview = await response.json();
assert.equal(preview.preview, true);
assert.equal(Object.keys(preview.platforms).length, 5);
assert.equal(fetchCalled, false, 'local preview must not depend on a public release');

globalThis.fetch = async () => Response.json(manifest);
response = await onRequestGet(context());
assert.equal(response.status, 200);
assert.equal(response.headers.get('x-content-type-options'), 'nosniff');
assert.deepEqual((await response.json()).platforms['darwin-aarch64'].setup.name, 'FileFlow-Setup-darwin-aarch64');

globalThis.fetch = async () => Response.json({ ...manifest, platforms: { 'darwin-aarch64': manifest.platforms['darwin-aarch64'] } });
response = await onRequestGet(context());
assert.equal(response.status, 502, 'an incomplete five-platform release must be rejected');

globalThis.fetch = async () => { throw new Error('offline'); };
response = await onRequestGet(context());
assert.equal(response.status, 503, 'an upstream network failure must be presented cleanly');
assert.equal(response.headers.get('cache-control'), 'no-store');

let requestIndex = 0;
globalThis.fetch = async () => {
  requestIndex += 1;
  if (requestIndex === 1) return new Response('Not Found', { status: 404 });
  if (requestIndex === 2) {
    return Response.json([{
      draft: false,
      prerelease: false,
      assets: [{
        name: 'downloads.json',
        browser_download_url: `https://github.com/${repository}/releases/download/v${version}/downloads.json`,
      }],
    }]);
  }
  return Response.json(manifest);
};
response = await onRequestGet(context());
assert.equal(response.status, 200, 'portal must recover the newest complete stable manifest');

await assert.rejects(
  fetchReleaseManifest(async () => new Response('<!doctype html><title>FileFlow</title>', {
    status: 200,
    headers: { 'Content-Type': 'text/html' },
  })),
  /page HTML/,
  'an HTML fallback must never surface as a raw JSON.parse error',
);

const deployWorkflow = readFileSync(resolve(repositoryRoot, '.github/workflows/site-cloudflare.yml'), 'utf8');
assert.doesNotMatch(deployWorkflow, /cloudflare\/wrangler-action/, 'CI must not mutate the pnpm workspace to install Wrangler');
assert.match(deployWorkflow, /working-directory: website/);
assert.match(deployWorkflow, /CLOUDFLARE_API_TOKEN: \$\{\{ secrets\.CLOUDFLARE_API_TOKEN \}\}/);
assert.match(deployWorkflow, /CLOUDFLARE_ACCOUNT_ID: \$\{\{ secrets\.CLOUDFLARE_ACCOUNT_ID \}\}/);
assert.match(deployWorkflow, /CLOUDFLARE_API_TOKEN:-/);
assert.match(deployWorkflow, /CLOUDFLARE_ACCOUNT_ID:-/);
assert.match(deployWorkflow, /Cloudflare Pages deployment skipped/);
assert.match(deployWorkflow, /npx --yes wrangler@4\.125\.0 pages deploy dist --project-name=fileflow-downloads/);

const redirects = readFileSync(resolve(root, 'public/_redirects'), 'utf8');
assert.ok(!redirects.includes('/*'), 'a static catch-all must not hide a missing API function');

console.log('[site] structure, conservative platform detection, release proxy and local checksum verifier OK');
