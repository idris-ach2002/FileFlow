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

export function isDistributableArtifactName(name) {
  const normalized = String(name).toLowerCase();
  if (!normalized.startsWith('fileflow')) return false;
  return /\.(dmg|msi|exe|deb|rpm|appimage|bin|sig)$/i.test(normalized)
    || normalized.endsWith('.app.tar.gz');
}

export function selectWindowsSetupExecutable(paths) {
  const candidates = paths.filter((path) => {
    const name = String(path).replaceAll('\\', '/').split('/').pop()?.toLowerCase() || '';
    return name.endsWith('.exe')
      && !name.startsWith('uninstall')
      && !name.includes('cli');
  });
  const exact = candidates.find((path) => {
    const name = String(path).replaceAll('\\', '/').split('/').pop()?.toLowerCase();
    return name === 'fileflow-setup.exe' || name === 'fileflowsetup.exe';
  });
  return exact || candidates.find((path) => /fileflow[ _.-]?setup/i.test(String(path))) || candidates[0];
}
