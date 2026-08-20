use crate::{AppState, commands::account::require_active_session};
use fileflow_storage::{HistoryEntry, RecipeRecord};
use tauri::State;

#[tauri::command]
pub fn history(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> Result<Vec<HistoryEntry>, String> {
    let account_id = require_active_session(&state)?;
    state
        .storage
        .history_for(account_id, limit.unwrap_or(100))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn favorites(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let account_id = require_active_session(&state)?;
    state.storage.favorites_for(account_id).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_favorite(
    state: State<'_, AppState>,
    action_id: String,
    favorite: bool,
) -> Result<(), String> {
    let account_id = require_active_session(&state)?;
    state
        .storage
        .set_favorite_for(account_id, &action_id, favorite)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn recipes(state: State<'_, AppState>) -> Result<Vec<RecipeRecord>, String> {
    let account_id = require_active_session(&state)?;
    state.storage.recipes_for(account_id).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_recipe(state: State<'_, AppState>, recipe: RecipeRecord) -> Result<(), String> {
    let account_id = require_active_session(&state)?;
    state
        .storage
        .save_recipe_for(account_id, &recipe)
        .map_err(|error| error.to_string())
}


#[tauri::command]
pub fn load_app_preferences(state: State<'_, AppState>) -> Result<Option<serde_json::Value>, String> {
    let account_key = preference_key(&state);
    let stored = state
        .storage
        .get_json::<serde_json::Value>(&account_key)
        .map_err(|error| error.to_string())?;
    if stored.is_some() || account_key == "app.preferences.v2" {
        return Ok(stored);
    }
    state
        .storage
        .get_json::<serde_json::Value>("app.preferences.v2")
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_app_preferences(
    state: State<'_, AppState>,
    preferences: serde_json::Value,
) -> Result<(), String> {
    let key = preference_key(&state);
    state
        .storage
        .put_json(&key, &preferences)
        .map_err(|error| error.to_string())
}

fn preference_key(state: &AppState) -> String {
    require_active_session(state)
        .map(|account_id| format!("app.preferences.v2.account.{account_id}"))
        .unwrap_or_else(|_| "app.preferences.v2".into())
}
