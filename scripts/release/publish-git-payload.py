#!/usr/bin/env python3
from __future__ import annotations
import argparse,hashlib,json,shutil,subprocess,tempfile
from pathlib import Path
CHUNK=80*1024*1024
TARGETS={
'x86_64-unknown-linux-gnu':('linux','x64','distribution/linux-x64','.AppImage'),
'aarch64-unknown-linux-gnu':('linux','arm64','distribution/linux-arm64','.AppImage'),
'aarch64-apple-darwin':('macos','arm64','distribution/macos-arm64','.dmg'),
'x86_64-apple-darwin':('macos','x64','distribution/macos-x64','.dmg'),
'x86_64-pc-windows-msvc':('windows','x64','distribution/windows-x64','.exe')}
def run(*a,cwd=None,capture=False):
 print('+',' '.join(a));return subprocess.run(a,cwd=str(cwd) if cwd else None,check=True,text=True,stdout=subprocess.PIPE if capture else None,stderr=subprocess.STDOUT if capture else None)
def choose(root,suffix):
 m=sorted(p for p in root.rglob('*') if p.is_file() and p.name.lower().endswith(suffix.lower()))
 if len(m)!=1: raise SystemExit(f'expected exactly one {suffix} below {root}, found {m}')
 return m[0]
def main():
 ap=argparse.ArgumentParser();ap.add_argument('--target',required=True);ap.add_argument('--root',required=True);ap.add_argument('--channel',choices=['candidate','production'],default='candidate');args=ap.parse_args()
 if args.target not in TARGETS: raise SystemExit(f'unsupported target {args.target}')
 platform,arch,branch,suffix=TARGETS[args.target];root=Path(args.root).resolve();package=choose(root,suffix);repo=Path(run('git','rev-parse','--show-toplevel',capture=True).stdout.strip());source=run('git','rev-parse','HEAD',capture=True,cwd=repo).stdout.strip();version=str(json.loads((repo/'src-tauri/tauri.conf.json').read_text())['version'])
 sha=hashlib.sha256();size=0
 with package.open('rb') as f:
  while True:
   b=f.read(1024*1024)
   if not b:break
   sha.update(b);size+=len(b)
 tmp=Path(tempfile.mkdtemp(prefix='fileflow-git-payload-'));wt=tmp/'worktree'
 try:
  run('git','worktree','add','--detach',str(wt),'HEAD',cwd=repo);run('git','switch','--orphan',f'payload-{platform}-{arch}',cwd=wt)
  for child in list(wt.iterdir()):
   if child.name=='.git':continue
   shutil.rmtree(child) if child.is_dir() else child.unlink()
  payload=wt/'payload';payload.mkdir();parts=0
  with package.open('rb') as src:
   while True:
    b=src.read(CHUNK)
    if not b:break
    (payload/f'part-{parts:04d}').write_bytes(b);parts+=1
  (wt/'manifest.env').write_text(f'VERSION={version}\nSOURCE_SHA={source}\nTARGET={args.target}\nPLATFORM={platform}\nARCH={arch}\nCHANNEL={args.channel}\nPACKAGE_NAME={package.name}\nPACKAGE_SHA256={sha.hexdigest()}\nPACKAGE_SIZE={size}\nCHUNK_COUNT={parts}\n',encoding='ascii')
  (wt/'README.txt').write_text('FileFlow binary transport branch. Generated automatically.\n')
  run('git','add','-A',cwd=wt);run('git','-c','user.name=github-actions[bot]','-c','user.email=41898282+github-actions[bot]@users.noreply.github.com','commit','-m',f'dist({platform}): FileFlow {version} {arch} {source[:12]}',cwd=wt);run('git','push','--force','origin',f'HEAD:refs/heads/{branch}',cwd=wt)
  print(f'[OK] {branch} chunks={parts} sha256={sha.hexdigest()}')
 finally:
  try: run('git','worktree','remove','--force',str(wt),cwd=repo)
  except Exception: pass
  shutil.rmtree(tmp,ignore_errors=True)
if __name__=='__main__':main()
