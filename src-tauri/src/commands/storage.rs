use crate::AppState;
use fileflow_storage::{HistoryEntry, RecipeRecord};
use tauri::State;

#[tauri::command]
pub fn history(state: State<'_, AppState>, limit: Option<usize>) -> Result<Vec<HistoryEntry>, String> {
    state.storage.history(limit.unwrap_or(100)).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn favorites(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    state.storage.favorites().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_favorite(state: State<'_, AppState>, action_id: String, favorite: bool) -> Result<(), String> {
    state.storage.set_favorite(&action_id, favorite).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn recipes(state: State<'_, AppState>) -> Result<Vec<RecipeRecord>, String> {
    state.storage.recipes().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_recipe(state: State<'_, AppState>, recipe: RecipeRecord) -> Result<(), String> {
    state.storage.save_recipe(&recipe).map_err(|error| error.to_string())
}
