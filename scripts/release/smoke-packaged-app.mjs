#!/usr/bin/env node
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync } from 'node:fs';
import { spawn, spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const args = new Map();
for (let i=2;i<process.argv.length;i+=2) args.set(process.argv[i], process.argv[i+1]);
const target=args.get('--target') || process.env.FILEFLOW_TARGET;
const timeoutMs=Number(args.get('--timeout-ms') || '60000');
if (!target) throw new Error('usage: smoke-packaged-app.mjs --target <target>');
const releaseRoot=resolve(root,'target',target,'release');
const bundleRoot=resolve(releaseRoot,'bundle');
if (!existsSync(bundleRoot)) throw new Error(`bundle root not found: ${bundleRoot}`);

function walk(dir) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir,{withFileTypes:true}).flatMap((entry)=>{
    const path=join(dir,entry.name); return entry.isDirectory() ? [path,...walk(path)] : [path];
  });
}
function first(predicate) { return walk(bundleRoot).find(predicate); }
function sleep(ms){ return new Promise((resolve)=>setTimeout(resolve,ms)); }
async function terminate(child) {
  if (!child) return;

  const pid = child.pid;

  const closePipes = () => {
    child.stdout?.destroy();
    child.stderr?.destroy();
    child.stdin?.destroy();
  };

  // Windows already has a reliable process-tree primitive.
  if (process.platform === 'win32') {
    if (pid && child.exitCode === null) {
      spawnSync(
        'taskkill',
        ['/PID', String(pid), '/T', '/F'],
        {
          stdio: 'ignore',
          timeout: 10000,
        },
      );
    }

    closePipes();
    return;
  }

  if (!pid) {
    closePipes();
    return;
  }

  const signalTree = (signal) => {
    try {
      // Unix packaged runtimes are started as their own process group.
      // Killing -pid terminates AppImage/Tauri children as well as
      // the launcher itself.
      process.kill(-pid, signal);
      return;
    } catch (error) {
      if (error?.code !== 'ESRCH') {
        try {
          child.kill(signal);
        } catch {
          // Process may already be gone.
        }
      }
    }
  };

  if (child.exitCode === null) {
    signalTree('SIGTERM');
    await sleep(750);
  }

  if (child.exitCode === null) {
    signalTree('SIGKILL');
    await sleep(250);
  }

  // AppImage descendants can inherit Node's stdout/stderr descriptors.
  // Destroying our streams prevents a surviving inherited descriptor
  // from keeping the Node event loop alive after a successful smoke.
  closePipes();
}

const temp=mkdtempSync(join(tmpdir(),'fileflow-package-smoke-'));
const healthFile=join(temp,'health.json');
let executable;
let installRoot=null;

if (process.platform==='darwin') {
  const app=first((path)=>path.endsWith('.app'));
  if (!app) throw new Error('packaged .app not found');
  const macos=join(app,'Contents','MacOS');
  executable=walk(macos).find((path)=>statSync(path).isFile());
} else if (process.platform==='linux') {
  executable=first((path)=>path.toLowerCase().endsWith('.appimage'));
  if (!executable) throw new Error('packaged AppImage not found');
} else if (process.platform==='win32') {
  const installer=first((path)=>path.toLowerCase().endsWith('.exe') && path.toLowerCase().includes('nsis'))
    || first((path)=>path.toLowerCase().endsWith('-setup.exe'))
    || first((path)=>path.toLowerCase().endsWith('.exe'));
  if (!installer) throw new Error('NSIS installer not found');
  installRoot=join(temp,'installed');
  const installed=spawnSync(installer,['/S',`/D=${installRoot}`],{encoding:'utf8',timeout:120000});
  if (installed.error || installed.status!==0) throw new Error(`NSIS silent install failed: ${installed.stderr||installed.stdout||installed.error}`);
  executable=walk(installRoot).find((path)=>path.toLowerCase().endsWith('.exe') && !basename(path).toLowerCase().startsWith('uninstall'));
}
if (!executable || !existsSync(executable)) throw new Error('unable to locate packaged FileFlow executable');

console.log(`[smoke] launching packaged runtime: ${executable}`);
const child=spawn(executable,[],{
  cwd: dirname(executable),
  env:{
    ...process.env,
    FILEFLOW_SMOKE_HEALTH_FILE:healthFile,
    FILEFLOW_SMOKE_TEST:'1',
    APPIMAGE_EXTRACT_AND_RUN:'1',
  },
  stdio:['ignore','pipe','pipe'],

  // On Unix this creates a dedicated process group. The cleanup code
  // can consequently terminate the complete packaged runtime tree
  // instead of only the AppImage launcher.
  detached: process.platform !== 'win32',
});
let output='';
child.stdout?.on('data',(chunk)=>{ output+=chunk.toString(); });
child.stderr?.on('data',(chunk)=>{ output+=chunk.toString(); });
const deadline=Date.now()+timeoutMs;
let payload=null;
try {
  while (Date.now()<deadline) {
    if (child.exitCode!==null) throw new Error(`packaged app exited early (${child.exitCode})\n${output.slice(-4000)}`);
    if (existsSync(healthFile)) {
      try { payload=JSON.parse(readFileSync(healthFile,'utf8')); } catch { /* atomic handoff may still be finishing */ }
      if (payload?.backend===true && payload?.frontend===true) break;
    }
    await sleep(200);
  }
  if (!payload || payload.backend!==true || payload.frontend!==true) throw new Error(`health handshake timed out after ${timeoutMs}ms\n${output.slice(-4000)}`);
  const health=payload.health||{};
  if (health.app!=='FileFlow' || !health.version || !health.os || !health.architecture) throw new Error(`invalid health payload: ${JSON.stringify(payload)}`);
  console.log(`[smoke] OK FileFlow ${health.version} ${health.os}/${health.architecture}; Angular→Tauri IPC ready`);
} finally {
  await terminate(child);
  rmSync(temp,{recursive:true,force:true});
}
