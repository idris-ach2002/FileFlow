const PLATFORM_OS = {
  'darwin-aarch64': 'macOS',
  'darwin-x86_64': 'macOS',
  'windows-x86_64': 'Windows',
  'linux-x86_64': 'Linux',
  'linux-aarch64': 'Linux',
};

export async function detectPlatform(client = globalThis.navigator, hintTimeoutMs = 400) {
  const profile = await detectDeviceProfile(client, {
    hintTimeoutMs,
    detectGraphics: false,
  });
  return profile.platform;
}

export async function detectDeviceProfile(client = globalThis.navigator, options = {}) {
  const hintTimeoutMs = options.hintTimeoutMs ?? 400;
  const graphicsTimeoutMs = options.graphicsTimeoutMs ?? 350;
  const detectGraphics = options.detectGraphics ?? true;
  const runtime = options.runtime ?? globalThis;
  const operatingSystem = detectOperatingSystem(client);
  const source = `${client?.userAgent || ''} ${client?.platform || ''}`.toLowerCase();

  let architecture = architectureFrom(source);
  let architectureSource = architecture ? 'user-agent' : null;
  let processor = null;
  let graphicsRenderer = options.graphicsRenderer ?? null;
  let conflictingEvidence = false;

  if (!architecture && client?.userAgentData?.getHighEntropyValues) {
    try {
      const hints = await withTimeout(
        client.userAgentData.getHighEntropyValues(['architecture', 'bitness']),
        hintTimeoutMs,
      );
      const hintedArchitecture = architectureFrom(`${hints?.architecture || ''} ${hints?.bitness || ''}`);
      if (hintedArchitecture) {
        architecture = hintedArchitecture;
        architectureSource = 'client-hints';
      }
    } catch {
      // Client hints are optional. An unknown architecture must never be guessed.
    }
  }

  if (operatingSystem === 'macOS' && detectGraphics) {
    if (!graphicsRenderer) {
      try {
        graphicsRenderer = await detectGraphicsRenderer(runtime, graphicsTimeoutMs);
      } catch {
        graphicsRenderer = null;
      }
    }
    const graphics = classifyGraphicsRenderer(graphicsRenderer);
    processor = graphics.processor;
    if (graphics.architecture) {
      if (architecture && architecture !== graphics.architecture) {
        conflictingEvidence = true;
        architecture = null;
        architectureSource = null;
      } else if (!architecture) {
        architecture = graphics.architecture;
        architectureSource = 'graphics';
      }
    }
  }

  const platform = conflictingEvidence ? null : platformFrom(operatingSystem, architecture);
  return {
    operatingSystem,
    architecture,
    platform,
    processor,
    graphicsRenderer: graphicsRenderer || null,
    confidence: platform ? 'high' : operatingSystem ? 'partial' : 'unknown',
    detectionSource: architectureSource,
    conflictingEvidence,
  };
}

export function detectOperatingSystem(client = globalThis.navigator) {
  const source = `${client?.userAgent || ''} ${client?.platform || ''}`.toLowerCase();
  if (source.includes('mac')) return 'macOS';
  if (source.includes('win')) return 'Windows';
  if (source.includes('linux')) return 'Linux';
  return null;
}

export function platformAccessState(platform, profile, showAll = false) {
  const detectedPlatform = profile?.platform || null;
  if (showAll) return platform === detectedPlatform ? 'recommended' : 'available';
  const operatingSystem = profile?.operatingSystem || null;
  if (!operatingSystem) return 'available';
  if (PLATFORM_OS[platform] !== operatingSystem) return 'locked';
  if (detectedPlatform) return platform === detectedPlatform ? 'recommended' : 'locked';
  if (profile?.architecture) return 'locked';
  return 'compatible';
}

export function classifyGraphicsRenderer(renderer) {
  const value = String(renderer || '').trim();
  if (!value) return { architecture: null, processor: null };

  // Browsers may deliberately reduce fingerprinting by reporting a generic Apple M1
  // renderer even on newer Apple Silicon Macs. Treat any Apple M-series renderer
  // as architecture evidence only; never surface the reported generation as CPU truth.
  if (/\bApple\s+M\d+(?:\s+(?:Pro|Max|Ultra))?\b/i.test(value)) {
    return { architecture: 'arm64', processor: 'Apple Silicon' };
  }

  if (/\bApple\s+GPU\b/i.test(value) || /\bAGX\b/i.test(value)) {
    return { architecture: 'arm64', processor: 'Apple Silicon' };
  }
  if (/\bIntel(?:\(R\))?\b/i.test(value)) {
    return { architecture: 'x64', processor: null };
  }
  return { architecture: null, processor: null };
}

export async function detectGraphicsRenderer(runtime = globalThis, timeoutMs = 350) {
  const webgl = rendererFromWebGl(runtime?.document);
  if (classifyGraphicsRenderer(webgl).architecture) return webgl;

  const gpu = runtime?.navigator?.gpu;
  if (gpu?.requestAdapter) {
    try {
      const adapter = await withTimeout(gpu.requestAdapter(), timeoutMs);
      const info = adapter?.info;
      const value = [info?.description, info?.architecture, info?.device, info?.vendor]
        .filter(Boolean)
        .join(' ')
        .trim();
      if (value) return value;
    } catch {
      // WebGPU is optional and may be disabled by browser privacy settings.
    }
  }
  return webgl || null;
}

function rendererFromWebGl(documentObject) {
  if (!documentObject?.createElement) return null;
  try {
    const canvas = documentObject.createElement('canvas');
    const gl = canvas.getContext?.('webgl2') || canvas.getContext?.('webgl');
    if (!gl) return null;
    const extension = gl.getExtension?.('WEBGL_debug_renderer_info');
    if (extension?.UNMASKED_RENDERER_WEBGL) {
      const renderer = gl.getParameter?.(extension.UNMASKED_RENDERER_WEBGL);
      if (renderer) return String(renderer);
    }
    const renderer = gl.getParameter?.(gl.RENDERER);
    return renderer ? String(renderer) : null;
  } catch {
    return null;
  }
}

function platformFrom(operatingSystem, architecture) {
  if (operatingSystem === 'macOS') {
    if (architecture === 'arm64') return 'darwin-aarch64';
    if (architecture === 'x64') return 'darwin-x86_64';
    return null;
  }
  if (operatingSystem === 'Windows') return architecture === 'arm64' ? null : 'windows-x86_64';
  if (operatingSystem === 'Linux') {
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

function architectureFrom(value) {
  if (/(?:^|[^a-z0-9])(arm64|aarch64)(?:$|[^a-z0-9])/.test(value)
    || /(?:^|[^a-z0-9])arm[^a-z0-9]+64(?:$|[^a-z0-9])/.test(value)) return 'arm64';
  if (/(?:^|[^a-z0-9])(x86_64|x86-64|amd64|win64|x64)(?:$|[^a-z0-9])/.test(value)
    || /(?:^|[^a-z0-9])x86[^a-z0-9]+64(?:$|[^a-z0-9])/.test(value)) return 'x64';
  return null;
}
