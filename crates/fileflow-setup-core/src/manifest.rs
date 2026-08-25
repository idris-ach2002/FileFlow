use crate::{Architecture, Platform};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};
use thiserror::Error;

const EXPECTED_REPOSITORY: &str = "idris-ach2002/FileFlow";
const REQUIRED_PLATFORMS: &[&str] = &[
    "darwin-aarch64",
    "darwin-x86_64",
    "windows-x86_64",
    "linux-x86_64",
    "linux-aarch64",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadManifest {
    pub schema_version: u32,
    pub version: String,
    pub published_at: String,
    pub repository: String,
    pub platforms: BTreeMap<String, PlatformDownloads>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformDownloads {
    pub application: DownloadArtifact,
    pub setup: DownloadArtifact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadArtifact {
    pub name: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub signature: Option<String>,
    pub package_type: String,
}

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("downloads.json utilise un schéma incompatible")]
    Schema,
    #[error("version de release invalide: {0}")]
    Version(String),
    #[error("date de publication invalide: {0}")]
    PublishedAt(String),
    #[error("dépôt de release non autorisé: {0}")]
    Repository(String),
    #[error("plateforme absente du manifeste: {0}")]
    MissingPlatform(String),
    #[error("URL de téléchargement non sécurisée: {0}")]
    InsecureUrl(String),
    #[error("checksum SHA-256 invalide pour {0}")]
    InvalidChecksum(String),
    #[error("taille invalide pour {0}")]
    InvalidSize(String),
    #[error("nom de fichier invalide pour {0}")]
    InvalidName(String),
    #[error("le checksum de {0} ne correspond pas au manifeste")]
    ChecksumMismatch(String),
    #[error("lecture impossible: {0}")]
    Io(#[from] std::io::Error),
}

impl DownloadManifest {
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.schema_version != 1 {
            return Err(ManifestError::Schema);
        }
        if !valid_version(&self.version) {
            return Err(ManifestError::Version(self.version.clone()));
        }
        if chrono::DateTime::parse_from_rfc3339(&self.published_at).is_err() {
            return Err(ManifestError::PublishedAt(self.published_at.clone()));
        }
        if self.repository != EXPECTED_REPOSITORY {
            return Err(ManifestError::Repository(self.repository.clone()));
        }
        let prefix = format!(
            "https://github.com/{}/releases/download/v{}/",
            self.repository, self.version
        );
        for platform in REQUIRED_PLATFORMS {
            let downloads = self
                .platforms
                .get(*platform)
                .ok_or_else(|| ManifestError::MissingPlatform((*platform).into()))?;
            validate_artifact(&downloads.application, &prefix)?;
            validate_artifact(&downloads.setup, &prefix)?;
        }
        Ok(())
    }

    pub fn for_current_platform(
        &self,
        platform: Platform,
        architecture: Architecture,
    ) -> Result<&PlatformDownloads, ManifestError> {
        let key = platform_key(platform, architecture);
        self.platforms
            .get(&key)
            .ok_or(ManifestError::MissingPlatform(key))
    }
}

pub fn platform_key(platform: Platform, architecture: Architecture) -> String {
    let os = match platform {
        Platform::Macos => "darwin",
        Platform::Windows => "windows",
        Platform::Linux => "linux",
    };
    let arch = match architecture {
        Architecture::X86_64 => "x86_64",
        Architecture::Aarch64 => "aarch64",
    };
    format!("{os}-{arch}")
}

pub fn sha256_file(path: &Path) -> Result<String, ManifestError> {
    let mut source = std::fs::File::open(path)?;
    let mut digest = Sha256::new();
    std::io::copy(&mut source, &mut digest)?;
    Ok(format!("{:x}", digest.finalize()))
}

pub fn verify_artifact(path: &Path, artifact: &DownloadArtifact) -> Result<(), ManifestError> {
    let metadata = std::fs::metadata(path)?;
    if metadata.len() != artifact.size {
        return Err(ManifestError::InvalidSize(artifact.name.clone()));
    }
    let actual = sha256_file(path)?;
    if !actual.eq_ignore_ascii_case(&artifact.sha256) {
        return Err(ManifestError::ChecksumMismatch(artifact.name.clone()));
    }
    Ok(())
}

fn validate_artifact(
    artifact: &DownloadArtifact,
    release_prefix: &str,
) -> Result<(), ManifestError> {
    if artifact.name.is_empty()
        || artifact.name == "."
        || artifact.name == ".."
        || artifact.name.contains('/')
        || artifact.name.contains('\\')
        || Path::new(&artifact.name)
            .file_name()
            .and_then(|name| name.to_str())
            != Some(artifact.name.as_str())
    {
        return Err(ManifestError::InvalidName(artifact.name.clone()));
    }
    if !artifact.url.starts_with(release_prefix)
        || !artifact.url.ends_with(&format!("/{}", artifact.name))
    {
        return Err(ManifestError::InsecureUrl(artifact.url.clone()));
    }
    if artifact.sha256.len() != 64 || !artifact.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(ManifestError::InvalidChecksum(artifact.name.clone()));
    }
    if artifact.size == 0 {
        return Err(ManifestError::InvalidSize(artifact.name.clone()));
    }
    Ok(())
}

fn valid_version(version: &str) -> bool {
    let main = version.split(['-', '+']).next().unwrap_or_default();
    let parts = main.split('.').collect::<Vec<_>>();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(name: &str) -> DownloadArtifact {
        DownloadArtifact {
            name: name.into(),
            url: format!(
                "https://github.com/{EXPECTED_REPOSITORY}/releases/download/v1.2.3/{name}"
            ),
            sha256: "a".repeat(64),
            size: 42,
            signature: None,
            package_type: "dmg".into(),
        }
    }

    #[test]
    fn validates_secure_download_manifest() {
        let manifest = DownloadManifest {
            schema_version: 1,
            version: "1.2.3".into(),
            published_at: "2026-08-24T00:00:00Z".into(),
            repository: EXPECTED_REPOSITORY.into(),
            platforms: REQUIRED_PLATFORMS
                .iter()
                .map(|platform| {
                    (
                        (*platform).into(),
                        PlatformDownloads {
                            application: artifact(&format!("FileFlow-{platform}.dmg")),
                            setup: artifact(&format!("FileFlowSetup-{platform}.dmg")),
                        },
                    )
                })
                .collect(),
        };
        assert!(manifest.validate().is_ok());
        assert!(
            manifest
                .for_current_platform(Platform::Macos, Architecture::Aarch64)
                .is_ok()
        );
    }

    #[test]
    fn rejects_insecure_artifact_url() {
        let mut item = artifact("FileFlow.dmg");
        item.url = "http://example.test/FileFlow.dmg".into();
        assert!(matches!(
            validate_artifact(
                &item,
                &format!("https://github.com/{EXPECTED_REPOSITORY}/releases/download/v1.2.3/")
            ),
            Err(ManifestError::InsecureUrl(_))
        ));
    }

    #[test]
    fn rejects_artifact_path_traversal() {
        let item = artifact("../../FileFlow.dmg");
        assert!(matches!(
            validate_artifact(
                &item,
                &format!("https://github.com/{EXPECTED_REPOSITORY}/releases/download/v1.2.3/")
            ),
            Err(ManifestError::InvalidName(_))
        ));
    }
}
