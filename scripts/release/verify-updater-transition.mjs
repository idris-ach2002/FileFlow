#!/usr/bin/env node
import { readFileSync } from 'node:fs';
const argv=new Map(); for(let i=2;i<process.argv.length;i+=2) argv.set(process.argv[i],process.argv[i+1]);
const previous=argv.get('--from'); const next=argv.get('--to'); const manifestPath=argv.get('--manifest')||'latest.json';
if(!previous||!next) throw new Error('usage: verify-updater-transition.mjs --from 1.0.1 --to 1.0.2 [--manifest latest.json]');
function parse(v){const m=v.replace(/^v/,'').match(/^(\d+)\.(\d+)\.(\d+)(?:[-+].*)?$/);if(!m)throw new Error(`invalid semver ${v}`);return m.slice(1).map(Number);}
function cmp(a,b){for(let i=0;i<3;i++){if(a[i]!==b[i])return a[i]-b[i];}return 0;}
if(cmp(parse(next),parse(previous))<=0) throw new Error(`updater transition must increase version: ${previous} -> ${next}`);
const manifest=JSON.parse(readFileSync(manifestPath,'utf8')); if(manifest.version.replace(/^v/,'')!==next.replace(/^v/,'')) throw new Error('latest.json version does not match target version');
const expected=['darwin-aarch64','darwin-x86_64','windows-x86_64','linux-x86_64','linux-aarch64'];
for(const key of expected){const item=manifest.platforms?.[key];if(!item?.url||!item?.signature)throw new Error(`latest.json missing ${key}`);}
console.log(`[updater] transition ${previous} -> ${next} is structurally valid on ${expected.length} platforms`);
