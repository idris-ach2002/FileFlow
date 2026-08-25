import { cpSync, mkdirSync, rmSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const output = resolve(root, 'dist');
rmSync(output, { recursive: true, force: true });
mkdirSync(output, { recursive: true });
cpSync(resolve(root, 'public'), output, { recursive: true });
console.log(`[site] Cloudflare Pages bundle -> ${output}`);
