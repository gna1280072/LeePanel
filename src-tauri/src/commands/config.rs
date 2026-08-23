use crate::{DbPool, config::{ConfigManager, Connection, Settings, SettingsManager, Favorite, FavoritesManager}, credentials::{self, CredKind}};

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
    // 1. 凭据 → 系统钥匙串（明文绝不落库）
    //    语义：None=保持不变 | Some("")=清空 | Some(值)=覆盖（新建连接均为"覆盖"）
    if connection.remember_me {
        if let Some(pw) = connection.password.as_deref() {
            if pw.is_empty() {
                credentials::store_delete_single(&connection.id, CredKind::Password)?;
            } else {
                credentials::store_set(&connection.id, CredKind::Password, pw)?;
            }
        }
        if let Some(pp) = connection.passphrase.as_deref() {
            if pp.is_empty() {
                credentials::store_delete_single(&connection.id, CredKind::Passphrase)?;
            } else {
                credentials::store_set(&connection.id, CredKind::Passphrase, pp)?;
            }
        }
    } else {
        // 取消"记住我" → 清理钥匙串残留（清理失败不阻塞保存，前端不再传 configId 不会误用）
        let _ = credentials::store_delete(&connection.id);
    }
    // 2. 计算最终标记（以钥匙串当前状态为准；读取失败按无凭据降级，不阻塞连接保存）
    let has_password = credentials::store_get(&connection.id, CredKind::Password).unwrap_or(None).is_some();
    let has_passphrase = credentials::store_get(&connection.id, CredKind::Passphrase).unwrap_or(None).is_some();
    // 3. DB 保存（仅元数据 + 标记，不含明文）
    let db_conn = Connection {
        password: None,
        passphrase: None,
        has_password: Some(has_password),
        has_passphrase: Some(has_passphrase),
        ..connection
    };
    let conn = db.lock().unwrap();
    ConfigManager::save(&conn, &db_conn)
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
    passphrase: Option<String>,
    remember_me: bool,
) -> Result<(), String> {
    // 凭据 → 系统钥匙串（明文绝不落库；非空才写入）
    if remember_me {
        if let Some(pw) = password.as_deref().filter(|p| !p.is_empty()) {
            credentials::store_set(&id, CredKind::Password, pw)?;
        }
        if let Some(pp) = passphrase.as_deref().filter(|p| !p.is_empty()) {
            credentials::store_set(&id, CredKind::Passphrase, pp)?;
        }
    } else {
        // 取消"记住我" → 清理钥匙串残留
        let _ = credentials::store_delete(&id);
    }
    let conn = db.lock().unwrap();
    ConfigManager::save_credentials(&conn, &id, &username, &auth_type, key_path.as_deref(), password.as_deref(), passphrase.as_deref(), remember_me)
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

// ===== Known Hosts Commands (SSH server identity, TOFU) =====

/// List all trusted SSH host keys (fingerprint store).
#[tauri::command]
pub fn known_hosts_list(db: tauri::State<'_, DbPool>) -> Vec<crate::db::KnownHost> {
    let conn = db.lock().unwrap();
    crate::db::KnownHostsManager::list(&conn)
}

/// Delete a trusted SSH host key (used after a legitimate server key rotation).
#[tauri::command]
pub fn known_hosts_delete(db: tauri::State<'_, DbPool>, host: String, key_type: String) -> Result<(), String> {
    let conn = db.lock().unwrap();
    crate::db::KnownHostsManager::delete(&conn, &host, &key_type)
}

// ===== Data Directory Commands =====

/// Get the local SQLite data directory (stores connections, settings, cache, etc.)
#[tauri::command]
pub fn get_data_dir() -> String {
    crate::db::db_dir().to_string_lossy().to_string()
}

/// Open the local SQLite data directory in the system file explorer
#[tauri::command]
pub fn open_data_dir() -> Result<(), String> {
    let dir = crate::db::db_dir();
    open::that(&dir).map_err(|e| format!("Failed to open directory: {}", e))
}
