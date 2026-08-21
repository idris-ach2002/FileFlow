//! SQLite persistence for preferences, favorites, recipes, accounts and operation history.

pub mod auth;

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::path::Path;
use thiserror::Error;
use uuid::Uuid;

use auth::{AccountProfile, OnboardingPreferences, PasswordHash};

const SCHEMA_VERSION: i64 = 5;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error(transparent)]
    Database(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: Uuid,
    pub action_id: String,
    pub input_count: u64,
    pub output_count: u64,
    pub input_bytes: u64,
    pub output_bytes: u64,
    pub destination: Option<String>,
    pub status: String,
    pub duration_ms: u64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeRecord {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub steps_json: String,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationJobRecord {
    pub id: Uuid,
    pub recipe_id: Option<Uuid>,
    pub status: String,
    pub current_step: u64,
    pub total_steps: u64,
    pub input_paths: Vec<String>,
    pub outputs_by_step: std::collections::HashMap<String, Vec<String>>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchedFolderRecord {
    pub id: Uuid,
    pub path: String,
    pub recipe_id: Uuid,
    pub enabled: bool,
    pub recursive: bool,
    pub extensions: Vec<String>,
    pub stability_seconds: u64,
    pub last_scan_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct Storage {
    connection: Mutex<Connection>,
}

impl Storage {
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        configure(&connection)?;
        migrate(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn in_memory() -> Result<Self, StorageError> {
        let connection = Connection::open_in_memory()?;
        configure(&connection)?;
        migrate(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn account_count(&self) -> Result<u64, StorageError> {
        let count =
            self.connection
                .lock()
                .query_row("SELECT COUNT(*) FROM accounts", [], |row| {
                    row.get::<_, i64>(0)
                })?;
        Ok(count.max(0) as u64)
    }

    /// Profiles known by this local installation, ordered like an account chooser.
    /// Password hashes never leave the storage layer.
    pub fn account_profiles(&self) -> Result<Vec<AccountProfile>, StorageError> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT a.id, a.email, a.created_at, a.updated_at, p.display_name, p.first_name, p.last_name, p.avatar_path \
             FROM accounts a JOIN profiles p ON p.account_id = a.id \
             LEFT JOIN account_usage u ON u.account_id = a.id \
             ORDER BY (u.last_used_at IS NULL) ASC, u.last_used_at DESC, a.created_at ASC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
            ))
        })?;
        rows.map(|row| profile_tuple(row?))
            .collect::<Result<Vec<_>, StorageError>>()
    }

    pub fn touch_account(&self, account_id: Uuid, at: DateTime<Utc>) -> Result<(), StorageError> {
        self.connection.lock().execute(
            "INSERT INTO account_usage(account_id, last_used_at) VALUES (?1, ?2) \
             ON CONFLICT(account_id) DO UPDATE SET last_used_at=excluded.last_used_at",
            params![account_id.to_string(), at.to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn remember_session(
        &self,
        account_id: Uuid,
        expires_at: DateTime<Utc>,
    ) -> Result<(), StorageError> {
        let now = Utc::now();
        let mut connection = self.connection.lock();
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO trusted_sessions(account_id, expires_at, last_used_at) VALUES (?1, ?2, ?3) \
             ON CONFLICT(account_id) DO UPDATE SET expires_at=excluded.expires_at, last_used_at=excluded.last_used_at",
            params![
                account_id.to_string(),
                expires_at.to_rfc3339(),
                now.to_rfc3339()
            ],
        )?;
        transaction.execute(
            "INSERT INTO account_usage(account_id, last_used_at) VALUES (?1, ?2) \
             ON CONFLICT(account_id) DO UPDATE SET last_used_at=excluded.last_used_at",
            params![account_id.to_string(), now.to_rfc3339()],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn trusted_account(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Option<(Uuid, DateTime<Utc>)>, StorageError> {
        let connection = self.connection.lock();
        let row = connection
            .query_row(
                "SELECT account_id, expires_at FROM trusted_sessions \
                 WHERE expires_at > ?1 ORDER BY last_used_at DESC LIMIT 1",
                params![now.to_rfc3339()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        Ok(row.and_then(|(account_id, expires_at)| {
            let account_id = Uuid::parse_str(&account_id).ok()?;
            let expires_at = DateTime::parse_from_rfc3339(&expires_at)
                .ok()?
                .with_timezone(&Utc);
            Some((account_id, expires_at))
        }))
    }

    pub fn clear_trusted_session(&self, account_id: Uuid) -> Result<(), StorageError> {
        self.connection.lock().execute(
            "DELETE FROM trusted_sessions WHERE account_id = ?1",
            params![account_id.to_string()],
        )?;
        Ok(())
    }

    pub fn purge_expired_trusted_sessions(
        &self,
        now: DateTime<Utc>,
    ) -> Result<usize, StorageError> {
        let removed = self.connection.lock().execute(
            "DELETE FROM trusted_sessions WHERE expires_at <= ?1",
            params![now.to_rfc3339()],
        )?;
        Ok(removed)
    }

    pub fn create_account(
        &self,
        profile: &AccountProfile,
        password_hash: &PasswordHash,
        onboarding: &OnboardingPreferences,
    ) -> Result<(), StorageError> {
        let password_json = serde_json::to_string(password_hash)?;
        let mut connection = self.connection.lock();
        let transaction = connection.transaction()?;
        let existing_accounts =
            transaction.query_row("SELECT COUNT(*) FROM accounts", [], |row| {
                row.get::<_, i64>(0)
            })?;
        transaction.execute(
            "INSERT INTO accounts(id, email, password_hash_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                profile.id.to_string(),
                profile.email,
                password_json,
                profile.created_at.to_rfc3339(),
                profile.updated_at.to_rfc3339(),
            ],
        )?;
        transaction.execute(
            "INSERT INTO profiles(account_id, display_name, first_name, last_name, avatar_path, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                profile.id.to_string(),
                profile.display_name,
                profile.first_name,
                profile.last_name,
                profile.avatar_path.as_ref().map(|value| value.to_string_lossy().into_owned()),
                profile.updated_at.to_rfc3339(),
            ],
        )?;
        transaction.execute(
            "INSERT INTO onboarding(account_id, completed, storage_directory, language, beginner_mode, preserve_originals, notifications, confirm_destructive_actions, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                onboarding.account_id.to_string(),
                bool_i64(onboarding.completed),
                onboarding.storage_directory.as_ref().map(|value| value.to_string_lossy().into_owned()),
                onboarding.language,
                bool_i64(onboarding.beginner_mode),
                bool_i64(onboarding.preserve_originals),
                bool_i64(onboarding.notifications),
                bool_i64(onboarding.confirm_destructive_actions),
                onboarding.created_at.to_rfc3339(),
                onboarding.updated_at.to_rfc3339(),
            ],
        )?;
        transaction.execute(
            "INSERT OR REPLACE INTO account_usage(account_id, last_used_at) VALUES (?1, ?2)",
            params![profile.id.to_string(), profile.updated_at.to_rfc3339()],
        )?;
        if existing_accounts == 0 {
            let account_id = profile.id.to_string();
            transaction.execute(
                "INSERT OR IGNORE INTO account_favorites(account_id, action_id, created_at) SELECT ?1, action_id, created_at FROM favorites",
                params![account_id],
            )?;
            transaction.execute(
                "INSERT OR IGNORE INTO account_history(account_id, id, action_id, input_count, output_count, input_bytes, output_bytes, destination, status, duration_ms, created_at) SELECT ?1, id, action_id, input_count, output_count, input_bytes, output_bytes, destination, status, duration_ms, created_at FROM history",
                params![profile.id.to_string()],
            )?;
            transaction.execute(
                "INSERT OR IGNORE INTO account_recipes(account_id, id, name, description, icon, steps_json, enabled, created_at, updated_at) SELECT ?1, id, name, description, icon, steps_json, enabled, created_at, updated_at FROM recipes",
                params![profile.id.to_string()],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn account_by_email(
        &self,
        email: &str,
    ) -> Result<Option<(AccountProfile, PasswordHash, OnboardingPreferences)>, StorageError> {
        let connection = self.connection.lock();
        let row = connection
            .query_row(
                "SELECT a.id, a.email, a.password_hash_json, a.created_at, a.updated_at, p.display_name, p.first_name, p.last_name, p.avatar_path, o.completed, o.storage_directory, o.language, o.beginner_mode, o.preserve_originals, o.notifications, o.confirm_destructive_actions, o.created_at, o.updated_at FROM accounts a JOIN profiles p ON p.account_id = a.id JOIN onboarding o ON o.account_id = a.id WHERE a.email = ?1",
                params![email],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?, row.get::<_, String>(7)?, row.get::<_, Option<String>>(8)?,
                        row.get::<_, i64>(9)?, row.get::<_, Option<String>>(10)?, row.get::<_, String>(11)?,
                        row.get::<_, i64>(12)?, row.get::<_, i64>(13)?, row.get::<_, i64>(14)?,
                        row.get::<_, i64>(15)?, row.get::<_, String>(16)?, row.get::<_, String>(17)?,
                    ))
                },
            )
            .optional()?;
        row.map(account_tuple).transpose()
    }

    pub fn profile(&self, account_id: Uuid) -> Result<Option<AccountProfile>, StorageError> {
        let connection = self.connection.lock();
        let row = connection
            .query_row(
                "SELECT a.id, a.email, a.created_at, a.updated_at, p.display_name, p.first_name, p.last_name, p.avatar_path FROM accounts a JOIN profiles p ON p.account_id = a.id WHERE a.id = ?1",
                params![account_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?, row.get::<_, Option<String>>(7)?,
                    ))
                },
            )
            .optional()?;
        row.map(profile_tuple).transpose()
    }

    pub fn update_profile(&self, profile: &AccountProfile) -> Result<(), StorageError> {
        let mut connection = self.connection.lock();
        let transaction = connection.transaction()?;
        transaction.execute(
            "UPDATE accounts SET email = ?1, updated_at = ?2 WHERE id = ?3",
            params![
                profile.email,
                profile.updated_at.to_rfc3339(),
                profile.id.to_string()
            ],
        )?;
        transaction.execute(
            "UPDATE profiles SET display_name = ?1, first_name = ?2, last_name = ?3, avatar_path = ?4, updated_at = ?5 WHERE account_id = ?6",
            params![
                profile.display_name, profile.first_name, profile.last_name,
                profile.avatar_path.as_ref().map(|value| value.to_string_lossy().into_owned()),
                profile.updated_at.to_rfc3339(), profile.id.to_string(),
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn onboarding(
        &self,
        account_id: Uuid,
    ) -> Result<Option<OnboardingPreferences>, StorageError> {
        let connection = self.connection.lock();
        let row = connection
            .query_row(
                "SELECT account_id, completed, storage_directory, language, beginner_mode, preserve_originals, notifications, confirm_destructive_actions, created_at, updated_at FROM onboarding WHERE account_id = ?1",
                params![account_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?, row.get::<_, i64>(4)?, row.get::<_, i64>(5)?,
                        row.get::<_, i64>(6)?, row.get::<_, i64>(7)?, row.get::<_, String>(8)?, row.get::<_, String>(9)?,
                    ))
                },
            )
            .optional()?;
        row.map(onboarding_tuple).transpose()
    }

    pub fn save_onboarding(&self, value: &OnboardingPreferences) -> Result<(), StorageError> {
        self.connection.lock().execute(
            "INSERT INTO onboarding(account_id, completed, storage_directory, language, beginner_mode, preserve_originals, notifications, confirm_destructive_actions, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) ON CONFLICT(account_id) DO UPDATE SET completed=excluded.completed, storage_directory=excluded.storage_directory, language=excluded.language, beginner_mode=excluded.beginner_mode, preserve_originals=excluded.preserve_originals, notifications=excluded.notifications, confirm_destructive_actions=excluded.confirm_destructive_actions, updated_at=excluded.updated_at",
            params![
                value.account_id.to_string(), bool_i64(value.completed),
                value.storage_directory.as_ref().map(|path| path.to_string_lossy().into_owned()),
                value.language, bool_i64(value.beginner_mode), bool_i64(value.preserve_originals),
                bool_i64(value.notifications), bool_i64(value.confirm_destructive_actions),
                value.created_at.to_rfc3339(), value.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn change_password_hash(
        &self,
        account_id: Uuid,
        password_hash: &PasswordHash,
    ) -> Result<(), StorageError> {
        let json = serde_json::to_string(password_hash)?;
        self.connection.lock().execute(
            "UPDATE accounts SET password_hash_json = ?1, updated_at = ?2 WHERE id = ?3",
            params![json, Utc::now().to_rfc3339(), account_id.to_string()],
        )?;
        Ok(())
    }

    pub fn set_favorite_for(
        &self,
        account_id: Uuid,
        action_id: &str,
        favorite: bool,
    ) -> Result<(), StorageError> {
        let connection = self.connection.lock();
        if favorite {
            connection.execute(
                "INSERT OR IGNORE INTO account_favorites(account_id, action_id, created_at) VALUES (?1, ?2, ?3)",
                params![account_id.to_string(), action_id, Utc::now().to_rfc3339()],
            )?;
        } else {
            connection.execute(
                "DELETE FROM account_favorites WHERE account_id = ?1 AND action_id = ?2",
                params![account_id.to_string(), action_id],
            )?;
        }
        Ok(())
    }

    pub fn favorites_for(&self, account_id: Uuid) -> Result<Vec<String>, StorageError> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT action_id FROM account_favorites WHERE account_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = statement.query_map(params![account_id.to_string()], |row| {
            row.get::<_, String>(0)
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }

    pub fn record_history_for(
        &self,
        account_id: Uuid,
        entry: &HistoryEntry,
    ) -> Result<(), StorageError> {
        self.connection.lock().execute(
            "INSERT INTO account_history(account_id, id, action_id, input_count, output_count, input_bytes, output_bytes, destination, status, duration_ms, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                account_id.to_string(), entry.id.to_string(), entry.action_id,
                as_i64(entry.input_count), as_i64(entry.output_count), as_i64(entry.input_bytes),
                as_i64(entry.output_bytes), entry.destination, entry.status,
                as_i64(entry.duration_ms), entry.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn history_for(
        &self,
        account_id: Uuid,
        limit: usize,
    ) -> Result<Vec<HistoryEntry>, StorageError> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT id, action_id, input_count, output_count, input_bytes, output_bytes, destination, status, duration_ms, created_at FROM account_history WHERE account_id = ?1 ORDER BY created_at DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(
            params![account_id.to_string(), limit.clamp(1, 500) as i64],
            |row| {
                let id: String = row.get(0)?;
                let created_at: String = row.get(9)?;
                Ok(HistoryEntry {
                    id: Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::nil()),
                    action_id: row.get(1)?,
                    input_count: row.get::<_, i64>(2)?.max(0) as u64,
                    output_count: row.get::<_, i64>(3)?.max(0) as u64,
                    input_bytes: row.get::<_, i64>(4)?.max(0) as u64,
                    output_bytes: row.get::<_, i64>(5)?.max(0) as u64,
                    destination: row.get(6)?,
                    status: row.get(7)?,
                    duration_ms: row.get::<_, i64>(8)?.max(0) as u64,
                    created_at: parse_time(created_at),
                })
            },
        )?;
        rows.collect::<Result<Vec<_>, rusqlite::Error>>()
            .map_err(StorageError::from)
    }

    pub fn save_recipe_for(
        &self,
        account_id: Uuid,
        recipe: &RecipeRecord,
    ) -> Result<(), StorageError> {
        self.connection.lock().execute(
            "INSERT INTO account_recipes(account_id, id, name, description, icon, steps_json, enabled, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) ON CONFLICT(account_id, id) DO UPDATE SET name=excluded.name, description=excluded.description, icon=excluded.icon, steps_json=excluded.steps_json, enabled=excluded.enabled, updated_at=excluded.updated_at",
            params![
                account_id.to_string(), recipe.id.to_string(), recipe.name, recipe.description,
                recipe.icon, recipe.steps_json, if recipe.enabled { 1_i64 } else { 0_i64 },
                recipe.created_at.to_rfc3339(), recipe.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn recipes_for(&self, account_id: Uuid) -> Result<Vec<RecipeRecord>, StorageError> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT id, name, description, icon, steps_json, enabled, created_at, updated_at FROM account_recipes WHERE account_id = ?1 ORDER BY updated_at DESC",
        )?;
        let rows = statement.query_map(params![account_id.to_string()], |row| {
            let id: String = row.get(0)?;
            let created_at: String = row.get(6)?;
            let updated_at: String = row.get(7)?;
            Ok(RecipeRecord {
                id: Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::nil()),
                name: row.get(1)?,
                description: row.get(2)?,
                icon: row.get(3)?,
                steps_json: row.get(4)?,
                enabled: row.get::<_, i64>(5)? != 0,
                created_at: parse_time(created_at),
                updated_at: parse_time(updated_at),
            })
        })?;
        rows.collect::<Result<Vec<_>, rusqlite::Error>>()
            .map_err(StorageError::from)
    }

    pub fn save_automation_job_for(
        &self,
        account_id: Uuid,
        job: &AutomationJobRecord,
    ) -> Result<(), StorageError> {
        let inputs_json = serde_json::to_string(&job.input_paths)?;
        let outputs_json = serde_json::to_string(&job.outputs_by_step)?;
        self.connection.lock().execute(
            "INSERT INTO automation_jobs(account_id, id, recipe_id, status, current_step, total_steps, input_paths_json, outputs_json, error, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11) ON CONFLICT(account_id, id) DO UPDATE SET recipe_id=excluded.recipe_id, status=excluded.status, current_step=excluded.current_step, total_steps=excluded.total_steps, input_paths_json=excluded.input_paths_json, outputs_json=excluded.outputs_json, error=excluded.error, updated_at=excluded.updated_at",
            params![
                account_id.to_string(),
                job.id.to_string(),
                job.recipe_id.map(|id| id.to_string()),
                job.status,
                as_i64(job.current_step),
                as_i64(job.total_steps),
                inputs_json,
                outputs_json,
                job.error,
                job.created_at.to_rfc3339(),
                job.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn automation_jobs_for(
        &self,
        account_id: Uuid,
        limit: usize,
    ) -> Result<Vec<AutomationJobRecord>, StorageError> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT id, recipe_id, status, current_step, total_steps, input_paths_json, outputs_json, error, created_at, updated_at FROM automation_jobs WHERE account_id = ?1 ORDER BY updated_at DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(
            params![account_id.to_string(), limit.clamp(1, 500) as i64],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                ))
            },
        )?;
        rows.map(|row| {
            let (
                id,
                recipe_id,
                status,
                current_step,
                total_steps,
                inputs_json,
                outputs_json,
                error,
                created_at,
                updated_at,
            ) = row?;
            Ok(AutomationJobRecord {
                id: Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::nil()),
                recipe_id: recipe_id.and_then(|value| Uuid::parse_str(&value).ok()),
                status,
                current_step: current_step.max(0) as u64,
                total_steps: total_steps.max(0) as u64,
                input_paths: serde_json::from_str(&inputs_json).unwrap_or_default(),
                outputs_by_step: serde_json::from_str(&outputs_json).unwrap_or_default(),
                error,
                created_at: parse_time(created_at),
                updated_at: parse_time(updated_at),
            })
        })
        .collect::<Result<Vec<_>, StorageError>>()
    }

    pub fn automation_job_for(
        &self,
        account_id: Uuid,
        job_id: Uuid,
    ) -> Result<Option<AutomationJobRecord>, StorageError> {
        Ok(self
            .automation_jobs_for(account_id, 500)?
            .into_iter()
            .find(|job| job.id == job_id))
    }

    pub fn mark_running_jobs_interrupted(&self) -> Result<usize, StorageError> {
        let changed = self.connection.lock().execute(
            "UPDATE automation_jobs SET status='interrupted', error=COALESCE(error, 'FileFlow a été interrompu avant la fin du workflow.'), updated_at=?1 WHERE status IN ('queued','running','waitingForResources','finalizing')",
            params![Utc::now().to_rfc3339()],
        )?;
        Ok(changed)
    }

    pub fn save_watched_folder_for(
        &self,
        account_id: Uuid,
        watch: &WatchedFolderRecord,
    ) -> Result<(), StorageError> {
        let extensions_json = serde_json::to_string(&watch.extensions)?;
        self.connection.lock().execute(
            "INSERT INTO watched_folders(account_id, id, path, recipe_id, enabled, recursive, extensions_json, stability_seconds, last_scan_at, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11) ON CONFLICT(account_id, id) DO UPDATE SET path=excluded.path, recipe_id=excluded.recipe_id, enabled=excluded.enabled, recursive=excluded.recursive, extensions_json=excluded.extensions_json, stability_seconds=excluded.stability_seconds, last_scan_at=excluded.last_scan_at, updated_at=excluded.updated_at",
            params![
                account_id.to_string(),
                watch.id.to_string(),
                watch.path,
                watch.recipe_id.to_string(),
                bool_i64(watch.enabled),
                bool_i64(watch.recursive),
                extensions_json,
                as_i64(watch.stability_seconds),
                watch.last_scan_at.map(|value| value.to_rfc3339()),
                watch.created_at.to_rfc3339(),
                watch.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn watched_folders_for(
        &self,
        account_id: Uuid,
    ) -> Result<Vec<WatchedFolderRecord>, StorageError> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT id, path, recipe_id, enabled, recursive, extensions_json, stability_seconds, last_scan_at, created_at, updated_at FROM watched_folders WHERE account_id=?1 ORDER BY updated_at DESC",
        )?;
        let rows = statement.query_map(params![account_id.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
            ))
        })?;
        rows.map(|row| {
            let (
                id,
                path,
                recipe_id,
                enabled,
                recursive,
                extensions_json,
                stability_seconds,
                last_scan_at,
                created_at,
                updated_at,
            ) = row?;
            Ok(WatchedFolderRecord {
                id: Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::nil()),
                path,
                recipe_id: Uuid::parse_str(&recipe_id).unwrap_or_else(|_| Uuid::nil()),
                enabled: enabled != 0,
                recursive: recursive != 0,
                extensions: serde_json::from_str(&extensions_json).unwrap_or_default(),
                stability_seconds: stability_seconds.max(1) as u64,
                last_scan_at: last_scan_at.map(parse_time),
                created_at: parse_time(created_at),
                updated_at: parse_time(updated_at),
            })
        })
        .collect::<Result<Vec<_>, StorageError>>()
    }

    pub fn delete_watched_folder_for(
        &self,
        account_id: Uuid,
        watch_id: Uuid,
    ) -> Result<(), StorageError> {
        self.connection.lock().execute(
            "DELETE FROM watched_folders WHERE account_id=?1 AND id=?2",
            params![account_id.to_string(), watch_id.to_string()],
        )?;
        self.connection.lock().execute(
            "DELETE FROM watched_seen WHERE account_id=?1 AND watch_id=?2",
            params![account_id.to_string(), watch_id.to_string()],
        )?;
        Ok(())
    }

    pub fn mark_watched_folder_scanned(
        &self,
        account_id: Uuid,
        watch_id: Uuid,
        scanned_at: DateTime<Utc>,
    ) -> Result<(), StorageError> {
        self.connection.lock().execute(
            "UPDATE watched_folders SET last_scan_at=?1, updated_at=?1 WHERE account_id=?2 AND id=?3",
            params![scanned_at.to_rfc3339(), account_id.to_string(), watch_id.to_string()],
        )?;
        Ok(())
    }

    pub fn watch_seen_signature(
        &self,
        account_id: Uuid,
        watch_id: Uuid,
        path: &str,
    ) -> Result<Option<String>, StorageError> {
        self.connection
            .lock()
            .query_row(
                "SELECT signature FROM watched_seen WHERE account_id=?1 AND watch_id=?2 AND path=?3",
                params![account_id.to_string(), watch_id.to_string(), path],
                |row| row.get(0),
            )
            .optional()
            .map_err(StorageError::from)
    }

    pub fn mark_watch_seen(
        &self,
        account_id: Uuid,
        watch_id: Uuid,
        path: &str,
        signature: &str,
    ) -> Result<(), StorageError> {
        self.connection.lock().execute(
            "INSERT INTO watched_seen(account_id, watch_id, path, signature, processed_at) VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(account_id, watch_id, path) DO UPDATE SET signature=excluded.signature, processed_at=excluded.processed_at",
            params![account_id.to_string(), watch_id.to_string(), path, signature, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn put_json<T: Serialize>(&self, key: &str, value: &T) -> Result<(), StorageError> {
        let json = serde_json::to_string(value)?;
        self.connection.lock().execute(
            "INSERT INTO settings(key, value_json, updated_at) VALUES (?1, ?2, ?3)\n             ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json, updated_at=excluded.updated_at",
            params![key, json, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn get_json<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, StorageError> {
        let json: Option<String> = self
            .connection
            .lock()
            .query_row(
                "SELECT value_json FROM settings WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?;
        json.map(|value| serde_json::from_str(&value).map_err(StorageError::from))
            .transpose()
    }

    pub fn set_favorite(&self, action_id: &str, favorite: bool) -> Result<(), StorageError> {
        let connection = self.connection.lock();
        if favorite {
            connection.execute(
                "INSERT OR IGNORE INTO favorites(action_id, created_at) VALUES (?1, ?2)",
                params![action_id, Utc::now().to_rfc3339()],
            )?;
        } else {
            connection.execute(
                "DELETE FROM favorites WHERE action_id = ?1",
                params![action_id],
            )?;
        }
        Ok(())
    }

    pub fn favorites(&self) -> Result<Vec<String>, StorageError> {
        let connection = self.connection.lock();
        let mut statement =
            connection.prepare("SELECT action_id FROM favorites ORDER BY created_at ASC")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }

    pub fn record_history(&self, entry: &HistoryEntry) -> Result<(), StorageError> {
        self.connection.lock().execute(
            "INSERT INTO history(id, action_id, input_count, output_count, input_bytes, output_bytes, destination, status, duration_ms, created_at)\n             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                entry.id.to_string(),
                entry.action_id,
                as_i64(entry.input_count),
                as_i64(entry.output_count),
                as_i64(entry.input_bytes),
                as_i64(entry.output_bytes),
                entry.destination,
                entry.status,
                as_i64(entry.duration_ms),
                entry.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn history(&self, limit: usize) -> Result<Vec<HistoryEntry>, StorageError> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT id, action_id, input_count, output_count, input_bytes, output_bytes, destination, status, duration_ms, created_at\n             FROM history ORDER BY created_at DESC LIMIT ?1",
        )?;
        let rows = statement.query_map(params![limit.clamp(1, 500) as i64], |row| {
            let id: String = row.get(0)?;
            let created_at: String = row.get(9)?;
            Ok((
                id,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, i64>(8)?,
                created_at,
            ))
        })?;

        rows.map(|row| {
            let (
                id,
                action_id,
                input_count,
                output_count,
                input_bytes,
                output_bytes,
                destination,
                status,
                duration_ms,
                created_at,
            ) = row?;
            Ok(HistoryEntry {
                id: Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::nil()),
                action_id,
                input_count: input_count.max(0) as u64,
                output_count: output_count.max(0) as u64,
                input_bytes: input_bytes.max(0) as u64,
                output_bytes: output_bytes.max(0) as u64,
                destination,
                status,
                duration_ms: duration_ms.max(0) as u64,
                created_at: DateTime::parse_from_rfc3339(&created_at)
                    .map(|value| value.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
            })
        })
        .collect::<Result<Vec<_>, rusqlite::Error>>()
        .map_err(StorageError::from)
    }

    pub fn save_recipe(&self, recipe: &RecipeRecord) -> Result<(), StorageError> {
        self.connection.lock().execute(
            "INSERT INTO recipes(id, name, description, icon, steps_json, enabled, created_at, updated_at)\n             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)\n             ON CONFLICT(id) DO UPDATE SET name=excluded.name, description=excluded.description, icon=excluded.icon, steps_json=excluded.steps_json, enabled=excluded.enabled, updated_at=excluded.updated_at",
            params![
                recipe.id.to_string(),
                recipe.name,
                recipe.description,
                recipe.icon,
                recipe.steps_json,
                if recipe.enabled { 1_i64 } else { 0_i64 },
                recipe.created_at.to_rfc3339(),
                recipe.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn recipes(&self) -> Result<Vec<RecipeRecord>, StorageError> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT id, name, description, icon, steps_json, enabled, created_at, updated_at FROM recipes ORDER BY updated_at DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
            ))
        })?;
        rows.map(|row| {
            let (id, name, description, icon, steps_json, enabled, created_at, updated_at) = row?;
            Ok(RecipeRecord {
                id: Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::nil()),
                name,
                description,
                icon,
                steps_json,
                enabled: enabled != 0,
                created_at: parse_time(created_at),
                updated_at: parse_time(updated_at),
            })
        })
        .collect::<Result<Vec<_>, rusqlite::Error>>()
        .map_err(StorageError::from)
    }
}

fn configure(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(
        "PRAGMA journal_mode=WAL;\n         PRAGMA foreign_keys=ON;\n         PRAGMA synchronous=NORMAL;\n         PRAGMA busy_timeout=3000;",
    )?;
    Ok(())
}

fn migrate(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_meta(version INTEGER NOT NULL);\n         INSERT INTO schema_meta(version) SELECT 0 WHERE NOT EXISTS (SELECT 1 FROM schema_meta);\n
         CREATE TABLE IF NOT EXISTS settings(\n           key TEXT PRIMARY KEY,\n           value_json TEXT NOT NULL,\n           updated_at TEXT NOT NULL\n         );\n
         CREATE TABLE IF NOT EXISTS favorites(\n           action_id TEXT PRIMARY KEY,\n           created_at TEXT NOT NULL\n         );\n
         CREATE TABLE IF NOT EXISTS history(\n           id TEXT PRIMARY KEY,\n           action_id TEXT NOT NULL,\n           input_count INTEGER NOT NULL,\n           output_count INTEGER NOT NULL,\n           input_bytes INTEGER NOT NULL,\n           output_bytes INTEGER NOT NULL,\n           destination TEXT,\n           status TEXT NOT NULL,\n           duration_ms INTEGER NOT NULL,\n           created_at TEXT NOT NULL\n         );\n         CREATE INDEX IF NOT EXISTS history_created_at ON history(created_at DESC);\n
         CREATE TABLE IF NOT EXISTS recipes(\n           id TEXT PRIMARY KEY,\n           name TEXT NOT NULL,\n           description TEXT NOT NULL,\n           icon TEXT NOT NULL,\n           steps_json TEXT NOT NULL,\n           enabled INTEGER NOT NULL DEFAULT 1,\n           created_at TEXT NOT NULL,\n           updated_at TEXT NOT NULL\n         );\n\n         CREATE TABLE IF NOT EXISTS accounts(\n           id TEXT PRIMARY KEY,\n           email TEXT NOT NULL UNIQUE COLLATE NOCASE,\n           password_hash_json TEXT NOT NULL,\n           created_at TEXT NOT NULL,\n           updated_at TEXT NOT NULL\n         );\n\n         CREATE TABLE IF NOT EXISTS account_usage(\n           account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,\n           last_used_at TEXT NOT NULL\n         );\n\n         CREATE TABLE IF NOT EXISTS trusted_sessions(\n           account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,\n           expires_at TEXT NOT NULL,\n           last_used_at TEXT NOT NULL\n         );\n         CREATE INDEX IF NOT EXISTS trusted_sessions_expiry ON trusted_sessions(expires_at);\n\n         CREATE TABLE IF NOT EXISTS profiles(\n           account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,\n           display_name TEXT NOT NULL,\n           first_name TEXT NOT NULL,\n           last_name TEXT NOT NULL,\n           avatar_path TEXT,\n           updated_at TEXT NOT NULL\n         );\n\n         CREATE TABLE IF NOT EXISTS onboarding(\n           account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,\n           completed INTEGER NOT NULL DEFAULT 0,\n           storage_directory TEXT,\n           language TEXT NOT NULL DEFAULT 'fr',\n           beginner_mode INTEGER NOT NULL DEFAULT 1,\n           preserve_originals INTEGER NOT NULL DEFAULT 1,\n           notifications INTEGER NOT NULL DEFAULT 1,\n           confirm_destructive_actions INTEGER NOT NULL DEFAULT 1,\n           created_at TEXT NOT NULL,\n           updated_at TEXT NOT NULL\n         );\n\n         CREATE TABLE IF NOT EXISTS account_favorites(\n           account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,\n           action_id TEXT NOT NULL,\n           created_at TEXT NOT NULL,\n           PRIMARY KEY(account_id, action_id)\n         );\n\n         CREATE TABLE IF NOT EXISTS account_history(\n           account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,\n           id TEXT NOT NULL,\n           action_id TEXT NOT NULL,\n           input_count INTEGER NOT NULL,\n           output_count INTEGER NOT NULL,\n           input_bytes INTEGER NOT NULL,\n           output_bytes INTEGER NOT NULL,\n           destination TEXT,\n           status TEXT NOT NULL,\n           duration_ms INTEGER NOT NULL,\n           created_at TEXT NOT NULL,\n           PRIMARY KEY(account_id, id)\n         );\n         CREATE INDEX IF NOT EXISTS account_history_created_at ON account_history(account_id, created_at DESC);\n\n         CREATE TABLE IF NOT EXISTS account_recipes(\n           account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,\n           id TEXT NOT NULL,\n           name TEXT NOT NULL,\n           description TEXT NOT NULL,\n           icon TEXT NOT NULL,\n           steps_json TEXT NOT NULL,\n           enabled INTEGER NOT NULL DEFAULT 1,\n           created_at TEXT NOT NULL,\n           updated_at TEXT NOT NULL,\n           PRIMARY KEY(account_id, id)\n         );\n\n         CREATE TABLE IF NOT EXISTS automation_jobs(\n           account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,\n           id TEXT NOT NULL,\n           recipe_id TEXT,\n           status TEXT NOT NULL,\n           current_step INTEGER NOT NULL DEFAULT 0,\n           total_steps INTEGER NOT NULL DEFAULT 0,\n           input_paths_json TEXT NOT NULL,\n           outputs_json TEXT NOT NULL DEFAULT '{}',\n           error TEXT,\n           created_at TEXT NOT NULL,\n           updated_at TEXT NOT NULL,\n           PRIMARY KEY(account_id, id)\n         );\n         CREATE INDEX IF NOT EXISTS automation_jobs_status ON automation_jobs(account_id, status, updated_at DESC);\n\n         CREATE TABLE IF NOT EXISTS watched_folders(\n           account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,\n           id TEXT NOT NULL,\n           path TEXT NOT NULL,\n           recipe_id TEXT NOT NULL,\n           enabled INTEGER NOT NULL DEFAULT 1,\n           recursive INTEGER NOT NULL DEFAULT 0,\n           extensions_json TEXT NOT NULL DEFAULT '[]',\n           stability_seconds INTEGER NOT NULL DEFAULT 3,\n           last_scan_at TEXT,\n           created_at TEXT NOT NULL,\n           updated_at TEXT NOT NULL,\n           PRIMARY KEY(account_id, id)\n         );\n\n         CREATE TABLE IF NOT EXISTS watched_seen(\n           account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,\n           watch_id TEXT NOT NULL,\n           path TEXT NOT NULL,\n           signature TEXT NOT NULL,\n           processed_at TEXT NOT NULL,\n           PRIMARY KEY(account_id, watch_id, path)\n         );",
    )?;
    connection.execute(
        "UPDATE schema_meta SET version = ?1",
        params![SCHEMA_VERSION],
    )?;
    Ok(())
}

fn bool_i64(value: bool) -> i64 {
    if value { 1 } else { 0 }
}

fn profile_tuple(
    row: (
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        Option<String>,
    ),
) -> Result<AccountProfile, StorageError> {
    let (id, email, created_at, updated_at, display_name, first_name, last_name, avatar_path) = row;
    Ok(AccountProfile {
        id: Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::nil()),
        email,
        display_name,
        first_name,
        last_name,
        avatar_path: avatar_path.map(Into::into),
        created_at: parse_time(created_at),
        updated_at: parse_time(updated_at),
    })
}

#[allow(clippy::type_complexity)]
fn account_tuple(
    row: (
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        i64,
        Option<String>,
        String,
        i64,
        i64,
        i64,
        i64,
        String,
        String,
    ),
) -> Result<(AccountProfile, PasswordHash, OnboardingPreferences), StorageError> {
    let (
        id,
        email,
        password_json,
        created_at,
        updated_at,
        display_name,
        first_name,
        last_name,
        avatar_path,
        completed,
        storage_directory,
        language,
        beginner_mode,
        preserve_originals,
        notifications,
        confirm_destructive_actions,
        onboarding_created_at,
        onboarding_updated_at,
    ) = row;
    let account_id = Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::nil());
    let password_hash = serde_json::from_str(&password_json)?;
    Ok((
        AccountProfile {
            id: account_id,
            email,
            display_name,
            first_name,
            last_name,
            avatar_path: avatar_path.map(Into::into),
            created_at: parse_time(created_at),
            updated_at: parse_time(updated_at),
        },
        password_hash,
        OnboardingPreferences {
            account_id,
            completed: completed != 0,
            storage_directory: storage_directory.map(Into::into),
            language,
            beginner_mode: beginner_mode != 0,
            preserve_originals: preserve_originals != 0,
            notifications: notifications != 0,
            confirm_destructive_actions: confirm_destructive_actions != 0,
            created_at: parse_time(onboarding_created_at),
            updated_at: parse_time(onboarding_updated_at),
        },
    ))
}

fn onboarding_tuple(
    row: (
        String,
        i64,
        Option<String>,
        String,
        i64,
        i64,
        i64,
        i64,
        String,
        String,
    ),
) -> Result<OnboardingPreferences, StorageError> {
    let (
        account_id,
        completed,
        storage_directory,
        language,
        beginner_mode,
        preserve_originals,
        notifications,
        confirm_destructive_actions,
        created_at,
        updated_at,
    ) = row;
    Ok(OnboardingPreferences {
        account_id: Uuid::parse_str(&account_id).unwrap_or_else(|_| Uuid::nil()),
        completed: completed != 0,
        storage_directory: storage_directory.map(Into::into),
        language,
        beginner_mode: beginner_mode != 0,
        preserve_originals: preserve_originals != 0,
        notifications: notifications != 0,
        confirm_destructive_actions: confirm_destructive_actions != 0,
        created_at: parse_time(created_at),
        updated_at: parse_time(updated_at),
    })
}

fn as_i64(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}

fn parse_time(value: String) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&value)
        .map(|time| time.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_settings_and_favorites() {
        let storage = Storage::in_memory().unwrap();
        storage.put_json("theme", &"dark").unwrap();
        assert_eq!(
            storage.get_json::<String>("theme").unwrap().as_deref(),
            Some("dark")
        );
        storage.set_favorite("pdf-merge", true).unwrap();
        assert_eq!(storage.favorites().unwrap(), vec!["pdf-merge"]);
    }

    #[test]
    fn round_trips_account_profile_and_onboarding() {
        let storage = Storage::in_memory().unwrap();
        let id = Uuid::new_v4();
        let now = Utc::now();
        let profile = AccountProfile {
            id,
            email: "person@example.test".into(),
            display_name: "Personne".into(),
            first_name: "Test".into(),
            last_name: "User".into(),
            avatar_path: None,
            created_at: now,
            updated_at: now,
        };
        let password = auth::hash_password("a sufficiently long password").unwrap();
        let mut onboarding = OnboardingPreferences::new(id);
        onboarding.storage_directory = Some(Path::new("/tmp/FileFlow").to_path_buf());
        storage
            .create_account(&profile, &password, &onboarding)
            .unwrap();

        let (loaded_profile, loaded_password, loaded_onboarding) = storage
            .account_by_email("person@example.test")
            .unwrap()
            .unwrap();
        assert_eq!(loaded_profile.id, id);
        assert_eq!(loaded_profile.display_name, "Personne");
        assert!(auth::verify_password(
            "a sufficiently long password",
            &loaded_password
        ));
        assert_eq!(loaded_onboarding.account_id, id);
        assert_eq!(storage.account_count().unwrap(), 1);
    }

    #[test]
    fn remembers_known_accounts_and_restores_only_unexpired_trusted_session() {
        let storage = Storage::in_memory().unwrap();
        let now = Utc::now();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();

        for (id, email, offset) in [
            (first, "first@example.test", 0_i64),
            (second, "second@example.test", 1_i64),
        ] {
            let created = now + chrono::Duration::seconds(offset);
            let profile = AccountProfile {
                id,
                email: email.into(),
                display_name: email.into(),
                first_name: String::new(),
                last_name: String::new(),
                avatar_path: None,
                created_at: created,
                updated_at: created,
            };
            storage
                .create_account(
                    &profile,
                    &auth::hash_password("a sufficiently long password").unwrap(),
                    &OnboardingPreferences::new(id),
                )
                .unwrap();
        }

        storage
            .touch_account(first, now + chrono::Duration::minutes(2))
            .unwrap();
        storage
            .remember_session(first, now + chrono::Duration::days(30))
            .unwrap();

        let known = storage.account_profiles().unwrap();
        assert_eq!(known.len(), 2);
        assert_eq!(known[0].id, first);

        let trusted = storage
            .trusted_account(now)
            .unwrap()
            .expect("trusted session");
        assert_eq!(trusted.0, first);
        storage.clear_trusted_session(first).unwrap();
        assert!(storage.trusted_account(now).unwrap().is_none());

        storage
            .remember_session(second, now - chrono::Duration::seconds(1))
            .unwrap();
        assert_eq!(storage.purge_expired_trusted_sessions(now).unwrap(), 1);
        assert!(storage.trusted_account(now).unwrap().is_none());
    }

    #[test]
    fn account_scoped_data_does_not_leak_between_profiles() {
        let storage = Storage::in_memory().unwrap();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let now = Utc::now();
        for (id, email) in [
            (first, "first@example.test"),
            (second, "second@example.test"),
        ] {
            let profile = AccountProfile {
                id,
                email: email.into(),
                display_name: email.into(),
                first_name: String::new(),
                last_name: String::new(),
                avatar_path: None,
                created_at: now,
                updated_at: now,
            };
            let password = auth::hash_password("a sufficiently long password").unwrap();
            let onboarding = OnboardingPreferences::new(id);
            storage
                .create_account(&profile, &password, &onboarding)
                .unwrap();
        }

        storage.set_favorite_for(first, "pdf-merge", true).unwrap();
        assert_eq!(storage.favorites_for(first).unwrap(), vec!["pdf-merge"]);
        assert!(storage.favorites_for(second).unwrap().is_empty());

        let entry = HistoryEntry {
            id: Uuid::new_v4(),
            action_id: "pdf-compress".into(),
            input_count: 1,
            output_count: 1,
            input_bytes: 100,
            output_bytes: 80,
            destination: None,
            status: "completed".into(),
            duration_ms: 10,
            created_at: now,
        };
        storage.record_history_for(first, &entry).unwrap();
        assert_eq!(storage.history_for(first, 20).unwrap().len(), 1);
        assert!(storage.history_for(second, 20).unwrap().is_empty());
    }

    #[test]
    fn records_history_in_reverse_chronological_order() {
        let storage = Storage::in_memory().unwrap();
        let entry = HistoryEntry {
            id: Uuid::new_v4(),
            action_id: "pdf-merge".into(),
            input_count: 2,
            output_count: 1,
            input_bytes: 100,
            output_bytes: 90,
            destination: Some("/tmp/result.pdf".into()),
            status: "completed".into(),
            duration_ms: 120,
            created_at: Utc::now(),
        };
        storage.record_history(&entry).unwrap();
        let history = storage.history(20).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].action_id, "pdf-merge");
    }
    #[test]
    fn persists_workflow_checkpoints_and_marks_interrupted_jobs() {
        let storage = Storage::in_memory().unwrap();
        let account_id = Uuid::new_v4();
        let now = Utc::now();
        let profile = AccountProfile {
            id: account_id,
            email: "automation@example.test".into(),
            display_name: "Automation".into(),
            first_name: String::new(),
            last_name: String::new(),
            avatar_path: None,
            created_at: now,
            updated_at: now,
        };
        storage
            .create_account(
                &profile,
                &auth::hash_password("a sufficiently long password").unwrap(),
                &OnboardingPreferences::new(account_id),
            )
            .unwrap();

        let job = AutomationJobRecord {
            id: Uuid::new_v4(),
            recipe_id: None,
            status: "running".into(),
            current_step: 2,
            total_steps: 4,
            input_paths: vec!["/tmp/input.jpg".into()],
            outputs_by_step: std::collections::HashMap::from([(
                "step-1".into(),
                vec!["/tmp/output.jpg".into()],
            )]),
            error: None,
            created_at: now,
            updated_at: now,
        };
        storage.save_automation_job_for(account_id, &job).unwrap();
        let loaded = storage
            .automation_job_for(account_id, job.id)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.current_step, 2);
        assert_eq!(loaded.outputs_by_step["step-1"], vec!["/tmp/output.jpg"]);
        assert_eq!(storage.mark_running_jobs_interrupted().unwrap(), 1);
        let interrupted = storage
            .automation_job_for(account_id, job.id)
            .unwrap()
            .unwrap();
        assert_eq!(interrupted.status, "interrupted");
    }

    #[test]
    fn watched_folder_state_and_seen_signatures_round_trip() {
        let storage = Storage::in_memory().unwrap();
        let account_id = Uuid::new_v4();
        let now = Utc::now();
        let profile = AccountProfile {
            id: account_id,
            email: "watch@example.test".into(),
            display_name: "Watch".into(),
            first_name: String::new(),
            last_name: String::new(),
            avatar_path: None,
            created_at: now,
            updated_at: now,
        };
        storage
            .create_account(
                &profile,
                &auth::hash_password("a sufficiently long password").unwrap(),
                &OnboardingPreferences::new(account_id),
            )
            .unwrap();
        let watch = WatchedFolderRecord {
            id: Uuid::new_v4(),
            path: "/tmp/inbox".into(),
            recipe_id: Uuid::new_v4(),
            enabled: true,
            recursive: true,
            extensions: vec!["jpg".into(), "png".into()],
            stability_seconds: 5,
            last_scan_at: None,
            created_at: now,
            updated_at: now,
        };
        storage.save_watched_folder_for(account_id, &watch).unwrap();
        let loaded = storage.watched_folders_for(account_id).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].extensions, vec!["jpg", "png"]);
        assert!(
            storage
                .watch_seen_signature(account_id, watch.id, "/tmp/inbox/a.jpg")
                .unwrap()
                .is_none()
        );
        storage
            .mark_watch_seen(account_id, watch.id, "/tmp/inbox/a.jpg", "12:34")
            .unwrap();
        assert_eq!(
            storage
                .watch_seen_signature(account_id, watch.id, "/tmp/inbox/a.jpg")
                .unwrap()
                .as_deref(),
            Some("12:34")
        );
        storage
            .delete_watched_folder_for(account_id, watch.id)
            .unwrap();
        assert!(storage.watched_folders_for(account_id).unwrap().is_empty());
        assert!(
            storage
                .watch_seen_signature(account_id, watch.id, "/tmp/inbox/a.jpg")
                .unwrap()
                .is_none()
        );
    }
}
