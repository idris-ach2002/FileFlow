#!/usr/bin/env node
import { existsSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const repoRoot=resolve(dirname(fileURLToPath(import.meta.url)),'../..');
const argv=new Map(); for(let i=2;i<process.argv.length;i+=2) argv.set(process.argv[i],process.argv[i+1]);
const root=resolve(repoRoot,argv.get('--root')||'dist/release'); const version=argv.get('--version'); const repository=argv.get('--repository')||process.env.GITHUB_REPOSITORY; const output=resolve(repoRoot,argv.get('--output')||'latest.json');
if(!version||!repository) throw new Error('usage: generate-updater-manifest.mjs --version X.Y.Z --repository owner/repo');
const mappings={
 'aarch64-apple-darwin':'darwin-aarch64','x86_64-apple-darwin':'darwin-x86_64','x86_64-pc-windows-msvc':'windows-x86_64','x86_64-unknown-linux-gnu':'linux-x86_64','aarch64-unknown-linux-gnu':'linux-aarch64'
};
function files(dir){return existsSync(dir)?readdirSync(dir).map((n)=>join(dir,n)):[];}
const platforms={};
for(const [target,key] of Object.entries(mappings)){
 const dir=join(root,target); const list=files(dir).filter((path)=>!/fileflow[ _.-]?setup/i.test(basename(path))); let artifact;
 if(key.startsWith('darwin-')) artifact=list.find((p)=>p.endsWith('.app.tar.gz'));
 else if(key.startsWith('windows-')) artifact=list.find((p)=>/-setup\.exe$/i.test(p))||list.find((p)=>p.toLowerCase().endsWith('.msi'));
 else artifact=list.find((p)=>p.toLowerCase().endsWith('.appimage'));
 if(!artifact) throw new Error(`missing updater artifact for ${target}`); const sig=`${artifact}.sig`; if(!existsSync(sig)) throw new Error(`missing updater signature ${basename(sig)}`);
 const signature=readFileSync(sig,'utf8').trim(); if(!signature) throw new Error(`empty updater signature ${sig}`);
 const encoded=encodeURIComponent(basename(artifact)).replaceAll('%2F','/');
 platforms[key]={signature,url:`https://github.com/${repository}/releases/download/v${version}/${encoded}`};
}
const manifest={version,notes:`FileFlow ${version}`,pub_date:new Date().toISOString(),platforms};
writeFileSync(output,JSON.stringify(manifest,null,2)+'\n'); console.log(`[updater] ${Object.keys(platforms).length} platforms -> ${output}`);
