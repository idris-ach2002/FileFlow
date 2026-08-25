const FALLBACK_REPOSITORY = 'idris-ach2002/FileFlow';

export async function onRequestGet(context) {
  const repository = context.env.FILEFLOW_REPOSITORY || FALLBACK_REPOSITORY;
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository)) {
    return json({ error: 'Configuration du dépôt invalide.' }, 500);
  }
  const requestUrl = new URL(context.request.url);
  if (isLocalHostname(requestUrl.hostname)
    && context.env.FILEFLOW_DISABLE_LOCAL_PREVIEW !== 'true') {
    return json(localPreviewManifest(repository), 200, {
      'Cache-Control': 'no-store',
      'X-FileFlow-Manifest-Mode': 'local-preview',
    });
  }
  const cache = caches.default;
  const key = new Request(new URL('/api/downloads', context.request.url), context.request);
  const cached = await cache.match(key);
  if (cached) return cached;

  let upstream;
  try {
    upstream = await fetchPublishedManifest(repository);
  } catch {
    return unavailable('Le service de publication FileFlow est temporairement inaccessible.');
  }
  if (!upstream.ok) {
    return unavailable('Aucune distribution stable complète n’est disponible.', upstream.status);
  }
  let manifest;
  try {
    manifest = await upstream.json();
  } catch {
    return json({ error: 'Le manifeste de téléchargement public n’est pas un JSON valide.' }, 502);
  }
  if (!validManifest(manifest, repository)) {
    return json({ error: 'Le manifeste de téléchargement public est invalide.' }, 502);
  }
  const response = json(manifest, 200, {
    'Cache-Control': 'public, max-age=60, s-maxage=300, stale-while-revalidate=3600',
  });
  context.waitUntil(cache.put(key, response.clone()));
  return response;
}

async function fetchPublishedManifest(repository) {
  const headers = { Accept: 'application/json', 'User-Agent': 'FileFlow-Download-Portal/1' };
  const options = {
    headers,
    signal: AbortSignal.timeout(10000),
    cf: { cacheTtl: 300, cacheEverything: true },
  };
  const latest = await fetch(
    `https://github.com/${repository}/releases/latest/download/downloads.json`,
    options,
  );
  if (latest.ok || latest.status !== 404) return latest;

  // Une release applicative plus récente peut ne pas encore contenir Setup.
  // Dans ce cas, retrouver la dernière release stable *complète* au lieu de
  // rendre tout le portail inutilisable.
  const releases = await fetch(`https://api.github.com/repos/${repository}/releases?per_page=20`, {
    ...options,
    cf: { cacheTtl: 120, cacheEverything: true },
  });
  if (!releases.ok) return latest;
  let payload;
  try {
    payload = await releases.json();
  } catch {
    return latest;
  }
  const asset = payload
    .filter((release) => !release.draft && !release.prerelease)
    .flatMap((release) => release.assets || [])
    .find((candidate) => candidate.name === 'downloads.json'
      && candidate.browser_download_url?.startsWith(`https://github.com/${repository}/releases/download/`));
  return asset ? fetch(asset.browser_download_url, options) : latest;
}

function isLocalHostname(hostname) {
  return hostname === 'localhost' || hostname === '127.0.0.1' || hostname === '::1';
}

function localPreviewManifest(repository) {
  const platforms = [
    'darwin-aarch64',
    'darwin-x86_64',
    'windows-x86_64',
    'linux-x86_64',
    'linux-aarch64',
  ];
  return {
    schemaVersion: 1,
    preview: true,
    version: null,
    publishedAt: null,
    repository,
    platforms: Object.fromEntries(platforms.map((platform) => [platform, {}])),
  };
}

function unavailable(message, upstreamStatus) {
  return json({ error: message, ...(upstreamStatus ? { upstreamStatus } : {}) }, 503, {
    'Cache-Control': 'no-store',
    'Retry-After': '60',
  });
}

function validManifest(manifest, repository) {
  if (manifest?.schemaVersion !== 1
    || !/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(String(manifest?.version || ''))
    || Number.isNaN(Date.parse(manifest?.publishedAt || ''))
    || manifest?.repository !== repository
    || !manifest?.platforms) return false;

  const required = ['darwin-aarch64', 'darwin-x86_64', 'windows-x86_64', 'linux-x86_64', 'linux-aarch64'];
  const releasePrefix = `https://github.com/${repository}/releases/download/v${manifest.version}/`;
  return required.every((platform) => {
    const downloads = manifest.platforms[platform];
    return validArtifact(downloads?.application, releasePrefix)
      && validArtifact(downloads?.setup, releasePrefix)
      && (!downloads?.cli || validArtifact(downloads.cli, releasePrefix));
  });
}

function validArtifact(artifact, releasePrefix) {
  if (!artifact || !String(artifact.url).startsWith(releasePrefix)
    || !/^[0-9a-f]{64}$/i.test(String(artifact.sha256 || ''))
    || !Number.isSafeInteger(artifact.size) || artifact.size <= 0
    || typeof artifact.name !== 'string' || !artifact.name || /[/\\]/.test(artifact.name)) return false;
  try {
    const urlName = decodeURIComponent(new URL(artifact.url).pathname.split('/').pop());
    return urlName === artifact.name;
  } catch {
    return false;
  }
}

function json(value, status, headers = {}) {
  return new Response(JSON.stringify(value), {
    status,
    headers: {
      'Content-Type': 'application/json; charset=utf-8',
      'X-Content-Type-Options': 'nosniff',
      'Referrer-Policy': 'no-referrer',
      ...headers,
    },
  });
}
