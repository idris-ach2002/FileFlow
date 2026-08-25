export async function detectPlatform(client = globalThis.navigator, hintTimeoutMs = 400) {
  const userAgent = String(client?.userAgent || '').toLowerCase();
  const platform = String(client?.platform || '').toLowerCase();
  const source = `${userAgent} ${platform}`;

  let architecture = architectureFrom(source);
  if (!architecture && client?.userAgentData?.getHighEntropyValues) {
    try {
      const hints = await withTimeout(
        client.userAgentData.getHighEntropyValues(['architecture', 'bitness']),
        hintTimeoutMs,
      );
      architecture = architectureFrom(`${hints?.architecture || ''} ${hints?.bitness || ''}`);
    } catch {
      // Client hints are optional. An unknown architecture must never be guessed.
    }
  }

  if (source.includes('mac')) {
    if (architecture === 'arm64') return 'darwin-aarch64';
    if (architecture === 'x64') return 'darwin-x86_64';
    return null;
  }
  if (source.includes('win')) return architecture === 'arm64' ? null : 'windows-x86_64';
  if (source.includes('linux')) {
    if (architecture === 'arm64') return 'linux-aarch64';
    if (architecture === 'x64') return 'linux-x86_64';
  }
  return null;
}

async function withTimeout(promise, timeoutMs) {
  let timeout;
  try {
    return await Promise.race([
      promise,
      new Promise((resolve) => {
        timeout = globalThis.setTimeout(() => resolve(null), timeoutMs);
      }),
    ]);
  } finally {
    globalThis.clearTimeout(timeout);
  }
}

export function detectOperatingSystem(client = globalThis.navigator) {
  const source = `${client?.userAgent || ''} ${client?.platform || ''}`.toLowerCase();
  if (source.includes('mac')) return 'macOS';
  if (source.includes('win')) return 'Windows';
  if (source.includes('linux')) return 'Linux';
  return null;
}

function architectureFrom(value) {
  if (/(?:^|[^a-z0-9])(arm64|aarch64)(?:$|[^a-z0-9])/.test(value)
    || /(?:^|[^a-z0-9])arm[^a-z0-9]+64(?:$|[^a-z0-9])/.test(value)) return 'arm64';
  if (/(?:^|[^a-z0-9])(x86_64|x86-64|amd64|win64|x64)(?:$|[^a-z0-9])/.test(value)) return 'x64';
  return null;
}
