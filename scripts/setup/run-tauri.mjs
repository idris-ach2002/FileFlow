#!/usr/bin/env node

import { existsSync, readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn } from 'node:child_process';
import { findLocalApplication } from './local-source.mjs';
import { macosBuildPipeline, prepareMacosBundledCli } from './macos-bundled-cli.mjs';
import { createMacosSetupDmg } from './create-macos-dmg.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const project = resolve(root, 'setup-tauri');
const cli = resolve(root, 'node_modules/@tauri-apps/cli/tauri.js');
const forwarded = process.argv.slice(2);
let requestedSource = process.env.FILEFLOW_SETUP_SOURCE || 'auto';
const sourceIndex = forwarded.indexOf('--source');
if (sourceIndex >= 0) {
  requestedSource = forwarded[sourceIndex + 1] || '';
  forwarded.splice(sourceIndex, 2);
}
if (!['auto', 'local', 'release'].includes(requestedSource)) {
  console.error('[setup-tauri] --source doit valoir auto, local ou release.');
  process.exit(2);
}

if (!existsSync(cli)) {
  console.error('[setup-tauri] CLI absent. Exécutez pnpm install --frozen-lockfile.');
  process.exit(2);
}

const environment = { ...process.env };
const setupTargetDirectory = resolve(
  root,
  environment.FILEFLOW_SETUP_TARGET_DIR || 'target/fileflow-setup',
);
environment.FILEFLOW_SETUP_TARGET_DIR = setupTargetDirectory;
environment.CARGO_TARGET_DIR = setupTargetDirectory;

if (forwarded[0] === 'dev'
  && requestedSource !== 'release'
  && !environment.FILEFLOW_SETUP_LOCAL_APPLICATION) {
  const local = findLocalApplication({
    root,
    version: JSON.parse(readFileSync(resolve(root, 'package.json'), 'utf8')).version,
  });
  if (local) {
    environment.FILEFLOW_SETUP_LOCAL_APPLICATION = local;
    environment.FILEFLOW_SETUP_LOCAL_VERSION = JSON.parse(
      readFileSync(resolve(root, 'package.json'), 'utf8'),
    ).version;
    console.log(`[setup-tauri] source locale automatique : ${local}`);
  } else if (requestedSource === 'local') {
    console.error('[setup-tauri] aucun paquet local de la version courante. Exécutez d’abord le préflight/build de l’application.');
    process.exit(2);
  } else {
    console.log('[setup-tauri] aucun paquet local courant ; utilisation de la release publique.');
  }
}

function runTauri(args) {
  return new Promise((resolveRun, rejectRun) => {
    const child = spawn(process.execPath, [cli, ...args], {
      cwd: project,
      env: environment,
      stdio: 'inherit',
    });

    const forwardSignal = (signal) => child.kill(signal);
    for (const signal of ['SIGINT', 'SIGTERM']) process.once(signal, forwardSignal);

    child.once('error', rejectRun);
    child.once('exit', (code, signal) => {
      for (const handledSignal of ['SIGINT', 'SIGTERM']) {
        process.removeListener(handledSignal, forwardSignal);
      }
      if (signal) {
        process.kill(process.pid, signal);
        return;
      }
      if (code !== 0) {
        rejectRun(new Error(`Tauri ${args[0] || 'command'} a échoué avec le code ${code ?? 'inconnu'}`));
        return;
      }
      resolveRun();
    });
  });
}

try {
  const macosPipeline = macosBuildPipeline(forwarded);
  if (macosPipeline) {
    console.log(`[setup-tauri] pipeline macOS ${macosPipeline.target}: build sans bundle → signature CLI → bundle.`);
    await runTauri(macosPipeline.buildArgs);
    prepareMacosBundledCli({
      root,
      project,
      targetDirectory: setupTargetDirectory,
      forwarded,
      environment,
    });
    await runTauri(macosPipeline.bundleArgs);
    if (macosPipeline.wantsDmg) {
      createMacosSetupDmg({
        target: macosPipeline.target,
        targetDirectory: setupTargetDirectory,
        project,
      });
    }
  } else {
    await runTauri(forwarded);
  }
} catch (error) {
  console.error(`[setup-tauri] échec: ${error.message}`);
  process.exit(1);
}
