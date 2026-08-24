#!/usr/bin/env node
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const configPath = join(root, 'src-tauri', 'tauri.conf.json');
const keyPath = resolve(
  process.env.FILEFLOW_UPDATER_KEY || join(homedir(), '.tauri', 'fileflow.key'),
);
const publicKeyPath = `${keyPath}.pub`;
const endpoint = process.env.FILEFLOW_UPDATE_ENDPOINT
  || 'https://github.com/idris-ach2002/FileFlow/releases/latest/download/latest.json';
const repository = process.env.FILEFLOW_GITHUB_REPOSITORY || repositoryFromGitRemote();
const password = process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD;

if (!password) {
  fail([
    'La phrase secrète de signature doit être fournie uniquement pour cette commande.',
    'Dans zsh :',
    '  read -s "TAURI_SIGNING_PRIVATE_KEY_PASSWORD?Phrase secrète Updater : "',
    '  export TAURI_SIGNING_PRIVATE_KEY_PASSWORD',
    '  pnpm run updater:setup',
    '  unset TAURI_SIGNING_PRIVATE_KEY_PASSWORD',
  ].join('\n'));
}

requireCommand('pnpm', ['--version']);
requireCommand('gh', ['auth', 'status', '--hostname', 'github.com']);

if (existsSync(keyPath) !== existsSync(publicKeyPath)) {
  fail(`Paire de clés incomplète : ${keyPath} et ${publicKeyPath} doivent exister ensemble.`);
}

if (!existsSync(keyPath) || !existsSync(publicKeyPath)) {
  mkdirSync(dirname(keyPath), { recursive: true, mode: 0o700 });
  console.log(`\n[updater] Génération de la paire de clés dans ${keyPath}`);
  run('pnpm', ['exec', 'tauri', 'signer', 'generate', '-w', keyPath], {
    stdio: 'inherit',
    env: { ...process.env, TAURI_SIGNING_PRIVATE_KEY_PASSWORD: password },
  });
}

if (!existsSync(keyPath) || !existsSync(publicKeyPath)) {
  fail(`La CLI Tauri n’a pas créé ${keyPath} et ${publicKeyPath}.`);
}

const privateKey = readFileSync(keyPath, 'utf8').trim();
const publicKey = readFileSync(publicKeyPath, 'utf8').trim();
if (!privateKey || !publicKey) fail('La paire de clés Updater est vide.');

console.log(`\n[updater] Configuration sécurisée de ${repository}`);
setSecret('TAURI_SIGNING_PRIVATE_KEY', privateKey);
setSecret('TAURI_SIGNING_PRIVATE_KEY_PASSWORD', password);
setSecret('TAURI_UPDATER_PUBKEY', publicKey);
run('gh', ['variable', 'set', 'FILEFLOW_UPDATE_ENDPOINT', '--repo', repository, '--body', endpoint]);

const config = JSON.parse(readFileSync(configPath, 'utf8'));
config.bundle ??= {};
config.bundle.createUpdaterArtifacts = true;
config.plugins ??= {};
config.plugins.updater = {
  pubkey: publicKey,
  endpoints: [endpoint],
  windows: { installMode: 'passive' },
};
writeFileSync(configPath, `${JSON.stringify(config, null, 2)}\n`);

run('gh', ['secret', 'list', '--repo', repository], { stdio: 'inherit' });
run('gh', ['variable', 'list', '--repo', repository], { stdio: 'inherit' });
console.log([
  '',
  '[updater] Configuration terminée.',
  `- Clé privée : ${keyPath} (hors du dépôt, à sauvegarder en lieu sûr)`,
  `- Clé publique : intégrée dans src-tauri/tauri.conf.json`,
  `- Manifeste : ${endpoint}`,
  '- GitHub : secrets Updater et variable endpoint configurés',
  '',
  'Vous pouvez maintenant reconstruire FileFlow. La première vérification utile',
  'nécessite une release GitHub complète et signée plus récente que la version installée.',
].join('\n'));

function repositoryFromGitRemote() {
  const result = spawnSync('git', ['remote', 'get-url', 'origin'], {
    cwd: root,
    encoding: 'utf8',
  });
  if (result.status !== 0) fail('Remote Git origin introuvable. Définissez FILEFLOW_GITHUB_REPOSITORY.');
  const remote = result.stdout.trim();
  const match = remote.match(/github\.com[/:]([^/]+\/[^/]+?)(?:\.git)?$/i);
  if (!match) fail(`Remote GitHub non reconnu : ${remote}`);
  return match[1];
}

function setSecret(name, value) {
  run('gh', ['secret', 'set', name, '--repo', repository], {
    input: `${value}\n`,
    stdio: ['pipe', 'inherit', 'inherit'],
  });
}

function requireCommand(command, args) {
  const result = spawnSync(command, args, { cwd: root, stdio: 'ignore' });
  if (result.status !== 0) fail(`Commande indisponible ou non authentifiée : ${command} ${args.join(' ')}`);
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: root,
    stdio: options.stdio || 'inherit',
    input: options.input,
    env: options.env || process.env,
  });
  if (result.error) fail(result.error.message);
  if (result.status !== 0) fail(`Échec : ${command} ${args.join(' ')}`);
}

function fail(message) {
  console.error(`\n[updater] ${message}`);
  process.exit(1);
}
