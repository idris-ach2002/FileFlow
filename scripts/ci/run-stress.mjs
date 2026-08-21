#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const cargo = process.platform === 'win32' ? 'cargo.exe' : 'cargo';
const stressFiles = process.env.FILEFLOW_STRESS_FILES || '2500';
const runs = [
  ['scheduler cancellation', ['test','-p','fileflow-scheduler','cancellation_stops_waiting_for_resources','--locked','--','--nocapture']],
  ['storage recovery', ['test','-p','fileflow-storage','persists_workflow_checkpoints_and_marks_interrupted_jobs','--locked','--','--nocapture']],
  ['intake bounded stress', ['test','-p','fileflow-intake','--test','stress_scan','--locked','--','--ignored','--nocapture']],
];
for (const [label,args] of runs) {
  console.log(`\n== ${label} ==`);
  const result=spawnSync(cargo,args,{cwd:root,stdio:'inherit',env:{...process.env,FILEFLOW_STRESS_FILES:stressFiles}});
  if (result.error) throw result.error;
  if (result.status!==0) process.exit(result.status??1);
}
console.log('\nFileFlow stress/recovery/cancellation suite passed.');
