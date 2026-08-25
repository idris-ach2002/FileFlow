#!/usr/bin/env node
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync } from 'node:fs';
import { spawn, spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { selectWindowsSetupExecutable, setupBundleRoot } from './artifact-layout.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const args = new Map();
for (let index = 2; index < process.argv.length; index += 2) {
  args.set(process.argv[index], process.argv[index + 1]);
}
const target = args.get('--target') || process.env.FILEFLOW_TARGET;
const timeoutMs = Number(args.get('--timeout-ms') || '60000');
if (!target) throw new Error('usage: smoke-packaged-setup.mjs --target <target>');

const bundleRoot = setupBundleRoot(root, target);
if (!existsSync(bundleRoot)) throw new Error(`bundle root not found: ${bundleRoot}`);

function walk(directory) {
  if (!existsSync(directory)) return [];
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    return entry.isDirectory() ? [path, ...walk(path)] : [path];
  });
}

function first(predicate) {
  return walk(bundleRoot).find(predicate);
}

function sleep(milliseconds) {
  return new Promise((resolvePromise) => setTimeout(resolvePromise, milliseconds));
}

async function terminate(child) {
  if (!child) return;
  const pid = child.pid;
  if (process.platform === 'win32') {
    if (pid && child.exitCode === null) {
      spawnSync('taskkill', ['/PID', String(pid), '/T', '/F'], { stdio: 'ignore', timeout: 10000 });
    }
  } else if (pid && child.exitCode === null) {
    try {
      process.kill(-pid, 'SIGTERM');
      await sleep(750);
      if (child.exitCode === null) process.kill(-pid, 'SIGKILL');
    } catch (error) {
      if (error?.code !== 'ESRCH') child.kill('SIGKILL');
    }
  }
  child.stdout?.destroy();
  child.stderr?.destroy();
  child.stdin?.destroy();
}

const temporary = mkdtempSync(join(tmpdir(), 'fileflow-setup-smoke-'));
const healthFile = join(temporary, 'setup-health.json');
let executable;

if (process.platform === 'darwin') {
  const application = first((path) => path.endsWith('FileFlowSetup.app'));
  if (!application) throw new Error('packaged FileFlowSetup.app not found');
  const macos = join(application, 'Contents', 'MacOS');
  executable = walk(macos).find((path) => statSync(path).isFile());
} else if (process.platform === 'linux') {
  executable = first((path) => path.toLowerCase().endsWith('.appimage')
    && /fileflow[ _.-]?setup/i.test(basename(path)));
} else if (process.platform === 'win32') {
  const installer = first((path) => path.toLowerCase().endsWith('.exe')
    && /fileflow[ _.-]?setup/i.test(basename(path))
    && !/cli/i.test(basename(path)));
  if (!installer) throw new Error('packaged FileFlow Setup installer not found');
  const installRoot = join(temporary, 'installed');
  const installed = spawnSync(installer, ['/S', `/D=${installRoot}`], {
    encoding: 'utf8',
    timeout: 120000,
  });
  if (installed.error || installed.status !== 0) {
    throw new Error(`FileFlow Setup silent install failed: ${installed.stderr || installed.stdout || installed.error}`);
  }
  executable = selectWindowsSetupExecutable(walk(installRoot));
}
if (!executable || !existsSync(executable)) throw new Error('unable to locate packaged FileFlow Setup executable');

console.log(`[setup-smoke] launching packaged Setup: ${executable}`);
const child = spawn(executable, [], {
  cwd: dirname(executable),
  env: {
    ...process.env,
    FILEFLOW_SETUP_SMOKE_TEST: '1',
    FILEFLOW_SETUP_SMOKE_HEALTH_FILE: healthFile,
    APPIMAGE_EXTRACT_AND_RUN: '1',
  },
  stdio: ['ignore', 'pipe', 'pipe'],
  detached: process.platform !== 'win32',
});
let output = '';
child.stdout?.on('data', (chunk) => { output += chunk.toString(); });
child.stderr?.on('data', (chunk) => { output += chunk.toString(); });
const deadline = Date.now() + timeoutMs;
let payload = null;
try {
  while (Date.now() < deadline) {
    if (child.exitCode !== null) {
      throw new Error(`packaged Setup exited early (${child.exitCode})\n${output.slice(-4000)}`);
    }
    if (existsSync(healthFile)) {
      try {
        payload = JSON.parse(readFileSync(healthFile, 'utf8'));
      } catch {
        // The atomic handoff may still be finishing.
      }
      if (payload?.backend === true && payload?.frontend === true) break;
    }
    await sleep(200);
  }
  if (!payload || payload.backend !== true || payload.frontend !== true) {
    throw new Error(`Setup health handshake timed out after ${timeoutMs}ms\n${output.slice(-4000)}`);
  }
  if (payload.app !== 'FileFlow Setup' || !payload.version || !payload.platform || !payload.architecture) {
    throw new Error(`invalid Setup health payload: ${JSON.stringify(payload)}`);
  }
  console.log(`[setup-smoke] OK FileFlow Setup ${payload.version} ${payload.platform}/${payload.architecture}; UI→Tauri IPC ready`);
} finally {
  await terminate(child);
  rmSync(temporary, { recursive: true, force: true });
}
