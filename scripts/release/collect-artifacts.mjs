#!/usr/bin/env node
import { cpSync, existsSync, mkdirSync, readdirSync, rmSync, statSync } from 'node:fs';
import { basename, dirname, extname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../..');
const args=new Map(); for(let i=2;i<process.argv.length;i+=2) args.set(process.argv[i],process.argv[i+1]);
const target=args.get('--target'); if(!target) throw new Error('usage: collect-artifacts.mjs --target <target>');
const bundle=resolve(root,'target',target,'release','bundle'); const out=resolve(root,'dist','release',target);
if(!existsSync(bundle)) throw new Error(`missing bundle root ${bundle}`);
rmSync(out,{recursive:true,force:true}); mkdirSync(out,{recursive:true});
const allowed=(name)=>/\.(dmg|msi|exe|deb|rpm|appimage|sig|gz)$/i.test(name);
function walk(dir){return readdirSync(dir,{withFileTypes:true}).flatMap((e)=>{const p=join(dir,e.name);return e.isDirectory()?walk(p):[p];});}
const files=walk(bundle).filter((p)=>allowed(basename(p)));
if(!files.length) throw new Error(`no distributable artifacts found below ${bundle}`);
const seen=new Set();
let totalBytes=0;
const maxBytesRaw=process.env.FILEFLOW_DISTRIBUTION_MAX_BYTES?.trim();
const maxBytes=maxBytesRaw?Number(maxBytesRaw):0;
if(maxBytesRaw && (!Number.isFinite(maxBytes) || maxBytes<=0)) throw new Error('FILEFLOW_DISTRIBUTION_MAX_BYTES must be a positive integer');
for(const source of files){
  const name=basename(source);
  if(seen.has(name)) throw new Error(`duplicate artifact basename for ${target}: ${name}`);
  seen.add(name);
  const bytes=statSync(source).size;
  totalBytes+=bytes;
  if(maxBytes && bytes>maxBytes) throw new Error(`artifact exceeds FILEFLOW_DISTRIBUTION_MAX_BYTES: ${name} (${bytes} > ${maxBytes})`);
  cpSync(source,join(out,name));
  console.log(`[collect] ${name}: ${(bytes/1024/1024).toFixed(1)} MiB`);
}
console.log(`[collect] ${files.length} artifact(s), total ${(totalBytes/1024/1024).toFixed(1)} MiB -> ${out}`);
