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

function assertHttpUrl(value, label) {
  const url = new URL(value);
  if (url.protocol !== 'https:' && !(url.protocol === 'http:' && ['127.0.0.1', 'localhost'].includes(url.hostname))) {
    throw new Error(`${label} must use HTTPS`);
  }
  return url;
}

export function validateLiveManifest(manifest, { expectedVersion, requiredPlatforms = [] } = {}) {
  if (!manifest || typeof manifest !== 'object') throw new Error('latest.json must contain a JSON object');
  if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(String(manifest.version || ''))) {
    throw new Error(`latest.json has an invalid version: ${manifest.version ?? 'missing'}`);
  }
  if (expectedVersion && manifest.version.replace(/^v/, '') !== expectedVersion.replace(/^v/, '')) {
    throw new Error(`latest.json version ${manifest.version} does not match ${expectedVersion}`);
  }
  if (!manifest.platforms || typeof manifest.platforms !== 'object') {
    throw new Error('latest.json platforms are missing');
  }
  for (const platform of requiredPlatforms) {
    if (!manifest.platforms[platform]) throw new Error(`latest.json missing ${platform}`);
  }
  const entries = Object.entries(manifest.platforms);
  if (!entries.length) throw new Error('latest.json does not expose any platform');
  for (const [platform, item] of entries) {
    if (!item || typeof item !== 'object') throw new Error(`latest.json entry ${platform} is invalid`);
    assertHttpUrl(item.url, `${platform} artifact URL`);
    if (typeof item.signature !== 'string' || item.signature.trim().length < 16) {
      throw new Error(`latest.json entry ${platform} has no usable signature`);
    }
  }
  return entries;
}

export async function verifyLiveUpdater({
  endpoint,
  expectedVersion,
  requiredPlatforms = [],
  attempts = 1,
  delayMs = 3000,
  timeoutMs = 15000,
}) {
  assertHttpUrl(endpoint, 'updater endpoint');
  let lastError;
  let manifest;
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    try {
      const separator = endpoint.includes('?') ? '&' : '?';
      const response = await fetchWithTimeout(`${endpoint}${separator}fileflow_probe=${Date.now()}`, {
        headers: { Accept: 'application/json', 'Cache-Control': 'no-cache' },
      }, timeoutMs);
      if (!response.ok) throw new Error(`updater endpoint returned HTTP ${response.status}`);
      manifest = await response.json();
      validateLiveManifest(manifest, { expectedVersion, requiredPlatforms });
      break;
    } catch (error) {
      lastError = error;
      if (attempt < attempts) await sleep(delayMs);
    }
  }
  if (!manifest) throw new Error(`live updater manifest unavailable: ${lastError instanceof Error ? lastError.message : lastError}`);

  const entries = validateLiveManifest(manifest, { expectedVersion, requiredPlatforms });
  for (const [platform, item] of entries) {
    const response = await fetchWithTimeout(item.url, {
      headers: { Range: 'bytes=0-0', 'Cache-Control': 'no-cache' },
    }, timeoutMs);
    if (![200, 206].includes(response.status)) {
      await response.body?.cancel();
      throw new Error(`${platform} updater artifact returned HTTP ${response.status}`);
    }
    await response.body?.cancel();
    console.log(`[updater-live] ${platform}: artifact reachable`);
  }
  console.log(`[updater-live] PASS ${manifest.version}: manifest and ${entries.length} artifact(s) reachable`);
  return manifest;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = new Map();
  for (let index = 2; index < process.argv.length; index += 2) args.set(process.argv[index], process.argv[index + 1]);
  const config = JSON.parse(readFileSync(resolve(root, 'src-tauri/tauri.conf.json'), 'utf8'));
  const endpoint = args.get('--endpoint') || config.plugins?.updater?.endpoints?.[0];
  if (!endpoint) throw new Error('usage: verify-live-updater.mjs [--endpoint URL] [--expected-version X.Y.Z]');
  const requiredPlatforms = (args.get('--require-platforms') || '').split(',').map((value) => value.trim()).filter(Boolean);
  await verifyLiveUpdater({
    endpoint,
    expectedVersion: args.get('--expected-version'),
    requiredPlatforms,
    attempts: Number(args.get('--attempts') || '1'),
    delayMs: Number(args.get('--delay-ms') || '3000'),
    timeoutMs: Number(args.get('--timeout-ms') || '15000'),
  });
}
