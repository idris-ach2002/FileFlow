use crate::{AppState, commands::account::require_active_session};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::State;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PathAccess {
    Read,
    Write,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathValidation {
    pub input: String,
    pub normalized: Option<PathBuf>,
    pub valid: bool,
    pub exists: bool,
    pub is_directory: bool,
    pub is_file: bool,
    pub is_symlink: bool,
    pub readable: bool,
    pub writable: bool,
    pub message: String,
}

#[tauri::command]
pub fn validate_system_path(
    state: State<'_, AppState>,
    path: String,
    access: PathAccess,
    require_directory: bool,
) -> Result<PathValidation, String> {
    require_active_session(&state)?;
    let input = path.trim().to_owned();
    if input.is_empty() {
        return Ok(invalid(&input, "Saisissez ou choisissez un chemin."));
    }
    let expanded = match expand_home(&input) {
        Ok(path) => path,
        Err(message) => return Ok(invalid(&input, &message)),
    };
    if !expanded.is_absolute() {
        return Ok(invalid(&input, "Utilisez un chemin absolu."));
    }
    let metadata = match fs::symlink_metadata(&expanded) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(invalid(&input, "Ce chemin n’existe pas sur cet appareil."));
        }
        Err(error) => return Ok(invalid(&input, &format!("Chemin inaccessible : {error}"))),
    };
    let is_symlink = metadata.file_type().is_symlink();
    if is_symlink {
        let mut result = invalid(
            &input,
            "Les liens symboliques ne sont pas acceptés par défaut.",
        );
        result.exists = true;
        result.is_symlink = true;
        return Ok(result);
    }
    let is_directory = metadata.is_dir();
    let is_file = metadata.is_file();
    if require_directory && !is_directory {
        let mut result = invalid(&input, "Ce chemin existe, mais ce n’est pas un dossier.");
        result.exists = true;
        result.is_file = is_file;
        return Ok(result);
    }
    let normalized = match expanded.canonicalize() {
        Ok(path) => path,
        Err(error) => {
            return Ok(invalid(
                &input,
                &format!("Impossible de normaliser ce chemin : {error}"),
            ));
        }
    };
    let readable = if is_directory {
        fs::read_dir(&normalized).is_ok()
    } else {
        fs::File::open(&normalized).is_ok()
    };
    let writable = match access {
        PathAccess::Read => true,
        PathAccess::Write if is_directory => probe_directory_write(&normalized),
        PathAccess::Write => false,
    };
    let valid = readable && writable;
    let message = if !readable {
        "Le chemin existe mais FileFlow ne peut pas le lire."
    } else if !writable {
        "Le dossier existe mais FileFlow ne peut pas y écrire."
    } else {
        "Chemin vérifié sur cet appareil."
    };
    Ok(PathValidation {
        input,
        normalized: Some(normalized),
        valid,
        exists: true,
        is_directory,
        is_file,
        is_symlink: false,
        readable,
        writable,
        message: message.into(),
    })
}

fn invalid(input: &str, message: &str) -> PathValidation {
    PathValidation {
        input: input.into(),
        normalized: None,
        valid: false,
        exists: false,
        is_directory: false,
        is_file: false,
        is_symlink: false,
        readable: false,
        writable: false,
        message: message.into(),
    }
}

fn expand_home(value: &str) -> Result<PathBuf, String> {
    if value == "~" || value.starts_with("~/") || value.starts_with("~\\") {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .ok_or_else(|| "Le dossier personnel est introuvable.".to_owned())?;
        if value.len() == 1 {
            return Ok(PathBuf::from(home));
        }
        return Ok(PathBuf::from(home).join(&value[2..]));
    }
    Ok(PathBuf::from(value))
}

fn probe_directory_write(directory: &Path) -> bool {
    let probe = directory.join(format!(".fileflow-write-test-{}", Uuid::new_v4().simple()));
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(file) => {
            drop(file);
            fs::remove_file(probe).is_ok()
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_absolute_paths_are_unchanged() {
        let path = if cfg!(windows) {
            r"C:\FileFlow"
        } else {
            "/tmp/FileFlow"
        };
        assert_eq!(expand_home(path).unwrap(), PathBuf::from(path));
    }
}
