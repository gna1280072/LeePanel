mod config;
mod db;
mod server;
mod ssh;

use config::{ConfigManager, Connection, Favorite, FavoritesManager, Settings, SettingsManager};
use db::{FbDirCache, FbFavorites};
use rusqlite::Connection as SqliteConn;
use server::*;
use ssh::SshManager;
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex as AsyncMutex;

type DbPool = std::sync::Mutex<SqliteConn>;

// ===== SSH Commands =====

#[tauri::command]
async fn ssh_connect(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    app: tauri::AppHandle,
    config: serde_json::Value,
) -> Result<String, String> {
    let session_id = uuid::Uuid::new_v4().to_string();
    let host = config["host"].as_str().unwrap_or("").to_string();
    let port = config["port"].as_u64().unwrap_or(22) as u16;
    let username = config["username"].as_str().unwrap_or("").to_string();
    let password = config["password"].as_str().map(|s| s.to_string());
    let key_path = config["keyPath"].as_str().map(|s| s.to_string());
    // ponytail: network operations without lock — only acquire briefly to insert session
    let session = SshManager::do_connect(session_id.clone(), host, port, username, password, key_path, app.clone()).await?;
    let mgr = ssh_mgr.lock().await;
    mgr.insert_session(session_id.clone(), session, app);
    drop(mgr);
    Ok(session_id)
}

#[tauri::command]
async fn ssh_input(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    data: &str,
) -> Result<(), String> {
    // ponytail: extract session quickly, release lock before network operations
    let mgr = ssh_mgr.lock().await;
    let _session = mgr.get_session(session_id)?;
    drop(mgr);
    _session.input_tx.send(data.as_bytes().to_vec()).await.map_err(|_| "Failed to send input".to_string())
}

#[tauri::command]
async fn ssh_resize(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    cols: u32,
    rows: u32,
) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    let _session = mgr.get_session(session_id)?;
    drop(mgr);
    _session.resize_tx.send((cols, rows)).await.map_err(|_| "Failed to send resize".to_string())
}

#[tauri::command]
async fn ssh_disconnect(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<(), String> {
    // ponytail: timeout on lock acquisition — if another op holds the lock for 3s, force disconnect locally
    match tokio::time::timeout(std::time::Duration::from_secs(3), ssh_mgr.lock()).await {
        Ok(mgr) => {
            mgr.cache.clear_session(session_id);
            let session = mgr.get_session(session_id).ok();
            drop(mgr);
            if let Some(ref s) = session {
                ssh::session_disconnect(s).await.ok();
            }
            let mgr = ssh_mgr.lock().await;
            mgr.remove_session(session_id);
            drop(mgr);
            Ok(())
        }
        Err(_) => {
            eprintln!("ssh_disconnect: lock timeout, forcing session removal for {}", session_id);
            Ok(())
        }
    }
}

#[tauri::command]
async fn ssh_get_cwd(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    drop(mgr);
    ssh::session_open_channel_and_exec(&session, "pwd", 5).await
}

#[tauri::command]
async fn ssh_list_dir(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    path: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    drop(mgr);
    ssh::session_list_dir(&session, path).await
}

#[tauri::command]
async fn ssh_stat_file(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    path: &str,
) -> Result<serde_json::Value, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    drop(mgr);
    ssh::session_stat_file(&session, path).await
}

#[tauri::command]
async fn ssh_read_file(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    path: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    drop(mgr);
    ssh::session_read_file(&session, path).await
}

#[tauri::command]
async fn ssh_write_file(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    path: &str,
    content: &str,
) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    drop(mgr);
    ssh::session_write_file(&session, path, content).await
}

#[tauri::command]
async fn ssh_delete_file(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    path: &str,
    is_dir: bool,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    drop(mgr);
    ssh::session_delete_file(&session, path, is_dir).await
}

#[tauri::command]
async fn ssh_delete_files_batch(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    paths: Vec<String>,
    is_dir: bool,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    drop(mgr);
    ssh::session_delete_files_batch(&session, &paths, is_dir).await
}

#[tauri::command]
async fn ssh_create_dir(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    path: &str,
) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    drop(mgr);
    ssh::session_create_dir(&session, path).await
}

#[tauri::command]
async fn ssh_rename_file(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    old_path: &str,
    new_path: &str,
) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    drop(mgr);
    ssh::session_rename_file(&session, old_path, new_path).await
}

#[tauri::command]
async fn ssh_rename_files_batch(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    renames: Vec<(String, String)>, // (old_path, new_path)
) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    drop(mgr);
    ssh::session_rename_files_batch(&session, &renames).await
}

#[tauri::command]
async fn ssh_copy_files_batch(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    sources: Vec<String>,
    dest_dir: &str,
    is_move: bool,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    drop(mgr);
    ssh::session_copy_files_batch(&session, &sources, dest_dir, is_move).await
}

#[tauri::command]
async fn ssh_set_permissions_batch(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    paths: Vec<String>,
    mode: &str,
) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    drop(mgr);
    ssh::session_set_permissions_batch(&session, &paths, mode).await
}

#[tauri::command]
async fn ssh_copy_file(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    app: tauri::AppHandle,
    session_id: &str,
    src: &str,
    dst: &str,
) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    drop(mgr);
    ssh::session_copy_file(&session, session_id, src, dst, &app).await
}

#[tauri::command]
async fn ssh_copy_dir(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    app: tauri::AppHandle,
    session_id: &str,
    src: &str,
    dst: &str,
) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    drop(mgr);
    ssh::session_copy_dir(&session, session_id, src, dst, &app).await
}

#[tauri::command]
async fn ssh_set_permissions(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    path: &str,
    mode: &str,
) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    drop(mgr);
    ssh::session_set_permissions(&session, path, mode).await
}

#[tauri::command]
async fn ssh_check_space(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    path: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    drop(mgr);
    ssh::session_check_space(&session, path).await
}

#[tauri::command]
async fn ssh_upload(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    app: tauri::AppHandle,
    session_id: &str,
    remote_path: &str,
    data: Vec<u8>,
) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    mgr.upload(session_id, remote_path, &data, &app).await
}

#[tauri::command]
async fn ssh_upload_chunk(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    remote_path: &str,
    data: Vec<u8>,
    offset: u64,
) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    mgr.upload_chunk(session_id, remote_path, &data, offset).await
}

#[tauri::command]
async fn ssh_download_file(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    app: tauri::AppHandle,
    session_id: &str,
    url: &str,
    dest: &str,
) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    mgr.download_file(session_id, url, dest, &app).await
}

#[tauri::command]
async fn ssh_download_to_local(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    remote_path: &str,
    file_name: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    mgr.download_to_local(session_id, remote_path, file_name).await
}

#[tauri::command]
async fn ssh_save_as_local(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    app: tauri::AppHandle,
    session_id: &str,
    remote_path: &str,
    file_name: &str,
) -> Result<String, String> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel::<Option<tauri_plugin_dialog::FilePath>>();
    let dialog = app.dialog().file();
    dialog.set_file_name(file_name).save_file(move |path| {
        let _ = tx.send(path);
    });
    let local_path = match rx.await.map_err(|_| "Dialog cancelled")? {
        Some(p) => p,
        None => return Err("Save cancelled".to_string()),
    };
    let local_str = local_path.to_string();
    let mgr = ssh_mgr.lock().await;
    let bytes = mgr.read_file_bytes(session_id, remote_path).await?;
    std::fs::write(&local_str, &bytes)
        .map_err(|e| format!("Failed to write local file: {}", e))?;
    Ok(local_str)
}

#[tauri::command]
async fn ssh_compress(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    app: tauri::AppHandle,
    session_id: &str,
    paths: Vec<String>,
    output: &str,
    format: &str,
) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    mgr.compress(session_id, &paths, output, format, &app).await
}

#[tauri::command]
async fn ssh_extract(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    app: tauri::AppHandle,
    session_id: &str,
    archive_path: &str,
    dest_dir: &str,
) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    mgr.extract(session_id, archive_path, dest_dir, &app).await
}

#[tauri::command]
async fn ssh_reconnect(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<(), String> {
    // ponytail: reconnect modifies sessions map, needs mgr lock briefly for disconnect/connect
    let mgr = ssh_mgr.lock().await;
    mgr.reconnect(session_id).await
}

#[tauri::command]
fn ssh_generate_keypair(algorithm: &str) -> Result<server::SshKeyPair, String> {
    server::generate_ssh_keypair(algorithm)
}

#[tauri::command]
async fn save_key_to_local(
    app: tauri::AppHandle,
    content: &str,
    file_name: &str,
) -> Result<String, String> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel::<Option<tauri_plugin_dialog::FilePath>>();
    let dialog = app.dialog().file();
    dialog.set_file_name(file_name).save_file(move |path| {
        let _ = tx.send(path);
    });
    let local_path = match rx.await.map_err(|_| "Dialog cancelled")? {
        Some(p) => p,
        None => return Err("Save cancelled".to_string()),
    };
    let local_str = local_path.to_string();
    std::fs::write(&local_str, content)
        .map_err(|e| format!("Failed to write key: {}", e))?;
    // Set permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&local_str, std::fs::Permissions::from_mode(0o600));
    }
    Ok(local_str)
}

// ===== Config Commands =====

#[tauri::command]
fn config_list(db: tauri::State<'_, DbPool>) -> Vec<Connection> {
    let conn = db.lock().unwrap();
    ConfigManager::list(&conn)
}

#[tauri::command]
fn config_save(db: tauri::State<'_, DbPool>, connection: Connection) -> Result<(), String> {
    let conn = db.lock().unwrap();
    ConfigManager::save(&conn, &connection)
}

#[tauri::command]
fn config_delete(db: tauri::State<'_, DbPool>, id: &str) -> Result<(), String> {
    let conn = db.lock().unwrap();
    ConfigManager::delete(&conn, id)
}

#[tauri::command]
fn config_save_credentials(
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
fn settings_load(db: tauri::State<'_, DbPool>) -> Settings {
    let conn = db.lock().unwrap();
    SettingsManager::load(&conn)
}

#[tauri::command]
fn settings_save(db: tauri::State<'_, DbPool>, settings: Settings) -> Result<(), String> {
    let conn = db.lock().unwrap();
    SettingsManager::save(&conn, &settings)
}

// ===== Favorites Commands =====

#[tauri::command]
fn favorites_list(db: tauri::State<'_, DbPool>) -> Vec<Favorite> {
    let conn = db.lock().unwrap();
    FavoritesManager::list(&conn)
}

#[tauri::command]
fn favorites_add(db: tauri::State<'_, DbPool>, favorite: Favorite) -> Result<(), String> {
    let conn = db.lock().unwrap();
    FavoritesManager::add(&conn, &favorite)
}

#[tauri::command]
fn favorites_remove(db: tauri::State<'_, DbPool>, path: &str) -> Result<(), String> {
    let conn = db.lock().unwrap();
    FavoritesManager::remove(&conn, path)
}

// ===== Server Commands =====

#[tauri::command]
async fn server_get_system_info(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<SystemInfo, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::get_system_info(&session, &cache, session_id).await
}

#[tauri::command]
async fn server_get_service_statuses(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<Vec<ServiceStatus>, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::get_service_statuses(&session, &cache, session_id).await
}

#[tauri::command]
async fn server_get_service_info(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    service: &str,
) -> Result<ServiceInfo, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::get_service_info(&session, &cache, session_id, service).await
}

#[tauri::command]
async fn server_service_action(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    service: &str,
    action: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    let cmd = format!("systemctl {} {}", action, service);
    let (_, stderr, code) = ssh::session_exec_with_output(&session, &cmd, 30).await?;
    // ponytail: invalidate service/software cache after start/stop/restart
    cache.invalidate(session_id, &["service_statuses", "software_list"]);
    if code != 0 && !stderr.is_empty() {
        Err(format!("{} failed: {}", service, stderr.trim()))
    } else {
        Ok(format!("{} {} OK", service, action))
    }
}

#[tauri::command]
async fn server_read_remote_file(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    path: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::read_remote_file(&session, &cache, session_id, path).await
}

#[tauri::command]
async fn server_write_remote_file(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    path: &str,
    content: &str,
) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::write_remote_file(&session, &cache, session_id, path, content).await
}

#[tauri::command]
async fn server_get_log_lines(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    path: &str,
    lines: u32,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::get_log_lines(&session, &cache, session_id, path, lines).await
}

#[tauri::command]
async fn server_test_nginx_config(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<(bool, String), String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    drop(mgr);
    let (stdout, stderr, code) = ssh::session_exec_with_output(&session, "nginx -t 2>&1", 10).await?;
    let combined = format!("{}{}", stdout, stderr);
    Ok((code == 0, combined))
}

#[tauri::command]
async fn server_list_nginx_vhosts(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<Vec<String>, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::list_nginx_vhosts(&session, &cache, session_id).await
}

#[tauri::command]
async fn server_find_mysql_service(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<(String, ServiceInfo), String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::find_mysql_service(&session, &cache, session_id).await
}

#[tauri::command]
async fn server_find_php_service(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<(String, ServiceInfo), String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::find_php_service(&session, &cache, session_id).await
}

#[tauri::command]
async fn server_find_php_fpm_config(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<(String, String), String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::find_php_fpm_config(&session, &cache, session_id).await
}

#[tauri::command]
async fn server_mysql_processes(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::get_mysql_processes(&session, &cache, session_id).await
}

#[tauri::command]
async fn server_mysql_query(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    query: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::exec_mysql_query(&session, &cache, session_id, query).await
}

#[tauri::command]
async fn server_list_databases(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<Vec<server::DbInfo>, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::list_databases(&session, &cache, session_id).await
}

#[tauri::command]
async fn server_mysql_create_database(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    db_name: &str,
    db_user: &str,
    db_pass: &str,
    charset: &str,
    access_type: &str,
    allowed_ip: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::create_database(&session, &cache, session_id, db_name, db_user, db_pass, charset, access_type, allowed_ip).await
}

#[tauri::command]
async fn server_mysql_delete_database(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    db_name: &str,
    db_user: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::delete_database(&session, &cache, session_id, db_name, db_user).await
}

#[tauri::command]
async fn server_mysql_change_db_access(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    db_name: &str,
    db_user: &str,
    access_type: &str,
    allowed_ip: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::change_db_access(&session, &cache, session_id, db_name, db_user, access_type, allowed_ip).await
}

#[tauri::command]
async fn server_change_mysql_root_password(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    new_password: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::change_mysql_root_password(&session, &cache, session_id, new_password).await
}

#[tauri::command]
async fn server_change_db_user_password(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    db_user: &str,
    new_password: &str,
    access_type: &str,
    allowed_ip: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::change_db_user_password(&session, &cache, session_id, db_user, new_password, access_type, allowed_ip).await
}

#[tauri::command]
async fn server_save_db_remark(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    db_name: &str,
    remark: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::save_db_remark(&session, &cache, session_id, db_name, remark).await
}

#[tauri::command]
async fn server_get_db_remarks(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<Vec<(String, String)>, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::get_db_remarks(&session, &cache, session_id).await
}

// ===== Database Credentials Commands =====

#[tauri::command]
async fn server_save_db_credentials(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    db_name: &str,
    password: &str,
    access_type: &str,
    allowed_ip: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::save_db_credentials(&session, &cache, session_id, db_name, password, access_type, allowed_ip).await
}

#[tauri::command]
async fn server_get_db_credentials(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<Vec<crate::db::DbCredential>, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::get_db_credentials(&session, &cache, session_id).await
}

#[tauri::command]
async fn server_get_db_credential(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    db_name: &str,
) -> Result<Option<crate::db::DbCredential>, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::get_db_credential(&session, &cache, session_id, db_name).await
}

#[tauri::command]
async fn server_update_db_credential_password(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    db_name: &str,
    password: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::update_db_credential_password(&session, &cache, session_id, db_name, password).await
}

#[tauri::command]
async fn server_backup_database(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    db_name: &str,
    db_user: &str,
    db_password: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::backup_database(&session, &cache, session_id, db_name, db_user, db_password).await
}

#[tauri::command]
async fn server_list_db_backups(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    db_name: &str,
) -> Result<Vec<server::BackupInfo>, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::list_db_backups(&session, &cache, session_id, db_name).await
}

#[tauri::command]
async fn server_delete_db_backup(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    backup_filename: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::delete_db_backup(&session, &cache, session_id, backup_filename).await
}

#[tauri::command]
async fn server_download_db_backup(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    backup_filename: &str,
) -> Result<Vec<u8>, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::download_db_backup(&session, &cache, session_id, backup_filename).await
}

#[tauri::command]
async fn server_save_db_backup_to_local(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    app: tauri::AppHandle,
    session_id: &str,
    backup_filename: &str,
) -> Result<String, String> {
    use tauri_plugin_dialog::DialogExt;
    
    // Extract just the filename from full path if needed
    let file_name_only = backup_filename.split('/').last().unwrap_or(backup_filename);
    
    let (tx, rx) = tokio::sync::oneshot::channel::<Option<tauri_plugin_dialog::FilePath>>();
    let dialog = app.dialog().file();
    dialog.set_file_name(file_name_only).save_file(move |path| {
        let _ = tx.send(path);
    });
    
    let local_path = match rx.await.map_err(|_| "Save cancelled")? {
        Some(p) => p,
        None => return Err("Save cancelled".to_string()),
    };
    
    let local_str = local_path.to_string();
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    
    // Get backup content from server
    let bytes = server::download_db_backup(&session, &cache, session_id, backup_filename).await?;
    
    // Write to local filesystem
    std::fs::write(&local_str, &bytes)
        .map_err(|e| format!("Failed to write local file: {}", e))?;
    
    Ok(local_str)
}

#[tauri::command]
async fn server_import_database_from_file(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    db_name: &str,
    db_user: &str,
    db_password: &str,
    sql_content: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::import_database_from_file(&session, &cache, session_id, db_name, db_user, db_password, sql_content).await
}

#[tauri::command]
async fn server_import_database_from_backup(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    db_name: &str,
    db_user: &str,
    db_password: &str,
    backup_filename: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::import_database_from_backup(&session, &cache, session_id, db_name, db_user, db_password, backup_filename).await
}

// ===== Redis Commands =====

#[tauri::command]
async fn server_redis_check_status(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<bool, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::check_redis_status(&session, &cache, session_id).await
}

#[tauri::command]
async fn server_redis_get_version(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::get_redis_version(&session, &cache, session_id).await
}

#[tauri::command]
async fn server_redis_dbsize_all(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<Vec<server::RedisDbSize>, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::redis_dbsize_all(&session, &cache, session_id).await
}

#[tauri::command]
async fn server_redis_scan_keys(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    db_index: u8,
    pattern: &str,
    search_type: &str,
    cursor: usize,
    count: usize,
) -> Result<(Vec<server::RedisKeyInfo>, usize), String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::redis_scan_keys(&session, &cache, session_id, db_index, pattern, search_type, cursor, count).await
}

#[tauri::command]
async fn server_redis_set_key(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    db_index: u8,
    key: &str,
    value: &str,
    ttl: Option<i64>,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::redis_set_key(&session, &cache, session_id, db_index, key, value, ttl).await
}

#[tauri::command]
async fn server_redis_del_key(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    db_index: u8,
    keys: Vec<String>,
) -> Result<usize, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::redis_del_key(&session, &cache, session_id, db_index, &keys).await
}

#[tauri::command]
async fn server_redis_flushdb(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    db_index: u8,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::redis_flushdb(&session, &cache, session_id, db_index).await
}

#[tauri::command]
async fn server_redis_save_backup(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::redis_save_backup(&session, &cache, session_id).await
}

#[tauri::command]
async fn server_redis_list_backups(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<Vec<server::BackupInfo>, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::redis_list_backups(&session, &cache, session_id).await
}

#[tauri::command]
async fn server_check_lnmp(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<LnmpStatus, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::check_lnmp_status(&session, &cache, session_id).await
}

#[tauri::command]
async fn server_install_lnmp(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    app: tauri::AppHandle,
    session_id: &str,
    config: LnmpInstallConfig,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    let result = server::install_lnmp(&session, &cache, session_id, &config, &app).await;
    // ponytail: invalidate LNMP-related caches after install
    cache.invalidate(session_id, &[
        "lnmp_status", "service_statuses", "software_list", "php_versions", "docker_status",
    ]);
    result
}

// ponytail: list sites directly from Nginx via SSH (10-min read cache)
#[tauri::command]
async fn server_list_sites(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    db: tauri::State<'_, DbPool>,
    session_id: &str,
) -> Result<Vec<server::SiteInfo>, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    let host = mgr.get_host(session_id).unwrap_or_default();
    drop(mgr);
    let mut sites = server::list_sites(&session, &cache, session_id).await?;
    // ponytail: fix up creation times from DB after SSH call (db is not Send)
    let conn = db.lock().map_err(|e| e.to_string())?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    for site in &mut sites {
        if let Ok(created_at) = db::SiteMetadataManager::save_or_get_created_at(
            &conn,
            &host,
            &site.domain,
            now_ms,
        ) {
            site.created_at = created_at;
        }
    }
    sites.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(sites)
}

// ===== File Browser Favorites (SQLite) =====

#[tauri::command]
async fn fb_favorites_list(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    db: tauri::State<'_, DbPool>,
    session_id: &str,
) -> Result<Vec<String>, String> {
    let mgr = ssh_mgr.lock().await;
    let host = mgr.get_host(session_id).unwrap_or_default();
    let conn = db.lock().map_err(|e| e.to_string())?;
    Ok(FbFavorites::list(&conn, &host))
}

#[tauri::command]
async fn fb_favorites_add(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    db: tauri::State<'_, DbPool>,
    session_id: &str,
    path: &str,
) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    let host = mgr.get_host(session_id).unwrap_or_default();
    let conn = db.lock().map_err(|e| e.to_string())?;
    FbFavorites::add(&conn, &host, path)
}

#[tauri::command]
async fn fb_favorites_remove(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    db: tauri::State<'_, DbPool>,
    session_id: &str,
    path: &str,
) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    let host = mgr.get_host(session_id).unwrap_or_default();
    let conn = db.lock().map_err(|e| e.to_string())?;
    FbFavorites::remove(&conn, &host, path)
}

// ===== File Browser Directory Cache =====

#[tauri::command]
async fn fb_cache_get(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    db: tauri::State<'_, DbPool>,
    session_id: &str,
    path: &str,
) -> Result<Option<(String, i64)>, String> {
    let mgr = ssh_mgr.lock().await;
    let host = mgr.get_host(session_id).unwrap_or_default();
    let conn = db.lock().map_err(|e| e.to_string())?;
    // ponytail: check if cache is enabled
    let enabled: bool = conn
        .query_row("SELECT value FROM settings WHERE key = 'cache_enabled'", [], |r| r.get::<_, String>(0))
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(true);
    if !enabled { return Ok(None); }
    // ponytail: read ttl from settings, default 24h
    let ttl: u32 = conn
        .query_row("SELECT value FROM settings WHERE key = 'cache_ttl_hours'", [], |r| r.get::<_, String>(0))
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(24);
    Ok(FbDirCache::get(&conn, &host, path, ttl))
}

#[tauri::command]
async fn fb_cache_put(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    db: tauri::State<'_, DbPool>,
    session_id: &str,
    path: &str,
    data: &str,
    file_count: u32,
) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    let host = mgr.get_host(session_id).unwrap_or_default();
    let conn = db.lock().map_err(|e| e.to_string())?;
    // ponytail: check if cache is enabled
    let enabled: bool = conn
        .query_row("SELECT value FROM settings WHERE key = 'cache_enabled'", [], |r| r.get::<_, String>(0))
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(true);
    if !enabled { return Ok(()); }
    // ponytail: skip caching if file count exceeds limit
    let max: u32 = conn
        .query_row("SELECT value FROM settings WHERE key = 'cache_max_files'", [], |r| r.get::<_, String>(0))
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(500);
    if file_count > max { return Ok(()); }
    FbDirCache::put(&conn, &host, path, data)
}

// ponytail: touch cached_at without rewriting data
#[tauri::command]
async fn fb_cache_touch(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    db: tauri::State<'_, DbPool>,
    session_id: &str,
    path: &str,
) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    let host = mgr.get_host(session_id).unwrap_or_default();
    let conn = db.lock().map_err(|e| e.to_string())?;
    // ponytail: check if cache is enabled
    let enabled: bool = conn
        .query_row("SELECT value FROM settings WHERE key = 'cache_enabled'", [], |r| r.get::<_, String>(0))
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(true);
    if !enabled { return Ok(()); }
    FbDirCache::touch(&conn, &host, path)
}

// ponytail: clear all directory cache
#[tauri::command]
async fn fb_cache_clear_all(
    db: tauri::State<'_, DbPool>,
) -> Result<u32, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    FbDirCache::clear_all(&conn)
}

// ponytail: count cached directories
#[tauri::command]
async fn fb_cache_count(
    db: tauri::State<'_, DbPool>,
) -> Result<u32, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    Ok(FbDirCache::count(&conn))
}

// ===== Generic UI State (reuses settings table) =====

#[tauri::command]
fn ui_state_get(
    db: tauri::State<'_, DbPool>,
    key: &str,
) -> Result<String, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    conn.query_row("SELECT value FROM settings WHERE key = ?1", rusqlite::params![key], |row| row.get(0))
        .map_err(|_| String::new()) // ponytail: return empty on not-found, simpler than Option
}

#[tauri::command]
fn ui_state_set(
    db: tauri::State<'_, DbPool>,
    key: &str,
    value: &str,
) -> Result<(), String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
        rusqlite::params![key, value],
    ).map_err(|e| format!("Failed to save UI state: {}", e))?;
    Ok(())
}

#[tauri::command]
async fn server_create_site(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    app: tauri::AppHandle,
    session_id: &str,
    domain: &str,
    root: &str,
    php_version: &str,
    running_dir: &str,
    open_basedir: bool,
    use_ssl: bool,
    create_db: bool,
    db_name: &str,
    db_user: &str,
    db_pass: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    let (_config_path, msg) = server::create_site(&session, &cache, session_id, domain, root, php_version, running_dir, open_basedir, use_ssl, create_db, db_name, db_user, db_pass, &app).await?;
    Ok(msg)
}

#[tauri::command]
async fn server_toggle_site(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    config_path: &str,
    domain: &str,
    enable: bool,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    let msg = server::toggle_site(&session, &cache, session_id, config_path, domain, enable).await?;
    Ok(msg)
}

#[tauri::command]
async fn server_delete_site(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    domain: &str,
    remove_files: bool,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    let msg = server::delete_site(&session, &cache, session_id, domain, remove_files).await?;
    Ok(msg)
}

#[tauri::command]
async fn server_update_site(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    old_domain: &str,
    new_domains: &str,
    new_root: &str,
    new_php_version: &str,
    index_files: &str,
    rewrite_rules: &str,
    config_path: &str,
    running_dir: &str,
    open_basedir: bool,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    let msg = server::update_site(&session, &cache, session_id, old_domain, new_domains, new_root, new_php_version, index_files, rewrite_rules, config_path, running_dir, open_basedir).await?;
    Ok(msg)
}

#[tauri::command]
async fn server_update_site_full(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    old_domain: &str,
    new_domains: &str,
    new_root: &str,
    new_php_version: &str,
    index_files: &str,
    rewrite_rules: &str,
    config_path: &str,
    running_dir: &str,
    open_basedir: bool,
    hotlink_enabled: bool,
    hotlink_extensions: &str,
    hotlink_allowed_domains: &str,
    hotlink_response: &str,
    hotlink_allow_empty_referer: bool,
    proxy_enabled: bool,
    proxy_path: &str,
    proxy_target: &str,
    proxy_websocket: bool,
    proxy_preserve_host: bool,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    let msg = server::update_site_full(
        &session, &cache, session_id, old_domain, new_domains, new_root,
        new_php_version, index_files, rewrite_rules, config_path,
        running_dir, open_basedir, hotlink_enabled, hotlink_extensions,
        hotlink_allowed_domains, hotlink_response, hotlink_allow_empty_referer,
        proxy_enabled, proxy_path, proxy_target, proxy_websocket, proxy_preserve_host
    ).await?;
    Ok(msg)
}

#[tauri::command]
async fn server_save_site_config(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    config_path: &str,
    config_content: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::save_site_config(&session, &cache, session_id, config_path, config_content).await
}

#[tauri::command]
async fn server_set_hotlink_protection(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    config_path: &str,
    enabled: bool,
    extensions: &str,
    allowed_domains: &str,
    response_code: &str,
    allow_empty_referer: bool,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::set_hotlink_protection(&session, &cache, session_id, config_path, enabled, extensions, allowed_domains, response_code, allow_empty_referer).await
}

#[tauri::command]
async fn server_set_reverse_proxy(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    config_path: &str,
    enabled: bool,
    proxy_path: &str,
    proxy_target: &str,
    websocket: bool,
    preserve_host: bool,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::set_reverse_proxy(&session, &cache, session_id, config_path, enabled, proxy_path, proxy_target, websocket, preserve_host).await
}

#[tauri::command]
async fn server_list_php_versions(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<Vec<String>, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::list_php_versions(&session, &cache, session_id).await
}

#[tauri::command]
async fn server_list_subdirs(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    path: &str,
) -> Result<Vec<String>, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::list_subdirs(&session, &cache, session_id, path).await
}

#[tauri::command]
async fn server_setup_ssl(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    app: tauri::AppHandle,
    session_id: &str,
    domain: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    let result = server::setup_ssl(&session, &cache, session_id, domain, &app).await;
    result
}

#[tauri::command]
async fn server_get_monitor_data(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<MonitorData, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::get_monitor_data(&session, &cache, session_id).await
}

#[tauri::command]
async fn server_firewall_list(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<FirewallInfo, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::get_firewall_rules(&session, &cache, session_id).await
}

#[tauri::command]
async fn server_firewall_add(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    port: &str,
    protocol: &str,
    action: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    let result = server::add_firewall_rule(&session, &cache, session_id, port, protocol, action).await;
    // ponytail: invalidate firewall cache after rule change
    cache.invalidate(session_id, &["firewall"]);
    result
}

#[tauri::command]
async fn server_firewall_remove(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    port: &str,
    protocol: &str,
    action: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    let result = server::remove_firewall_rule(&session, &cache, session_id, port, protocol, action).await;
    cache.invalidate(session_id, &["firewall"]);
    result
}

#[tauri::command]
async fn server_firewall_toggle(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    enable: bool,
) -> Result<FirewallToggleResult, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    let result = server::toggle_firewall(&session, &cache, session_id, enable).await;
    cache.invalidate(session_id, &["firewall"]);
    result
}

#[tauri::command]
async fn server_get_software_list(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<Vec<SoftwareInfo>, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::get_software_list(&session, &cache, session_id).await
}

#[tauri::command]
async fn server_get_available_php_versions(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<Vec<String>, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::get_available_php_versions(&session, &cache, session_id).await
}

#[tauri::command]
async fn server_get_removable_sources(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<Vec<String>, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::get_removable_sources(&session, &cache, session_id).await
}

#[tauri::command]
async fn server_remove_sources(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    source_names: Vec<String>,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::remove_sources(&session, &cache, session_id, source_names).await
}

#[tauri::command]
async fn server_clean_and_update_sources(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    app: tauri::AppHandle,
    session_id: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::clean_and_update_sources(&session, &cache, session_id, &app).await
}

#[tauri::command]
async fn server_add_source(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    name: &str,
    url: &str,
    gpg_key: Option<&str>,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::add_source(&session, &cache, session_id, name, url, gpg_key).await
}

#[tauri::command]
async fn server_software_action(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    db: tauri::State<'_, DbPool>,
    app: tauri::AppHandle,
    session_id: &str,
    software: &str,
    action: &str,
    options: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    // ponytail: read command timeout from settings, default 30 min
    let timeout_mins: u64 = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        conn.query_row("SELECT value FROM settings WHERE key = 'command_timeout_minutes'", [], |r| r.get::<_, String>(0))
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30)
    };
    let timeout_secs = timeout_mins * 60;
    let result = server::software_action(&session, &cache, session_id, software, action, options, &app, timeout_secs).await;
    // ponytail: invalidate software/service caches after install/uninstall
    cache.invalidate(session_id, &[
        "software_list", "service_statuses", "lnmp_status", "docker_status",
    ]);
    result
}

#[tauri::command]
async fn server_reboot(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    force: bool,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    let result = server::reboot_server(&session, &cache, session_id, force).await;
    // ponytail: clear all cache after reboot (everything is stale)
    cache.clear_session(session_id);
    result
}

#[tauri::command]
async fn server_get_uptime(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<(String, String), String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::get_server_uptime(&session, &cache, session_id).await
}

#[tauri::command]
async fn server_deploy_pubkey(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    pubkey: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::deploy_ssh_pubkey(&session, &cache, session_id, pubkey).await
}

#[tauri::command]
async fn server_get_ssh_auth_mode(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<SshAuthMode, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::get_ssh_auth_mode(&session, &cache, session_id).await
}

#[tauri::command]
async fn server_set_ssh_auth_mode(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    password_enabled: bool,
    pubkey_enabled: bool,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    let result = server::set_ssh_auth_mode(&session, &cache, session_id, password_enabled, pubkey_enabled).await;
    cache.invalidate(session_id, &["ssh_auth_mode"]);
    result
}

#[tauri::command]
async fn server_get_bbr_status(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<BbrStatus, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::get_bbr_status(&session, &cache, session_id).await
}

#[tauri::command]
async fn server_set_bbr_status(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    enable: bool,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    let result = server::set_bbr_status(&session, &cache, session_id, enable).await;
    cache.invalidate(session_id, &["bbr_status"]);
    result
}

#[tauri::command]
async fn server_get_site_logs(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    domain: &str,
) -> Result<Vec<SiteLogInfo>, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::get_site_logs(&session, &cache, session_id, domain).await
}

#[tauri::command]
async fn server_read_site_log(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    log_path: &str,
    lines: usize,
    date_from: Option<&str>,
    date_to: Option<&str>,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::read_site_log(&session, &cache, session_id, log_path, lines, date_from, date_to).await
}

// ===== Docker Commands =====

#[tauri::command]
async fn server_check_docker(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<server::DockerStatus, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::check_docker(&session, &cache, session_id).await
}

#[tauri::command]
async fn server_install_docker(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    app: tauri::AppHandle,
    session_id: &str,
    use_mirror: bool,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    let result = server::install_docker(&session, &cache, session_id, use_mirror, &app).await;
    cache.invalidate(session_id, &["docker_status", "software_list"]);
    result
}

#[tauri::command]
async fn server_uninstall_docker(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    app: tauri::AppHandle,
    session_id: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    let result = server::uninstall_docker(&session, &cache, session_id, &app).await;
    cache.invalidate(session_id, &["docker_status", "software_list"]);
    result
}

#[tauri::command]
async fn server_docker_container_list(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<Vec<server::DockerContainer>, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::docker_container_list(&session, &cache, session_id).await
}

#[tauri::command]
async fn server_docker_container_action(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    container_id: &str,
    action: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::docker_container_action(&session, &cache, session_id, container_id, action).await
}

#[tauri::command]
async fn server_docker_container_remove(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    container_id: &str,
    force: bool,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::docker_container_remove(&session, &cache, session_id, container_id, force).await
}

#[tauri::command]
async fn server_docker_container_logs(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    container_id: &str,
    lines: usize,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::docker_container_logs(&session, &cache, session_id, container_id, lines).await
}

#[tauri::command]
async fn server_docker_image_list(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<Vec<server::DockerImage>, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::docker_image_list(&session, &cache, session_id).await
}

#[tauri::command]
async fn server_docker_image_pull(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    app: tauri::AppHandle,
    session_id: &str,
    image_name: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::docker_image_pull(&session, &cache, session_id, image_name, &app).await
}

#[tauri::command]
async fn server_docker_image_remove(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    image_id: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::docker_image_remove(&session, &cache, session_id, image_id).await
}

#[tauri::command]
async fn server_docker_image_run(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    app: tauri::AppHandle,
    session_id: &str,
    image_name: &str,
    run_args: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::docker_image_run(&session, &cache, session_id, image_name, run_args, &app).await
}

#[tauri::command]
async fn server_docker_get_mirror_config(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<Vec<String>, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::docker_get_mirror_config(&session, &cache, session_id).await
}

#[tauri::command]
async fn server_docker_set_mirror_config(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    mirrors: Vec<String>,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::docker_set_mirror_config(&session, &cache, session_id, &mirrors).await
}

// ===== SSH Response Cache =====

// ponytail: explicit cache invalidation from frontend (e.g. after manual operations)
#[tauri::command]
async fn server_cache_invalidate(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    keys: Vec<String>,
) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    let key_refs: Vec<&str> = keys.iter().map(|s| s.as_str()).collect();
    mgr.cache.invalidate(session_id, &key_refs);
    Ok(())
}

// ===== Custom Software Commands =====

#[tauri::command]
async fn custom_software_list(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    db: tauri::State<'_, DbPool>,
    session_id: &str,
) -> Result<Vec<SoftwareInfo>, String> {
    let mgr = ssh_mgr.lock().await;
    let host = mgr.get_host(session_id).unwrap_or_default();
    let session = mgr.get_session(session_id)?;
    drop(mgr);
    // ponytail: read DB synchronously, drop guard before any .await
    let entries = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        db::CustomSoftwareManager::list(&conn, &host)
    };
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    let packages: Vec<String> = entries.iter().map(|e| e.package_name.clone()).collect();
    let mut detected = server::detect_custom_software(&session, &packages).await?;
    // ponytail: merge display_name and category from DB into detected results
    for d in &mut detected {
        if let Some(entry) = entries.iter().find(|e| e.package_name == d.name) {
            d.display_name = entry.display_name.clone();
            d.category = entry.category.clone();
        }
    }
    Ok(detected)
}

#[tauri::command]
async fn custom_software_add(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    db: tauri::State<'_, DbPool>,
    session_id: &str,
    package_name: &str,
    display_name: &str,
    category: &str,
) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    let host = mgr.get_host(session_id).unwrap_or_default();
    drop(mgr);
    let conn = db.lock().map_err(|e| e.to_string())?;
    db::CustomSoftwareManager::add(&conn, &host, package_name, display_name, category)
}

#[tauri::command]
async fn custom_software_remove(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    db: tauri::State<'_, DbPool>,
    session_id: &str,
    package_name: &str,
) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    let host = mgr.get_host(session_id).unwrap_or_default();
    drop(mgr);
    let conn = db.lock().map_err(|e| e.to_string())?;
    db::CustomSoftwareManager::remove(&conn, &host, package_name)
}

#[tauri::command]
async fn custom_software_action(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    db: tauri::State<'_, DbPool>,
    app: tauri::AppHandle,
    session_id: &str,
    package_name: &str,
    action: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    // ponytail: read command timeout from settings, default 30 min
    let timeout_mins: u64 = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        conn.query_row("SELECT value FROM settings WHERE key = 'command_timeout_minutes'", [], |r| r.get::<_, String>(0))
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30)
    };
    let timeout_secs = timeout_mins * 60;
    server::custom_software_action(&session, &cache, session_id, package_name, action, &app, timeout_secs).await
}

#[tauri::command]
async fn server_check_installation(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<serde_json::Value, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    drop(mgr);
    // ponytail: check if login shell has active child processes (install script/tee)
    // kill -0 alone is unreliable — stale PID may be reused by unrelated process
    let (pid_out, _, _) = ssh::session_exec_with_output(
        &session,
        "test -f /tmp/leepanel-install.pid && pgrep -P $(cat /tmp/leepanel-install.pid) >/dev/null 2>&1 && test -f /tmp/leepanel-install.log && test \"$(find /tmp/leepanel-install.log -mmin -5 2>/dev/null)\" && echo RUNNING || (rm -f /tmp/leepanel-install.pid /tmp/leepanel-install.info; echo IDLE)",
        8,
    ).await?;
    let running = pid_out.trim().contains("RUNNING");
    // ponytail: always read log (needed for final output when install just finished)
    let log = ssh::session_exec_with_output(&session, "cat /tmp/leepanel-install.log 2>/dev/null || true", 10)
        .await
        .map(|(out, _, _)| out)
        .unwrap_or_default();
    // ponytail: read action info for recovery label
    let info = ssh::session_exec_with_output(&session, "cat /tmp/leepanel-install.info 2>/dev/null || true", 5)
        .await
        .map(|(out, _, _)| out.trim().to_string())
        .unwrap_or_default();
    let (action, software) = if let Some((a, s)) = info.split_once(':') {
        (a.to_string(), s.to_string())
    } else {
        (String::new(), String::new())
    };
    Ok(serde_json::json!({ "running": running, "log": log, "action": action, "software": software }))
}

// ===== App Entry =====

pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir { file_name: None }),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Webview),
                ])
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let ssh_mgr = Arc::new(AsyncMutex::new(SshManager::new()));
            app.manage(ssh_mgr);

            // Initialize SQLite database
            let db = db::init_db().expect("Failed to initialize database");
            app.manage(db);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // SSH
            ssh_connect, ssh_input, ssh_resize, ssh_disconnect,
            ssh_get_cwd, ssh_list_dir, ssh_stat_file, ssh_read_file, ssh_write_file,
            ssh_delete_file, ssh_delete_files_batch, ssh_create_dir, ssh_rename_file, ssh_rename_files_batch,
            ssh_copy_file, ssh_copy_files_batch, ssh_copy_dir, ssh_set_permissions, ssh_set_permissions_batch,
            ssh_check_space, ssh_upload, ssh_upload_chunk, ssh_download_file,
            ssh_download_to_local, ssh_save_as_local,
            ssh_compress, ssh_extract, ssh_reconnect,
            ssh_generate_keypair, save_key_to_local,
            // Config
            config_list, config_save, config_delete, config_save_credentials,
            // Settings
            settings_load, settings_save,
            // Favorites
            favorites_list, favorites_add, favorites_remove,
            // Server
            server_get_system_info, server_get_service_statuses,
            server_get_service_info, server_service_action,
            server_read_remote_file, server_write_remote_file,
            server_get_log_lines, server_test_nginx_config,
            server_list_nginx_vhosts, server_find_mysql_service,
            server_find_php_service, server_find_php_fpm_config,
            server_mysql_processes, server_mysql_query,
            server_list_databases, server_mysql_create_database, server_mysql_delete_database,
            server_mysql_change_db_access,
            server_change_mysql_root_password,
            server_change_db_user_password,
            // Redis
            server_redis_check_status, server_redis_get_version,
            server_redis_dbsize_all, server_redis_scan_keys,
            server_redis_set_key, server_redis_del_key,
            server_redis_flushdb, server_redis_save_backup, server_redis_list_backups,
            server_check_lnmp, server_install_lnmp,
            server_list_sites, server_create_site,
            server_toggle_site,
            server_delete_site, server_update_site, server_update_site_full,
            server_save_site_config, server_set_hotlink_protection, server_set_reverse_proxy,
            server_list_php_versions, server_list_subdirs,
            server_setup_ssl, server_get_monitor_data,
            server_firewall_list, server_firewall_add,
            server_firewall_remove, server_firewall_toggle,
            server_get_software_list, server_get_available_php_versions, server_software_action,
            server_get_removable_sources, server_remove_sources, server_clean_and_update_sources, server_add_source,
            server_reboot, server_get_uptime,
            server_deploy_pubkey, server_get_ssh_auth_mode,
            server_set_ssh_auth_mode, server_get_bbr_status,
            server_set_bbr_status, server_get_site_logs,
            server_read_site_log,
            // File Browser
            fb_favorites_list, fb_favorites_add, fb_favorites_remove,
            fb_cache_get, fb_cache_put, fb_cache_touch, fb_cache_clear_all, fb_cache_count,
            ui_state_get, ui_state_set,
            // Docker
            server_check_docker, server_install_docker, server_uninstall_docker,
            server_docker_container_list, server_docker_container_action,
            server_docker_container_remove, server_docker_container_logs,
            server_docker_image_list, server_docker_image_pull, server_docker_image_remove, server_docker_image_run,
            server_docker_get_mirror_config, server_docker_set_mirror_config,
            // Cache
            server_cache_invalidate,
            // Database Remarks
            server_save_db_remark, server_get_db_remarks,
            // Database Credentials
            server_save_db_credentials, server_get_db_credentials,
            server_get_db_credential, server_update_db_credential_password,
            // Database Backup & Import
            server_backup_database, server_list_db_backups, server_delete_db_backup,
            server_download_db_backup, server_save_db_backup_to_local,
            server_import_database_from_file, server_import_database_from_backup,
            // Custom Software
            custom_software_list, custom_software_add, custom_software_remove, custom_software_action,
            server_check_installation,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
