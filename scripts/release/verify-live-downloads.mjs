#!/usr/bin/env node
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(fileURLToPath(new URL('../..', import.meta.url)));

function sleep(milliseconds) {
  return new Promise((resolveDelay) => setTimeout(resolveDelay, milliseconds));
}

async function fetchWithTimeout(url, options = {}, timeoutMs = 15000) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), timeoutMs);
  try {
    return await fetch(url, { redirect: 'follow', ...options, signal: controller.signal });
  } finally {
    clearTimeout(timeout);
  }
}

export function validateDownloadManifest(manifest, { expectedVersion, requiredPlatforms = [] } = {}) {
  if (!manifest || manifest.schemaVersion !== 1) throw new Error('downloads.json schemaVersion must be 1');
  if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(String(manifest.version || ''))) {
    throw new Error(`downloads.json has an invalid version: ${manifest.version ?? 'missing'}`);
  }
  if (expectedVersion && manifest.version !== expectedVersion.replace(/^v/, '')) {
    throw new Error(`downloads.json version ${manifest.version} does not match ${expectedVersion}`);
  }
  for (const platform of requiredPlatforms) {
    if (!manifest.platforms?.[platform]) throw new Error(`downloads.json missing ${platform}`);
  }
  for (const [platform, downloads] of Object.entries(manifest.platforms || {})) {
    const artifacts = [
      ['application', downloads?.application],
      ['setup', downloads?.setup],
      ...Object.entries(downloads?.setupVariants || {}).map(([type, artifact]) => [`setupVariants.${type}`, artifact]),
    ];
    for (const [type, artifact] of artifacts) {
      if (!artifact) throw new Error(`${platform} has no ${type} artifact`);
      const url = new URL(String(artifact.url));
      if (url.protocol !== 'https:' && !(url.protocol === 'http:' && ['127.0.0.1', 'localhost'].includes(url.hostname))) {
        throw new Error(`${platform}/${type} does not use HTTPS`);
      }
      if (!/^[0-9a-f]{64}$/i.test(String(artifact.sha256 || ''))) throw new Error(`${platform}/${type} has invalid SHA-256`);
      if (!Number.isSafeInteger(artifact.size) || artifact.size <= 0) throw new Error(`${platform}/${type} has invalid size`);
    }
  }
  return manifest;
}

async function artifactReachable(artifact, timeoutMs) {
  let response = await fetchWithTimeout(artifact.url, { method: 'HEAD', headers: { 'Cache-Control': 'no-cache' } }, timeoutMs);
  if (response.status === 405 || response.status === 403) {
    response = await fetchWithTimeout(artifact.url, { headers: { Range: 'bytes=0-0', 'Cache-Control': 'no-cache' } }, timeoutMs);
  }
  if (!response.ok && response.status !== 206) throw new Error(`${artifact.name} returned HTTP ${response.status}`);
}

export async function verifyLiveDownloads({ endpoint, expectedVersion, requiredPlatforms = [], attempts = 1, delayMs = 3000, timeoutMs = 15000 }) {
  let manifest;
  let lastError;
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    try {
      const separator = endpoint.includes('?') ? '&' : '?';
      const response = await fetchWithTimeout(`${endpoint}${separator}fileflow_probe=${Date.now()}`, { headers: { Accept: 'application/json', 'Cache-Control': 'no-cache' } }, timeoutMs);
      if (!response.ok) throw new Error(`downloads endpoint returned HTTP ${response.status}`);
      manifest = validateDownloadManifest(await response.json(), { expectedVersion, requiredPlatforms });
      break;
    } catch (error) {
      lastError = error;
      if (attempt < attempts) await sleep(delayMs);
    }
  }
  if (!manifest) throw lastError;
  for (const [platform, downloads] of Object.entries(manifest.platforms)) {
    await artifactReachable(downloads.application, timeoutMs);
    await artifactReachable(downloads.setup, timeoutMs);
    for (const artifact of Object.values(downloads.setupVariants || {})) {
      await artifactReachable(artifact, timeoutMs);
    }
    console.log(`[downloads-live] ${platform}: application + setup variants reachable`);
  }
  console.log(`[downloads-live] PASS ${manifest.version}: portal manifest and artifacts reachable`);
  return manifest;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = new Map();
  for (let index = 2; index < process.argv.length; index += 2) args.set(process.argv[index], process.argv[index + 1]);
  const config = JSON.parse(readFileSync(resolve(root, 'src-tauri/tauri.conf.json'), 'utf8'));
  const repository = args.get('--repository') || 'idris-ach2002/FileFlow';
  const endpoint = args.get('--endpoint') || `https://github.com/${repository}/releases/latest/download/downloads.json`;
  const requiredPlatforms = (args.get('--require-platforms') || '').split(',').map((value) => value.trim()).filter(Boolean);
  await verifyLiveDownloads({
    endpoint,
    expectedVersion: args.get('--expected-version') || config.version,
    requiredPlatforms,
    attempts: Number(args.get('--attempts') || '1'),
    delayMs: Number(args.get('--delay-ms') || '3000'),
    timeoutMs: Number(args.get('--timeout-ms') || '15000'),
  });
}
