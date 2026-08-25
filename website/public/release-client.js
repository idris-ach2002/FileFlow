export async function fetchReleaseManifest(fetcher = globalThis.fetch, timeoutMs = 4500) {
  const controller = new AbortController();
  const timeout = globalThis.setTimeout(() => controller.abort(), timeoutMs);
  let response;

  try {
    response = await fetcher('/api/downloads', {
      headers: { Accept: 'application/json' },
      signal: controller.signal,
    });
  } catch (error) {
    if (error?.name === 'AbortError') {
      throw new Error('Le service de téléchargement met trop de temps à répondre.');
    }
    throw new Error('Le service de téléchargement est momentanément inaccessible.');
  } finally {
    globalThis.clearTimeout(timeout);
  }

  const contentType = response.headers.get('content-type') || '';
  const body = await response.text();
  let payload = null;

  if (contentType.includes('application/json') || looksLikeJson(body)) {
    try {
      payload = JSON.parse(body);
    } catch {
      throw new Error('Le service de téléchargement a renvoyé un manifeste JSON invalide.');
    }
  }

  if (!response.ok) {
    throw new Error(
      payload?.error || `Service de téléchargement indisponible (HTTP ${response.status}).`,
    );
  }
  if (!payload) {
    throw new Error(
      'Le serveur a renvoyé une page HTML à la place du manifeste de téléchargement.',
    );
  }
  if (!payload.platforms || typeof payload.platforms !== 'object') {
    throw new Error('Le manifeste de téléchargement ne contient aucune plateforme valide.');
  }

  return payload;
}

function looksLikeJson(value) {
  const first = value.trimStart()[0];
  return first === '{' || first === '[';
}
