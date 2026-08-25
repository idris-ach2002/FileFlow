#!/usr/bin/env node
import { copyFileSync, existsSync, mkdirSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { setupReleaseRoot } from './artifact-layout.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const args = new Map();
for (let index = 2; index < process.argv.length; index += 2) args.set(process.argv[index], process.argv[index + 1]);
const target = args.get('--target');
if (!target) throw new Error('usage: package-setup-cli.mjs --target <rust-target>');

const windows = target.includes('windows');
const releaseRoot = setupReleaseRoot(root, target);
const source = join(releaseRoot, `fileflow-setup-cli${windows ? '.exe' : ''}`);
if (!existsSync(source)) throw new Error(`missing Setup CLI binary: ${source}`);
const outputDirectory = join(releaseRoot, 'bundle', 'setup-cli');
mkdirSync(outputDirectory, { recursive: true });
const output = join(outputDirectory, `FileFlowSetupCLI_${target}${windows ? '.exe' : '.bin'}`);
copyFileSync(source, output);
console.log(`[setup-cli] ${(statSync(output).size / 1024 / 1024).toFixed(1)} MiB -> ${output}`);
