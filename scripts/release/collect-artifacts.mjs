#!/usr/bin/env node
import { cpSync, existsSync, mkdirSync, readdirSync, rmSync } from 'node:fs';
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
for(const source of files){const name=basename(source);if(seen.has(name)) throw new Error(`duplicate artifact basename for ${target}: ${name}`);seen.add(name);cpSync(source,join(out,name));}
console.log(`[collect] ${files.length} artifact(s) -> ${out}`);
