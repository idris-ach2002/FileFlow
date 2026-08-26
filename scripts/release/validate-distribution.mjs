#!/usr/bin/env node
import { existsSync, readdirSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { applicationBundleRoot, setupBundleRoot } from './artifact-layout.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const args = new Map();
for (let index = 2; index < process.argv.length; index += 2) {
  args.set(process.argv[index], process.argv[index + 1]);
}
const target = args.get('--target');
const strict = process.argv.includes('--strict');
const requireSetup = process.argv.includes('--require-setup');
if (!target) {
  throw new Error('usage: validate-distribution.mjs --target <target> [--strict] [--require-setup]');
}

function walk(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    return entry.isDirectory() ? [path, ...walk(path)] : [path];
  });
}
function filesBelow(directory, label) {
  if (!existsSync(directory)) throw new Error(`${label} bundle root missing: ${directory}`);
  return walk(directory);
}
function execute(command, commandArgs) {
  return spawnSync(command, commandArgs, { encoding: 'utf8' });
}
function run(command, commandArgs) {
  const result = execute(command, commandArgs);
  if (result.error || result.status !== 0) {
    throw new Error(`${command} ${commandArgs.join(' ')} failed: ${result.stderr || result.stdout || result.error}`);
  }
  return result.stdout || '';
}
function commandExists(command) {
  const result = execute(command, ['--version']);
  return !result.error && result.status === 0;
}
function one(files, predicate, label) {
  const matches = files.filter(predicate);
  if (matches.length !== 1) {
    throw new Error(`${label}: expected exactly one artifact, found ${matches.map((path) => basename(path)).join(', ') || 'none'}`);
  }
  return matches[0];
}
function isSetup(path) {
  return /fileflow[ _.-]?setup/i.test(basename(path));
}
function isSetupCli(path) {
  return /fileflow[ _.-]?setup[ _.-]?cli/i.test(basename(path));
}
function requireLinuxDesktopMetadata(packagePath, packageType, label) {
  let listing = '';
  if (packageType === 'deb') {
    listing = run('dpkg-deb', ['--contents', packagePath]);
  } else if (packageType === 'rpm' && commandExists('rpm')) {
    listing = run('rpm', ['-qpl', packagePath]);
  } else {
    return;
  }
  if (!/usr\/share\/applications\/[^\n]*\.desktop/i.test(listing)) {
    throw new Error(`${label} ${packageType.toUpperCase()} does not contain a freedesktop launcher`);
  }
  if (!/usr\/share\/icons\//i.test(listing)) {
    throw new Error(`${label} ${packageType.toUpperCase()} does not contain application icons`);
  }
}

const applicationFiles = filesBelow(applicationBundleRoot(root, target), 'application');
const setupFiles = requireSetup ? filesBelow(setupBundleRoot(root, target), 'Setup') : [];

if (process.platform === 'darwin') {
  const app = one(applicationFiles, (path) => path.endsWith('FileFlow.app'), 'macOS application APP');
  const dmg = one(applicationFiles, (path) => path.endsWith('.dmg') && !isSetup(path), 'macOS application DMG');
  run('codesign', ['--verify', '--deep', '--strict', '--verbose=2', app]);
  run('hdiutil', ['verify', dmg]);
  if (strict) {
    run('xcrun', ['stapler', 'validate', app]);
    run('xcrun', ['stapler', 'validate', dmg]);
    run('spctl', ['--assess', '--type', 'execute', '--verbose=2', app]);
  }
  if (requireSetup) {
    const setupApp = one(setupFiles, (path) => path.endsWith('FileFlowSetup.app'), 'macOS Setup APP');
    const setupDmg = one(setupFiles, (path) => path.endsWith('.dmg') && isSetup(path), 'macOS Setup DMG');
    run('codesign', ['--verify', '--deep', '--strict', '--verbose=2', setupApp]);
    run('hdiutil', ['verify', setupDmg]);
    if (strict) {
      run('xcrun', ['stapler', 'validate', setupApp]);
      run('xcrun', ['stapler', 'validate', setupDmg]);
      run('spctl', ['--assess', '--type', 'execute', '--verbose=2', setupApp]);
    }
  }
} else if (process.platform === 'win32') {
  const appExe = one(applicationFiles, (path) => /\.exe$/i.test(path) && !isSetup(path), 'Windows application NSIS EXE');
  one(applicationFiles, (path) => /\.msi$/i.test(path) && !isSetup(path), 'Windows application MSI');
  if (strict) {
    const escaped = appExe.replaceAll("'", "''");
    run('powershell', ['-NoProfile', '-Command', `$s=Get-AuthenticodeSignature -LiteralPath '${escaped}'; if ($s.Status -ne 'Valid') { Write-Error $s.Status; exit 2 }`]);
  }
  if (requireSetup) {
    const setupExe = one(setupFiles, (path) => /\.exe$/i.test(path) && isSetup(path) && !isSetupCli(path), 'Windows Setup NSIS EXE');
    one(setupFiles, (path) => /\.msi$/i.test(path) && isSetup(path), 'Windows Setup MSI');
    one(setupFiles, (path) => /\.exe$/i.test(path) && isSetupCli(path), 'Windows Setup CLI');
    if (strict) {
      const escaped = setupExe.replaceAll("'", "''");
      run('powershell', ['-NoProfile', '-Command', `$s=Get-AuthenticodeSignature -LiteralPath '${escaped}'; if ($s.Status -ne 'Valid') { Write-Error $s.Status; exit 2 }`]);
    }
  }
} else {
  const appImage = one(applicationFiles, (path) => path.toLowerCase().endsWith('.appimage') && !isSetup(path), 'Linux application AppImage');
  const appDeb = one(applicationFiles, (path) => path.toLowerCase().endsWith('.deb') && !isSetup(path), 'Linux application DEB');
  const appRpm = one(applicationFiles, (path) => path.toLowerCase().endsWith('.rpm') && !isSetup(path), 'Linux application RPM');
  run('file', [appImage]);
  requireLinuxDesktopMetadata(appDeb, 'deb', 'FileFlow');
  requireLinuxDesktopMetadata(appRpm, 'rpm', 'FileFlow');
  if (requireSetup) {
    const setupAppImage = one(setupFiles, (path) => path.toLowerCase().endsWith('.appimage') && isSetup(path), 'Linux Setup AppImage');
    const setupDeb = one(setupFiles, (path) => path.toLowerCase().endsWith('.deb') && isSetup(path), 'Linux Setup DEB');
    const setupRpm = one(setupFiles, (path) => path.toLowerCase().endsWith('.rpm') && isSetup(path), 'Linux Setup RPM');
    run('file', [setupAppImage]);
    requireLinuxDesktopMetadata(setupDeb, 'deb', 'FileFlow Setup');
    requireLinuxDesktopMetadata(setupRpm, 'rpm', 'FileFlow Setup');
  }
}

console.log(`[distribution] validated ${target}${requireSetup ? ' + Setup' : ''}${strict ? ' (strict signatures/notarization)' : ''}`);
