//! SQLite persistence for preferences, favorites, recipes and operation history.

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;
use uuid::Uuid;

const SCHEMA_VERSION: i64 = 1;

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
            .query_row("SELECT value_json FROM settings WHERE key = ?1", params![key], |row| row.get(0))
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
            connection.execute("DELETE FROM favorites WHERE action_id = ?1", params![action_id])?;
        }
        Ok(())
    }

    pub fn favorites(&self) -> Result<Vec<String>, StorageError> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare("SELECT action_id FROM favorites ORDER BY created_at ASC")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(StorageError::from)
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
            let (id, action_id, input_count, output_count, input_bytes, output_bytes, destination, status, duration_ms, created_at) = row?;
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
         CREATE TABLE IF NOT EXISTS recipes(\n           id TEXT PRIMARY KEY,\n           name TEXT NOT NULL,\n           description TEXT NOT NULL,\n           icon TEXT NOT NULL,\n           steps_json TEXT NOT NULL,\n           enabled INTEGER NOT NULL DEFAULT 1,\n           created_at TEXT NOT NULL,\n           updated_at TEXT NOT NULL\n         );",
    )?;
    connection.execute("UPDATE schema_meta SET version = ?1", params![SCHEMA_VERSION])?;
    Ok(())
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
        assert_eq!(storage.get_json::<String>("theme").unwrap().as_deref(), Some("dark"));
        storage.set_favorite("pdf-merge", true).unwrap();
        assert_eq!(storage.favorites().unwrap(), vec!["pdf-merge"]);
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
}
