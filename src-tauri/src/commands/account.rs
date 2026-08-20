use crate::{AppState, LoginAttempt, SessionRecord};
use chrono::{Duration, Utc};
use fileflow_storage::auth::{
    AccountProfile, OnboardingPreferences, consume_password_work, hash_password, normalize_email,
    verify_password,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use uuid::Uuid;

const SESSION_HOURS: i64 = 12;
const MAX_AVATAR_BYTES: u64 = 4 * 1024 * 1024;
const LOGIN_FAILURES_BEFORE_DELAY: u32 = 5;
const MAX_LOGIN_DELAY_SECONDS: i64 = 15 * 60;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountBootstrap {
    pub has_account: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAccountRequest {
    pub email: String,
    pub password: String,
    pub display_name: String,
    pub first_name: String,
    pub last_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileUpdate {
    pub display_name: String,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthSessionResponse {
    pub token: String,
    pub expires_at: chrono::DateTime<Utc>,
    pub profile: AccountProfile,
    pub onboarding: OnboardingPreferences,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvatarPayload {
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

#[tauri::command]
pub fn account_bootstrap(state: State<'_, AppState>) -> Result<AccountBootstrap, String> {
    Ok(AccountBootstrap {
        has_account: state
            .storage
            .account_count()
            .map_err(|error| error.to_string())?
            > 0,
    })
}

#[tauri::command]
pub async fn create_account(
    state: State<'_, AppState>,
    request: CreateAccountRequest,
) -> Result<AuthSessionResponse, String> {
    let email = normalize_email(&request.email);
    validate_email(&email)?;
    if state
        .storage
        .account_by_email(&email)
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Err("Un compte utilise déjà cette adresse e-mail sur cet appareil.".into());
    }

    let password = request.password.clone();
    let password_hash = tokio::task::spawn_blocking(move || hash_password(&password))
        .await
        .map_err(|error| format!("Le calcul sécurisé du mot de passe a été interrompu : {error}"))?
        .map_err(|error| error.to_string())?;

    let account_id = Uuid::new_v4();
    let now = Utc::now();
    let profile = AccountProfile {
        id: account_id,
        email,
        display_name: clean_name(&request.display_name, 80),
        first_name: clean_name(&request.first_name, 80),
        last_name: clean_name(&request.last_name, 80),
        avatar_path: None,
        created_at: now,
        updated_at: now,
    };
    if profile.display_name.is_empty() {
        return Err("Choisissez un nom à afficher.".into());
    }
    let onboarding = OnboardingPreferences::new(account_id);
    state
        .storage
        .create_account(&profile, &password_hash, &onboarding)
        .map_err(|error| error.to_string())?;
    Ok(start_session(&state, profile, onboarding))
}

#[tauri::command]
pub async fn login(
    state: State<'_, AppState>,
    request: LoginRequest,
) -> Result<AuthSessionResponse, String> {
    let email = normalize_email(&request.email);
    validate_email(&email)?;
    check_login_allowed(&state, &email)?;

    let account = state
        .storage
        .account_by_email(&email)
        .map_err(|error| error.to_string())?;

    let password = request.password;
    let Some((profile, password_hash, onboarding)) = account else {
        tokio::task::spawn_blocking(move || consume_password_work(&password))
            .await
            .map_err(|error| format!("La vérification du mot de passe a été interrompue : {error}"))?;
        record_login_failure(&state, &email);
        return Err("Adresse e-mail ou mot de passe incorrect.".into());
    };

    let valid = tokio::task::spawn_blocking(move || verify_password(&password, &password_hash))
        .await
        .map_err(|error| format!("La vérification du mot de passe a été interrompue : {error}"))?;
    if !valid {
        record_login_failure(&state, &email);
        return Err("Adresse e-mail ou mot de passe incorrect.".into());
    }
    state.login_attempts.remove(&email);
    Ok(start_session(&state, profile, onboarding))
}

#[tauri::command]
pub async fn change_password(
    state: State<'_, AppState>,
    token: String,
    request: ChangePasswordRequest,
) -> Result<AuthSessionResponse, String> {
    let account_id = require_session(&state, &token)?;
    let profile = state
        .storage
        .profile(account_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Profil introuvable.".to_owned())?;
    let Some((stored_profile, password_hash, onboarding)) = state
        .storage
        .account_by_email(&profile.email)
        .map_err(|error| error.to_string())?
    else {
        return Err("Compte introuvable.".into());
    };
    if stored_profile.id != account_id {
        return Err("Session invalide.".into());
    }

    let current_password = request.current_password;
    let valid = tokio::task::spawn_blocking(move || verify_password(&current_password, &password_hash))
        .await
        .map_err(|error| format!("La vérification du mot de passe a été interrompue : {error}"))?;
    if !valid {
        return Err("Le mot de passe actuel est incorrect.".into());
    }

    let new_password = request.new_password;
    let new_hash = tokio::task::spawn_blocking(move || hash_password(&new_password))
        .await
        .map_err(|error| format!("Le calcul sécurisé du nouveau mot de passe a été interrompu : {error}"))?
        .map_err(|error| error.to_string())?;
    state
        .storage
        .change_password_hash(account_id, &new_hash)
        .map_err(|error| error.to_string())?;

    Ok(start_session(&state, profile, onboarding))
}

#[tauri::command]
pub fn logout(state: State<'_, AppState>, token: String) -> bool {
    let mut session = state.session.write();
    let matches = session.as_ref().is_some_and(|current| current.token == token);
    if matches {
        *session = None;
        drop(session);
        clear_session_runtime(&state);
        return true;
    }
    false
}

#[tauri::command]
pub fn current_session(
    state: State<'_, AppState>,
    token: String,
) -> Result<AuthSessionResponse, String> {
    let account_id = require_session(&state, &token)?;
    let profile = state
        .storage
        .profile(account_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Profil introuvable.".to_owned())?;
    let onboarding = state
        .storage
        .onboarding(account_id)
        .map_err(|error| error.to_string())?
        .unwrap_or_else(|| OnboardingPreferences::new(account_id));
    let expires_at = state
        .session
        .read()
        .as_ref()
        .map(|value| value.expires_at)
        .ok_or_else(|| "Session expirée.".to_owned())?;
    Ok(AuthSessionResponse {
        token,
        expires_at,
        profile,
        onboarding,
    })
}

#[tauri::command]
pub fn save_onboarding(
    state: State<'_, AppState>,
    token: String,
    mut onboarding: OnboardingPreferences,
) -> Result<OnboardingPreferences, String> {
    let account_id = require_session(&state, &token)?;
    onboarding.account_id = account_id;
    onboarding.language = normalize_language(&onboarding.language);
    onboarding.preserve_originals = true;
    onboarding.storage_directory = normalize_storage_directory(onboarding.storage_directory)?;
    if onboarding.completed && onboarding.storage_directory.is_none() {
        return Err("Choisissez un dossier FileFlow avant de terminer la configuration.".into());
    }
    onboarding.updated_at = Utc::now();
    state
        .storage
        .save_onboarding(&onboarding)
        .map_err(|error| error.to_string())?;
    Ok(onboarding)
}

#[tauri::command]
pub fn update_profile(
    state: State<'_, AppState>,
    token: String,
    request: ProfileUpdate,
) -> Result<AccountProfile, String> {
    let account_id = require_session(&state, &token)?;
    let mut profile = state
        .storage
        .profile(account_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Profil introuvable.".to_owned())?;
    let email = normalize_email(&request.email);
    validate_email(&email)?;
    if let Some((other, _, _)) = state
        .storage
        .account_by_email(&email)
        .map_err(|error| error.to_string())?
        && other.id != account_id
    {
        return Err("Cette adresse e-mail est déjà utilisée par un autre profil.".into());
    }
    profile.email = email;
    profile.display_name = clean_name(&request.display_name, 80);
    profile.first_name = clean_name(&request.first_name, 80);
    profile.last_name = clean_name(&request.last_name, 80);
    profile.updated_at = Utc::now();
    if profile.display_name.is_empty() {
        return Err("Le nom affiché ne peut pas être vide.".into());
    }
    state
        .storage
        .update_profile(&profile)
        .map_err(|error| error.to_string())?;
    Ok(profile)
}

#[tauri::command]
pub fn choose_profile_avatar(
    app: AppHandle,
    state: State<'_, AppState>,
    token: String,
) -> Result<Option<AccountProfile>, String> {
    let account_id = require_session(&state, &token)?;
    let Some(file) = app
        .dialog()
        .file()
        .set_title("Choisir une photo de profil")
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    let source = file.into_path().map_err(|error| error.to_string())?;
    let metadata = std::fs::metadata(&source).map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_AVATAR_BYTES {
        return Err("La photo doit être une image de moins de 4 Mo.".into());
    }
    let extension = validate_avatar_content(&source)?;
    let avatar_dir = state.data_dir.join("profiles").join(account_id.to_string());
    std::fs::create_dir_all(&avatar_dir).map_err(|error| error.to_string())?;
    let destination = avatar_dir.join(format!("avatar.{extension}"));
    std::fs::copy(&source, &destination).map_err(|error| error.to_string())?;

    let mut profile = state
        .storage
        .profile(account_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Profil introuvable.".to_owned())?;
    profile.avatar_path = Some(destination);
    profile.updated_at = Utc::now();
    state
        .storage
        .update_profile(&profile)
        .map_err(|error| error.to_string())?;
    Ok(Some(profile))
}

#[tauri::command]
pub fn profile_avatar(
    state: State<'_, AppState>,
    token: String,
) -> Result<Option<AvatarPayload>, String> {
    let account_id = require_session(&state, &token)?;
    let profile = state
        .storage
        .profile(account_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Profil introuvable.".to_owned())?;
    let Some(path) = profile.avatar_path else {
        return Ok(None);
    };
    let profile_root = state.data_dir.join("profiles").join(account_id.to_string());
    let canonical_root = profile_root
        .canonicalize()
        .map_err(|_| "Le dossier de profil n’existe plus.".to_owned())?;
    let canonical_path = path
        .canonicalize()
        .map_err(|_| "La photo de profil n’existe plus.".to_owned())?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err("Le chemin de la photo de profil est invalide.".into());
    }
    let metadata = std::fs::metadata(&canonical_path).map_err(|error| error.to_string())?;
    if metadata.len() > MAX_AVATAR_BYTES {
        return Err("Photo de profil trop volumineuse.".into());
    }
    let mime_type = match canonical_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "webp" => "image/webp",
        _ => "image/jpeg",
    }
    .to_owned();
    let bytes = std::fs::read(canonical_path).map_err(|error| error.to_string())?;
    Ok(Some(AvatarPayload { mime_type, bytes }))
}

#[tauri::command]
pub fn default_storage_directory(app: AppHandle) -> Result<PathBuf, String> {
    let base = app
        .path()
        .document_dir()
        .or_else(|_| app.path().home_dir())
        .map_err(|error| error.to_string())?;
    Ok(base.join("FileFlow"))
}

#[tauri::command]
pub fn choose_storage_directory(app: AppHandle) -> Result<Option<PathBuf>, String> {
    let Some(folder) = app
        .dialog()
        .file()
        .set_title("Choisir le dossier FileFlow")
        .blocking_pick_folder()
    else {
        return Ok(None);
    };
    folder.into_path().map(Some).map_err(|error| error.to_string())
}

fn start_session(
    state: &AppState,
    profile: AccountProfile,
    onboarding: OnboardingPreferences,
) -> AuthSessionResponse {
    let expires_at = Utc::now() + Duration::hours(SESSION_HOURS);
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    *state.session.write() = Some(SessionRecord {
        token: token.clone(),
        account_id: profile.id,
        expires_at,
    });
    AuthSessionResponse {
        token,
        expires_at,
        profile,
        onboarding,
    }
}

pub(crate) fn require_active_session(state: &AppState) -> Result<Uuid, String> {
    let mut session = state.session.write();
    let Some(current) = session.as_ref() else {
        return Err("Connectez-vous pour continuer.".into());
    };
    if current.expires_at <= Utc::now() {
        *session = None;
        drop(session);
        clear_session_runtime(state);
        return Err("Votre session a expiré. Reconnectez-vous.".into());
    }
    Ok(current.account_id)
}

pub(crate) fn require_session(state: &AppState, token: &str) -> Result<Uuid, String> {
    let mut session = state.session.write();
    let Some(current) = session.as_ref() else {
        return Err("Connectez-vous pour continuer.".into());
    };
    if current.expires_at <= Utc::now() {
        *session = None;
        drop(session);
        clear_session_runtime(state);
        return Err("Votre session a expiré. Reconnectez-vous.".into());
    }
    if current.token != token {
        return Err("Session invalide.".into());
    }
    Ok(current.account_id)
}

fn clear_session_runtime(state: &AppState) {
    for job in state.jobs.iter() {
        job.value().cancel();
    }
    state.recent_outputs.clear();
}

fn check_login_allowed(state: &AppState, email: &str) -> Result<(), String> {
    let Some(attempt) = state.login_attempts.get(email) else {
        return Ok(());
    };
    let Some(blocked_until) = attempt.blocked_until else {
        return Ok(());
    };
    let now = Utc::now();
    if blocked_until <= now {
        drop(attempt);
        state.login_attempts.remove(email);
        return Ok(());
    }
    let seconds = (blocked_until - now).num_seconds().max(1);
    Err(format!(
        "Trop de tentatives. Réessayez dans environ {seconds} seconde{}.",
        if seconds > 1 { "s" } else { "" }
    ))
}

fn record_login_failure(state: &AppState, email: &str) {
    let mut attempt = state
        .login_attempts
        .entry(email.to_owned())
        .or_insert(LoginAttempt {
            failures: 0,
            blocked_until: None,
        });
    attempt.failures = attempt.failures.saturating_add(1);
    if attempt.failures >= LOGIN_FAILURES_BEFORE_DELAY {
        let exponent = attempt
            .failures
            .saturating_sub(LOGIN_FAILURES_BEFORE_DELAY)
            .min(5);
        let seconds = (30_i64.saturating_mul(1_i64 << exponent)).min(MAX_LOGIN_DELAY_SECONDS);
        attempt.blocked_until = Some(Utc::now() + Duration::seconds(seconds));
    }
}

fn normalize_storage_directory(path: Option<PathBuf>) -> Result<Option<PathBuf>, String> {
    let Some(path) = path else {
        return Ok(None);
    };
    if path.as_os_str().is_empty() {
        return Err("Le dossier FileFlow ne peut pas être vide.".into());
    }
    if path.exists() {
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err("Le dossier FileFlow doit être un dossier réel, pas un fichier ou un lien symbolique.".into());
        }
    } else {
        std::fs::create_dir_all(&path).map_err(|error| {
            format!("Impossible de créer le dossier FileFlow : {error}")
        })?;
    }
    path.canonicalize()
        .map(Some)
        .map_err(|error| format!("Impossible de valider le dossier FileFlow : {error}"))
}

fn validate_avatar_content(path: &Path) -> Result<&'static str, String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut header = [0_u8; 16];
    let read = file.read(&mut header).map_err(|error| error.to_string())?;
    let bytes = &header[..read];
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Ok("jpg");
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok("png");
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Ok("webp");
    }
    Err("Formats de photo acceptés : JPG, PNG et WebP authentiques.".into())
}

fn validate_email(email: &str) -> Result<(), String> {
    let valid = email.len() <= 254
        && email
            .split_once('@')
            .is_some_and(|(local, domain)| !local.is_empty() && domain.contains('.') && !domain.ends_with('.'));
    if valid {
        Ok(())
    } else {
        Err("Saisissez une adresse e-mail valide.".into())
    }
}

fn clean_name(value: &str, max: usize) -> String {
    value
        .trim()
        .chars()
        .filter(|character| !character.is_control())
        .take(max)
        .collect()
}

fn normalize_language(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "en" => "en".into(),
        "de" => "de".into(),
        _ => "fr".into(),
    }
}
