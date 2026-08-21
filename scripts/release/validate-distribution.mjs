#!/usr/bin/env node
import { existsSync, readdirSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../..'); const argv=new Map(); for(let i=2;i<process.argv.length;i+=2) argv.set(process.argv[i],process.argv[i+1]); const target=argv.get('--target'); const strict=process.argv.includes('--strict');
if(!target)throw new Error('usage: validate-distribution.mjs --target <target> [--strict]'); const bundle=join(root,'target',target,'release','bundle'); if(!existsSync(bundle))throw new Error('bundle root missing');
function walk(dir){return readdirSync(dir,{withFileTypes:true}).flatMap((e)=>{const p=join(dir,e.name);return e.isDirectory()?[p,...walk(p)]:[p];});}
function run(cmd,args){const r=spawnSync(cmd,args,{encoding:'utf8'});if(r.error||r.status!==0)throw new Error(`${cmd} ${args.join(' ')} failed: ${r.stderr||r.stdout||r.error}`);}
const files=walk(bundle);
if(process.platform==='darwin'){
 const app=files.find((p)=>p.endsWith('.app')); const dmg=files.find((p)=>p.endsWith('.dmg')); if(!app||!dmg)throw new Error('macOS APP/DMG missing'); run('codesign',['--verify','--deep','--strict','--verbose=2',app]);
 if(strict){run('xcrun',['stapler','validate',app]);run('xcrun',['stapler','validate',dmg]);run('spctl',['--assess','--type','execute','--verbose=2',app]);}
}else if(process.platform==='win32'){
 const installers=files.filter((p)=>/\.(exe|msi)$/i.test(p)); if(!installers.length)throw new Error('Windows installers missing'); if(strict)for(const path of installers){const escaped=path.replaceAll("'","''");run('powershell',['-NoProfile','-Command',`$s=Get-AuthenticodeSignature -LiteralPath '${escaped}'; if ($s.Status -ne 'Valid') { Write-Error $s.Status; exit 2 }`]);}
}else{
 const appImage=files.find((p)=>p.toLowerCase().endsWith('.appimage')); if(!appImage)throw new Error('Linux AppImage missing'); run('file',[appImage]);
}
console.log(`[distribution] validated ${target}${strict?' (strict signatures/notarization)':''}`);
