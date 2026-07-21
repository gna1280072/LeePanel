use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;
use crate::{DbPool, ssh::SshManager, db::{FbDirCache, FbFavorites}};

// ===== File Browser Favorites (SQLite) =====

#[tauri::command]
pub async fn fb_favorites_list(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, db: tauri::State<'_, DbPool>, session_id: &str) -> Result<Vec<String>, String> {
    let mgr = ssh_mgr.lock().await;
    let host = mgr.get_host(session_id).unwrap_or_default();
    let conn = db.lock().map_err(|e| e.to_string())?;
    Ok(FbFavorites::list(&conn, &host))
}

#[tauri::command]
pub async fn fb_favorites_add(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, db: tauri::State<'_, DbPool>, session_id: &str, path: &str) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    let host = mgr.get_host(session_id).unwrap_or_default();
    let conn = db.lock().map_err(|e| e.to_string())?;
    FbFavorites::add(&conn, &host, path)
}

#[tauri::command]
pub async fn fb_favorites_remove(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, db: tauri::State<'_, DbPool>, session_id: &str, path: &str) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    let host = mgr.get_host(session_id).unwrap_or_default();
    let conn = db.lock().map_err(|e| e.to_string())?;
    FbFavorites::remove(&conn, &host, path)
}

// ===== File Browser Directory Cache =====

#[tauri::command]
pub async fn fb_cache_get(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, db: tauri::State<'_, DbPool>, session_id: &str, path: &str) -> Result<Option<(String, i64)>, String> {
    let mgr = ssh_mgr.lock().await;
    let host = mgr.get_host(session_id).unwrap_or_default();
    let conn = db.lock().map_err(|e| e.to_string())?;
    // ponytail: check if cache is enabled
    let enabled: bool = conn.query_row("SELECT value FROM settings WHERE key = 'cache_enabled'", [], |r| r.get::<_, String>(0)).ok().and_then(|v| v.parse().ok()).unwrap_or(true);
    if !enabled { return Ok(None); }
    // ponytail: read ttl from settings, default 24h
    let ttl: u32 = conn.query_row("SELECT value FROM settings WHERE key = 'cache_ttl_hours'", [], |r| r.get::<_, String>(0)).ok().and_then(|v| v.parse().ok()).unwrap_or(24);
    Ok(FbDirCache::get(&conn, &host, path, ttl))
}

#[tauri::command]
pub async fn fb_cache_put(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, db: tauri::State<'_, DbPool>, session_id: &str, path: &str, data: &str, file_count: u32) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    let host = mgr.get_host(session_id).unwrap_or_default();
    let conn = db.lock().map_err(|e| e.to_string())?;
    let enabled: bool = conn.query_row("SELECT value FROM settings WHERE key = 'cache_enabled'", [], |r| r.get::<_, String>(0)).ok().and_then(|v| v.parse().ok()).unwrap_or(true);
    if !enabled { return Ok(()); }
    let max: u32 = conn.query_row("SELECT value FROM settings WHERE key = 'cache_max_files'", [], |r| r.get::<_, String>(0)).ok().and_then(|v| v.parse().ok()).unwrap_or(500);
    if file_count > max { return Ok(()); }
    FbDirCache::put(&conn, &host, path, data)
}

// ponytail: touch cached_at without rewriting data
#[tauri::command]
pub async fn fb_cache_touch(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, db: tauri::State<'_, DbPool>, session_id: &str, path: &str) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    let host = mgr.get_host(session_id).unwrap_or_default();
    let conn = db.lock().map_err(|e| e.to_string())?;
    let enabled: bool = conn.query_row("SELECT value FROM settings WHERE key = 'cache_enabled'", [], |r| r.get::<_, String>(0)).ok().and_then(|v| v.parse().ok()).unwrap_or(true);
    if !enabled { return Ok(()); }
    FbDirCache::touch(&conn, &host, path)
}

// ponytail: clear all directory cache
#[tauri::command]
pub async fn fb_cache_clear_all(db: tauri::State<'_, DbPool>) -> Result<u32, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    FbDirCache::clear_all(&conn)
}

// ponytail: count cached directories
#[tauri::command]
pub async fn fb_cache_count(db: tauri::State<'_, DbPool>) -> Result<u32, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    Ok(FbDirCache::count(&conn))
}

// ===== Generic UI State (reuses settings table) =====

#[tauri::command]
pub fn ui_state_get(db: tauri::State<'_, DbPool>, key: &str) -> Result<String, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    conn.query_row("SELECT value FROM settings WHERE key = ?1", rusqlite::params![key], |row| row.get(0))
        .map_err(|_| String::new()) // ponytail: return empty on not-found, simpler than Option
}

#[tauri::command]
pub fn ui_state_set(db: tauri::State<'_, DbPool>, key: &str, value: &str) -> Result<(), String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    conn.execute("INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)", rusqlite::params![key, value])
        .map_err(|e| format!("Failed to save UI state: {}", e))?;
    Ok(())
}
