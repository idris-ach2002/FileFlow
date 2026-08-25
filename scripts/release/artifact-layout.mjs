import { resolve } from 'node:path';

export function applicationReleaseRoot(repositoryRoot, target) {
  return resolve(repositoryRoot, 'target', target, 'release');
}

export function applicationBundleRoot(repositoryRoot, target) {
  return resolve(applicationReleaseRoot(repositoryRoot, target), 'bundle');
}

export function setupTargetRoot(repositoryRoot, environment = process.env) {
  return resolve(
    repositoryRoot,
    environment.FILEFLOW_SETUP_TARGET_DIR || 'target/fileflow-setup',
  );
}

export function setupReleaseRoot(repositoryRoot, target, environment = process.env) {
  return resolve(setupTargetRoot(repositoryRoot, environment), target, 'release');
}

export function setupBundleRoot(repositoryRoot, target, environment = process.env) {
  return resolve(setupReleaseRoot(repositoryRoot, target, environment), 'bundle');
}
