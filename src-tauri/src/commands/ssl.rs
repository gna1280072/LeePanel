//! SSL 自定义证书相关命令（上传 / 自签 / 移除）。
//!
//! 权限模型 v8：所有用户输入参数在进入后端逻辑前做白名单校验；
//! 证书/私钥 PEM 文本只用于落盘，不参与任何 shell 字符串拼接。

use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;

use crate::{ssh::SshManager, ssl_cert, DbPool};

fn validate_site_domains(domains: &[String]) -> Result<(), String> {
    for d in domains {
        // 允许通配符域名（*.example.com）作为 server_name
        let bare = d.trim().trim_start_matches("*.");
        crate::permissions::validate_domain(bare)?;
    }
    Ok(())
}

/// 站点主域名可能带通配符（`*.example.com`），校验时剥离前缀。
fn validate_site_domain(domain: &str) -> Result<(), String> {
    crate::permissions::validate_domain(domain.trim().trim_start_matches("*."))
}

#[tauri::command]
pub async fn server_ssl_site_infos(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    targets: Vec<ssl_cert::SslTarget>,
) -> Result<Vec<ssl_cert::SslCertInfo>, String> {
    for t in &targets {
        validate_site_domain(&t.domain)?;
        crate::permissions::validate_path(&t.config_path)?;
    }
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    drop(mgr);
    ssl_cert::list_infos(&session, &targets).await
}

#[tauri::command]
pub async fn server_ssl_precheck(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    domain: &str,
    cert_pem: &str,
    key_pem: &str,
    chain_pem: Option<String>,
    site_domains: Vec<String>,
) -> Result<ssl_cert::SslPrecheck, String> {
    validate_site_domain(domain)?;
    validate_site_domains(&site_domains)?;
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    drop(mgr);
    ssl_cert::precheck(
        &session,
        domain,
        cert_pem,
        key_pem,
        chain_pem.as_deref(),
        &site_domains,
    )
    .await
}

#[tauri::command]
pub async fn server_ssl_install_custom(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    db: tauri::State<'_, DbPool>,
    app: tauri::AppHandle,
    session_id: &str,
    domain: &str,
    config_path: &str,
    cert_pem: &str,
    key_pem: &str,
    chain_pem: Option<String>,
    stop_letsencrypt: bool,
    site_domains: Vec<String>,
) -> Result<String, String> {
    crate::permissions::validate_domain(domain)?;
    crate::permissions::validate_path(config_path)?;
    validate_site_domains(&site_domains)?;

    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    let info = session.connect_info.clone();
    drop(mgr);

    let result = ssl_cert::install_custom(
        &session,
        &cache,
        session_id,
        domain,
        config_path,
        cert_pem,
        key_pem,
        chain_pem.as_deref(),
        stop_letsencrypt,
        &site_domains,
        &app,
    )
    .await;

    if let Ok(conn) = db.lock() {
        crate::audit::audit_log(
            &conn,
            &info.host,
            &info.username,
            "ssl_install_custom",
            &format!("ssl install custom {}", domain),
            if result.is_ok() { "success" } else { "error" },
            &result.as_ref().err().cloned().unwrap_or_default(),
        );
    }
    result
}

#[tauri::command]
pub async fn server_ssl_generate_self_signed(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    db: tauri::State<'_, DbPool>,
    app: tauri::AppHandle,
    session_id: &str,
    domain: &str,
    config_path: &str,
    days: u32,
    site_domains: Vec<String>,
) -> Result<String, String> {
    crate::permissions::validate_domain(domain)?;
    crate::permissions::validate_path(config_path)?;
    validate_site_domains(&site_domains)?;

    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    let info = session.connect_info.clone();
    drop(mgr);

    let result = ssl_cert::generate_self_signed(
        &session,
        &cache,
        session_id,
        domain,
        config_path,
        days,
        &site_domains,
        &app,
    )
    .await;

    if let Ok(conn) = db.lock() {
        crate::audit::audit_log(
            &conn,
            &info.host,
            &info.username,
            "ssl_generate_self_signed",
            &format!("ssl generate self-signed {} ({}d)", domain, days),
            if result.is_ok() { "success" } else { "error" },
            &result.as_ref().err().cloned().unwrap_or_default(),
        );
    }
    result
}

#[tauri::command]
pub async fn server_ssl_remove_custom(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    db: tauri::State<'_, DbPool>,
    app: tauri::AppHandle,
    session_id: &str,
    domain: &str,
    config_path: &str,
) -> Result<String, String> {
    crate::permissions::validate_domain(domain)?;
    crate::permissions::validate_path(config_path)?;

    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    let info = session.connect_info.clone();
    drop(mgr);

    let result =
        ssl_cert::remove_custom(&session, &cache, session_id, domain, config_path, &app).await;

    if let Ok(conn) = db.lock() {
        crate::audit::audit_log(
            &conn,
            &info.host,
            &info.username,
            "ssl_remove_custom",
            &format!("ssl remove custom {}", domain),
            if result.is_ok() { "success" } else { "error" },
            &result.as_ref().err().cloned().unwrap_or_default(),
        );
    }
    result
}
