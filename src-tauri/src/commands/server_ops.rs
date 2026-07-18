use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;
use crate::{DbPool, ssh, ssh::SshManager, server, db};
use server::*;

// ===== Site Commands =====

// ponytail: list sites directly from Nginx via SSH (10-min read cache)
#[tauri::command]
pub async fn server_list_sites(
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
            &conn, &host, &site.domain, now_ms,
        ) {
            site.created_at = created_at;
        }
    }
    sites.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(sites)
}

#[tauri::command]
pub async fn server_create_site(
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
pub async fn server_toggle_site(
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
pub async fn server_delete_site(
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
pub async fn server_update_site(
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
pub async fn server_update_site_full(
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
pub async fn server_save_site_config(
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
pub async fn server_set_hotlink_protection(
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
pub async fn server_set_reverse_proxy(
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
pub async fn server_list_php_versions(
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
pub async fn server_list_subdirs(
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
pub async fn server_setup_ssl(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    app: tauri::AppHandle,
    session_id: &str,
    domain: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::setup_ssl(&session, &cache, session_id, domain, &app).await
}

// ===== Monitor / Firewall / Software =====

#[tauri::command]
pub async fn server_get_monitor_data(
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
pub async fn server_firewall_list(
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
pub async fn server_firewall_add(
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
    cache.invalidate(session_id, &["firewall"]);
    result
}

#[tauri::command]
pub async fn server_firewall_remove(
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
pub async fn server_firewall_toggle(
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
pub async fn server_get_software_list(
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
pub async fn server_get_available_php_versions(
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
pub async fn server_get_available_mysql_versions(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<Vec<server::MysqlVariant>, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    server::get_available_mysql_versions(&session, &cache, session_id).await
}

#[tauri::command]
pub async fn server_get_removable_sources(
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
pub async fn server_remove_sources(
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
pub async fn server_clean_and_update_sources(
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
pub async fn server_add_source(
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
pub async fn server_software_action(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    db: tauri::State<'_, DbPool>,
    app: tauri::AppHandle,
    session_id: &str,
    software: &str,
    action: &str,
    options: &str,
    display_name: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    // ponytail: read command timeout from settings, default 30 min
    let timeout_mins: u64 = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        conn.query_row("SELECT value FROM settings WHERE key = 'command_timeout_minutes'", [], |r| r.get::<_, String>(0))
            .ok().and_then(|v| v.parse().ok()).unwrap_or(30)
    };
    let timeout_secs = timeout_mins * 60;
    let result = server::software_action(&session, &cache, session_id, software, action, options, display_name, &app, timeout_secs).await;
    cache.invalidate(session_id, &["software_list", "service_statuses", "lnmp_status", "docker_status"]);
    result
}

// ===== System Misc =====

#[tauri::command]
pub async fn server_reboot(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    force: bool,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    let result = server::reboot_server(&session, &cache, session_id, force).await;
    cache.clear_session(session_id);
    result
}

#[tauri::command]
pub async fn server_get_uptime(
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
pub async fn server_deploy_pubkey(
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
pub async fn server_get_ssh_auth_mode(
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
pub async fn server_set_ssh_auth_mode(
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
pub async fn server_get_bbr_status(
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
pub async fn server_set_bbr_status(
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
pub async fn server_get_site_logs(
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
pub async fn server_read_site_log(
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
pub async fn server_check_docker(
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
pub async fn server_install_docker(
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
pub async fn server_uninstall_docker(
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
pub async fn server_docker_container_list(
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
pub async fn server_docker_container_action(
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
pub async fn server_docker_container_remove(
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
pub async fn server_docker_container_logs(
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
pub async fn server_docker_image_list(
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
pub async fn server_docker_image_pull(
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
pub async fn server_docker_image_remove(
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
pub async fn server_docker_image_run(
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
pub async fn server_docker_get_mirror_config(
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
pub async fn server_docker_set_mirror_config(
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

// ===== Cache & Misc =====

// ponytail: explicit cache invalidation from frontend
#[tauri::command]
pub async fn server_cache_invalidate(
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
pub async fn custom_software_list(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    db: tauri::State<'_, DbPool>,
    session_id: &str,
) -> Result<Vec<SoftwareInfo>, String> {
    let mgr = ssh_mgr.lock().await;
    let host = mgr.get_host(session_id).unwrap_or_default();
    let session = mgr.get_session(session_id)?;
    drop(mgr);
    let entries = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        db::CustomSoftwareManager::list(&conn, &host)
    };
    if entries.is_empty() { return Ok(Vec::new()); }
    let packages: Vec<String> = entries.iter().map(|e| e.package_name.clone()).collect();
    let mut detected = server::detect_custom_software(&session, &packages).await?;
    for d in &mut detected {
        if let Some(entry) = entries.iter().find(|e| e.package_name == d.name) {
            d.display_name = entry.display_name.clone();
            d.category = entry.category.clone();
        }
    }
    Ok(detected)
}

#[tauri::command]
pub async fn custom_software_add(
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
pub async fn custom_software_remove(
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
pub async fn custom_software_action(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    db: tauri::State<'_, DbPool>,
    app: tauri::AppHandle,
    session_id: &str,
    package_name: &str,
    action: &str,
    display_name: &str,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    let timeout_mins: u64 = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        conn.query_row("SELECT value FROM settings WHERE key = 'command_timeout_minutes'", [], |r| r.get::<_, String>(0))
            .ok().and_then(|v| v.parse().ok()).unwrap_or(30)
    };
    let timeout_secs = timeout_mins * 60;
    server::custom_software_action(&session, &cache, session_id, package_name, action, display_name, &app, timeout_secs).await
}

#[tauri::command]
pub async fn server_check_installation(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<serde_json::Value, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    drop(mgr);
    // ponytail: check if login shell has active child processes (install script/tee)
    let (pid_out, _, _) = ssh::session_exec_with_output(
        &session,
        "test -f /tmp/leepanel-install.pid && pgrep -P $(cat /tmp/leepanel-install.pid) >/dev/null 2>&1 && test -f /tmp/leepanel-install.log && test \"$(find /tmp/leepanel-install.log -mmin -5 2>/dev/null)\" && echo RUNNING || (rm -f /tmp/leepanel-install.pid /tmp/leepanel-install.info; echo IDLE)",
        8,
    ).await?;
    let running = pid_out.trim().contains("RUNNING");
    let log = ssh::session_exec_with_output(&session, "cat /tmp/leepanel-install.log 2>/dev/null || true", 10)
        .await.map(|(out, _, _)| out).unwrap_or_default();
    let info = ssh::session_exec_with_output(&session, "cat /tmp/leepanel-install.info 2>/dev/null || true", 5)
        .await.map(|(out, _, _)| out.trim().to_string()).unwrap_or_default();
    let (action, display_name) = if let Some((a, s)) = info.split_once(':') {
        (a.to_string(), s.to_string())
    } else {
        (String::new(), String::new())
    };
    Ok(serde_json::json!({ "running": running, "log": log, "action": action, "displayName": display_name }))
}
