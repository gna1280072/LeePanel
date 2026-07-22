use crate::{DbPool, config::{ConfigManager, Connection, Settings, SettingsManager, Favorite, FavoritesManager}};

// ponytail: clear proxy env vars on demand so updater can retry without proxy
#[tauri::command]
pub fn clear_proxy_env() {
    for var in ["http_proxy", "https_proxy", "HTTP_PROXY", "HTTPS_PROXY", "all_proxy", "ALL_PROXY"] {
        std::env::remove_var(var);
    }
}

// ===== Config Commands =====

#[tauri::command]
pub fn config_list(db: tauri::State<'_, DbPool>) -> Vec<Connection> {
    let conn = db.lock().unwrap();
    ConfigManager::list(&conn)
}

#[tauri::command]
pub fn config_save(db: tauri::State<'_, DbPool>, connection: Connection) -> Result<(), String> {
    let conn = db.lock().unwrap();
    ConfigManager::save(&conn, &connection)
}

#[tauri::command]
pub fn config_delete(db: tauri::State<'_, DbPool>, id: &str) -> Result<(), String> {
    let conn = db.lock().unwrap();
    ConfigManager::delete(&conn, id)
}

#[tauri::command]
pub fn config_save_credentials(
    db: tauri::State<'_, DbPool>,
    id: String,
    username: String,
    auth_type: String,
    key_path: Option<String>,
    password: Option<String>,
    remember_me: bool,
) -> Result<(), String> {
    println!("Saving credentials: id={}, username={}, auth_type={}, key_path={:?}, password={:?}, remember_me={}", 
             id, username, auth_type, key_path, password.as_ref().map(|_| "***"), remember_me);
    let conn = db.lock().unwrap();
    ConfigManager::save_credentials(&conn, &id, &username, &auth_type, key_path.as_deref(), password.as_deref(), remember_me)
}

// ===== Settings Commands =====

#[tauri::command]
pub fn settings_load(db: tauri::State<'_, DbPool>) -> Settings {
    let conn = db.lock().unwrap();
    SettingsManager::load(&conn)
}

#[tauri::command]
pub fn settings_save(db: tauri::State<'_, DbPool>, settings: Settings) -> Result<(), String> {
    let conn = db.lock().unwrap();
    SettingsManager::save(&conn, &settings)
}

// ===== Favorites Commands =====

#[tauri::command]
pub fn favorites_list(db: tauri::State<'_, DbPool>) -> Vec<Favorite> {
    let conn = db.lock().unwrap();
    FavoritesManager::list(&conn)
}

#[tauri::command]
pub fn favorites_add(db: tauri::State<'_, DbPool>, favorite: Favorite) -> Result<(), String> {
    let conn = db.lock().unwrap();
    FavoritesManager::add(&conn, &favorite)
}

#[tauri::command]
pub fn favorites_remove(db: tauri::State<'_, DbPool>, path: &str) -> Result<(), String> {
    let conn = db.lock().unwrap();
    FavoritesManager::remove(&conn, path)
}
