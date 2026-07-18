use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;
use crate::{ssh, ssh::SshManager, server};
use server::*;

// ===== Server Commands =====

#[tauri::command]
pub async fn server_get_system_info(
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
pub async fn server_get_service_statuses(
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
pub async fn server_get_service_info(
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
pub async fn server_service_action(
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
pub async fn server_read_remote_file(
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
pub async fn server_write_remote_file(
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
pub async fn server_get_log_lines(
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
pub async fn server_test_nginx_config(
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
pub async fn server_list_nginx_vhosts(
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
pub async fn server_find_mysql_service(
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
pub async fn server_find_php_service(
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
pub async fn server_find_php_fpm_config(
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
pub async fn server_mysql_processes(
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
pub async fn server_mysql_query(
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
pub async fn server_list_databases(
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
pub async fn server_mysql_create_database(
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
pub async fn server_mysql_delete_database(
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
pub async fn server_mysql_clear_database(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    db_name: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::clear_database(&session, &cache, session_id, db_name).await
}

#[tauri::command]
pub async fn server_mysql_change_db_access(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    db_name: &str,
    db_user: &str,
    db_pass: &str,
    access_type: &str,
    allowed_ip: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::change_db_access(&session, &cache, session_id, db_name, db_user, db_pass, access_type, allowed_ip).await
}

#[tauri::command]
pub async fn server_change_mysql_root_password(
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
pub async fn server_change_db_user_password(
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
pub async fn server_save_db_remark(
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
pub async fn server_get_db_remarks(
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
pub async fn server_save_db_credentials(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    db_name: &str,
    db_user: &str,
    password: &str,
    access_type: &str,
    allowed_ip: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::save_db_credentials(&session, &cache, session_id, db_name, db_user, password, access_type, allowed_ip).await
}

#[tauri::command]
pub async fn server_get_db_credentials(
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
pub async fn server_get_db_credential(
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
pub async fn server_update_db_credential_password(
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
pub async fn server_backup_database(
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
pub async fn server_list_db_backups(
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
pub async fn server_delete_db_backup(
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
pub async fn server_download_db_backup(
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
pub async fn server_save_db_backup_to_local(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    app: tauri::AppHandle,
    session_id: &str,
    backup_filename: &str,
) -> Result<String, String> {
    use tauri_plugin_dialog::DialogExt;
    let file_name_only = backup_filename.split('/').last().unwrap_or(backup_filename);
    let (tx, rx) = tokio::sync::oneshot::channel::<Option<tauri_plugin_dialog::FilePath>>();
    let dialog = app.dialog().file();
    dialog.set_file_name(file_name_only).save_file(move |path| { let _ = tx.send(path); });
    let local_path = match rx.await.map_err(|_| "Save cancelled")? {
        Some(p) => p,
        None => return Err("Save cancelled".to_string()),
    };
    let local_str = local_path.to_string();
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    let bytes = server::download_db_backup(&session, &cache, session_id, backup_filename).await?;
    std::fs::write(&local_str, &bytes).map_err(|e| format!("Failed to write local file: {}", e))?;
    Ok(local_str)
}

#[tauri::command]
pub async fn server_import_database_from_file(
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
pub async fn server_import_database_from_file_bytes(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    db_name: &str,
    db_user: &str,
    db_password: &str,
    file_name: &str,
    file_bytes: Vec<u8>,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::import_database_from_file_bytes(&session, &cache, session_id, db_name, db_user, db_password, file_name, file_bytes).await
}

#[tauri::command]
pub async fn server_import_database_from_backup(
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
pub async fn server_redis_check_status(
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
pub async fn server_redis_get_version(
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
pub async fn server_redis_dbsize_all(
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
pub async fn server_redis_scan_keys(
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
pub async fn server_redis_set_key(
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
pub async fn server_redis_del_key(
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
pub async fn server_redis_flushdb(
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
pub async fn server_redis_save_backup(
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
pub async fn server_redis_list_backups(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<Vec<server::BackupInfo>, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::redis_list_backups(&session, &cache, session_id).await
}

// ===== LNMP =====

#[tauri::command]
pub async fn server_check_lnmp(
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
pub async fn server_install_lnmp(
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
    cache.invalidate(session_id, &[
        "lnmp_status", "service_statuses", "software_list", "php_versions", "docker_status",
    ]);
    result
}
