use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::Mutex as AsyncMutex;
use crate::ssh::{self, SshManager};
use crate::server;
use crate::tunnel::TunnelManager;
use crate::HostKeyPending;

/// Resolve a pending first-contact host-key confirmation from the SSH handshake.
/// The frontend shows the fingerprint dialog and calls this with `trusted`.
#[tauri::command]
pub async fn ssh_confirm_host_key(
    pending: tauri::State<'_, HostKeyPending>,
    session_id: String,
    trusted: bool,
) -> Result<(), String> {
    let sender = pending
        .lock()
        .unwrap()
        .remove(&session_id)
        .ok_or_else(|| "No pending host key confirmation for this session".to_string())?;
    sender
        .send(trusted)
        .map_err(|_| "Host key confirmation channel closed".to_string())
}

#[tauri::command]
pub async fn ssh_connect(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    app: tauri::AppHandle,
    config: serde_json::Value,
) -> Result<String, String> {
    let session_id = uuid::Uuid::new_v4().to_string();
    let host = config["host"].as_str().unwrap_or("").to_string();
    let port = config["port"].as_u64().unwrap_or(22) as u16;
    let username = config["username"].as_str().unwrap_or("").to_string();
    // 会话级凭据：前端显式传入时优先（新建/编辑后未落钥匙串的临时凭据）
    let mut password = config["password"].as_str().map(|s| s.to_string())
        .filter(|p| !p.is_empty());
    let key_path = config["keyPath"].as_str().map(|s| s.to_string());
    // ponytail: empty passphrase means "no passphrase" — must be None, not Some("")
    let mut passphrase = config["passphrase"].as_str().map(|s| s.to_string())
        .filter(|p| !p.is_empty());
    // 已保存凭据：会话级未提供时，按 configId 从系统钥匙串读取（明文不进前端）
    if let Some(cid) = config["configId"].as_str() {
        if password.is_none() {
            password = crate::credentials::store_get(cid, crate::credentials::CredKind::Password)?;
        }
        if passphrase.is_none() {
            passphrase = crate::credentials::store_get(cid, crate::credentials::CredKind::Passphrase)?;
        }
    }
    let cols = config["cols"].as_u64().unwrap_or(80) as u32;
    let rows = config["rows"].as_u64().unwrap_or(24) as u32;
    // SSH 2FA（v9）：服务器标记 tfa_enabled → 认证走 keyboard-interactive；
    // tfaCode 为连接表单预填的 TOTP 验证码（不落库，仅本次连接使用）。
    let tfa_enabled = config["tfaEnabled"].as_bool().unwrap_or(false);
    let tfa_code = config["tfaCode"].as_str().map(|s| s.to_string())
        .filter(|c| !c.is_empty());
    // 权限模型 v8：连接模式 + sudo 密码（auth_mode='sudo' 且 sudo_password_mode='keyring' 时
    // 从系统钥匙串自动加载到会话缓存；ask 模式由前端在首次 sudo 时弹窗输入）
    let auth_mode = config["authMode"].as_str().unwrap_or("direct_root").to_string();
    let mut sudo_password: Option<String> = None;
    if auth_mode == "sudo" {
        if let Some(cid) = config["configId"].as_str() {
            if config["sudoPasswordMode"].as_str().unwrap_or("ask") == "keyring" {
                sudo_password = crate::credentials::store_get(cid, crate::credentials::CredKind::SudoPassword)?;
            }
        }
    }
    // ponytail: network operations without lock — only acquire briefly to insert session
    let session = SshManager::do_connect(session_id.clone(), host, port, username, password, key_path, passphrase, auth_mode, sudo_password, tfa_enabled, tfa_code, app.clone(), cols, rows).await?;
    let mgr = ssh_mgr.lock().await;
    mgr.insert_session(session_id.clone(), session, app);
    drop(mgr);
    Ok(session_id)
}

/// 权限模型 v8：设置会话级 sudo 密码（ask 模式弹窗输入后调用）。
/// `remember=true` 时同时写入系统钥匙串（需 config_id）。
#[tauri::command]
pub async fn ssh_set_sudo_password(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: String,
    password: String,
    config_id: Option<String>,
    remember: bool,
) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    mgr.set_sudo_password(&session_id, password, config_id, remember).await
}

/// 权限模型 v8：生成 `/etc/sudoers.d/leepanel` 白名单配置文本（用户粘贴到服务器部署）。
#[tauri::command]
pub fn ssh_generate_sudoers(username: String) -> String {
    crate::permissions::sudoers_config(&username)
}

#[tauri::command]
pub async fn ssh_input(
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
pub async fn ssh_resize(
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
pub async fn ssh_disconnect(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    tunnel_mgr: tauri::State<'_, Arc<AsyncMutex<TunnelManager>>>,
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
            // Close all tunnels for this session
            tunnel_mgr.lock().await.close_session_tunnels(session_id).await;
            Ok(())
        }
        Err(_) => {
            eprintln!("ssh_disconnect: lock timeout, forcing session removal for {}", session_id);
            Ok(())
        }
    }
}

#[tauri::command]
pub async fn ssh_get_cwd(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, session_id: &str) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await; let session = mgr.get_session(session_id)?; drop(mgr);
    ssh::session_open_channel_and_exec(&session, "pwd", 5).await
}

#[tauri::command]
pub async fn ssh_list_dir(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, session_id: &str, path: &str) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await; let session = mgr.get_session(session_id)?; drop(mgr);
    ssh::session_list_dir(&session, path).await
}

#[tauri::command]
pub async fn ssh_stat_file(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, session_id: &str, path: &str) -> Result<serde_json::Value, String> {
    let mgr = ssh_mgr.lock().await; let session = mgr.get_session(session_id)?; drop(mgr);
    ssh::session_stat_file(&session, path).await
}

#[tauri::command]
pub async fn ssh_read_file(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, session_id: &str, path: &str) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await; let session = mgr.get_session(session_id)?; drop(mgr);
    ssh::session_read_file(&session, path).await
}

#[tauri::command]
pub async fn ssh_write_file(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, session_id: &str, path: &str, content: &str) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await; let session = mgr.get_session(session_id)?; drop(mgr);
    ssh::session_write_file(&session, path, content).await
}

#[tauri::command]
pub async fn ssh_delete_file(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, session_id: &str, path: &str, is_dir: bool) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await; let session = mgr.get_session(session_id)?; drop(mgr);
    ssh::session_delete_file(&session, path, is_dir).await
}

#[tauri::command]
pub async fn ssh_delete_files_batch(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, session_id: &str, paths: Vec<String>, is_dir: bool) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await; let session = mgr.get_session(session_id)?; drop(mgr);
    ssh::session_delete_files_batch(&session, &paths, is_dir).await
}

#[tauri::command]
pub async fn ssh_create_dir(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, session_id: &str, path: &str) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await; let session = mgr.get_session(session_id)?; drop(mgr);
    ssh::session_create_dir(&session, path).await
}

#[tauri::command]
pub async fn ssh_rename_file(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, session_id: &str, old_path: &str, new_path: &str) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await; let session = mgr.get_session(session_id)?; drop(mgr);
    ssh::session_rename_file(&session, old_path, new_path).await
}

#[tauri::command]
pub async fn ssh_rename_files_batch(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, session_id: &str, renames: Vec<(String, String)>) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await; let session = mgr.get_session(session_id)?; drop(mgr);
    ssh::session_rename_files_batch(&session, &renames).await
}

#[tauri::command]
pub async fn ssh_copy_files_batch(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, session_id: &str, sources: Vec<String>, dest_dir: &str, is_move: bool) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await; let session = mgr.get_session(session_id)?; drop(mgr);
    ssh::session_copy_files_batch(&session, &sources, dest_dir, is_move).await
}

#[tauri::command]
pub async fn ssh_set_permissions_batch(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, session_id: &str, paths: Vec<String>, mode: &str) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await; let session = mgr.get_session(session_id)?; drop(mgr);
    ssh::session_set_permissions_batch(&session, &paths, mode).await
}

#[tauri::command]
pub async fn ssh_copy_file(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, app: tauri::AppHandle, session_id: &str, src: &str, dst: &str) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await; let session = mgr.get_session(session_id)?; drop(mgr);
    ssh::session_copy_file(&session, session_id, src, dst, &app).await
}

#[tauri::command]
pub async fn ssh_copy_dir(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, app: tauri::AppHandle, session_id: &str, src: &str, dst: &str) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await; let session = mgr.get_session(session_id)?; drop(mgr);
    ssh::session_copy_dir(&session, session_id, src, dst, &app).await
}

#[tauri::command]
pub async fn ssh_set_permissions(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, session_id: &str, path: &str, mode: &str) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await; let session = mgr.get_session(session_id)?; drop(mgr);
    ssh::session_set_permissions(&session, path, mode).await
}

#[tauri::command]
pub async fn ssh_check_space(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, session_id: &str, path: &str) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await; let session = mgr.get_session(session_id)?; drop(mgr);
    ssh::session_check_space(&session, path).await
}

#[tauri::command]
pub async fn ssh_upload(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, app: tauri::AppHandle, session_id: &str, remote_path: &str, data: Vec<u8>) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    mgr.upload(session_id, remote_path, &data, &app).await
}

#[tauri::command]
pub async fn ssh_upload_chunk(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, session_id: &str, remote_path: &str, data: Vec<u8>, offset: u64) -> Result<(), String> {
    // ponytail: release SshManager lock before I/O — uses cached SFTP session
    let session = { let mgr = ssh_mgr.lock().await; mgr.get_session(session_id)? };
    let sftp = ssh::session_open_sftp(&session).await?;
    use russh_sftp::protocol::OpenFlags;
    let mut file = if offset == 0 {
        sftp.create(remote_path).await
    } else {
        sftp.open_with_flags(remote_path, OpenFlags::APPEND | OpenFlags::WRITE).await
    }.map_err(|e| format!("Failed to open file: {}", e))?;
    use tokio::io::AsyncWriteExt;
    file.write_all(&data).await.map_err(|e| format!("Write failed: {}", e))?;
    file.shutdown().await.map_err(|e| format!("Failed to finalize: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn ssh_sftp_reset(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, session_id: &str) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    mgr.sftp_reset(session_id);
    Ok(())
}

#[tauri::command]
pub async fn ssh_upload_files_batch(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, app: tauri::AppHandle, session_id: &str, files: Vec<(String, Vec<u8>)>) -> Result<u32, String> {
    // ponytail: get session under lock, then release lock for all I/O
    let session = { let mgr = ssh_mgr.lock().await; mgr.get_session(session_id)? };
    let sftp = ssh::session_open_sftp(&session).await?;
    let semaphore = Arc::new(tokio::sync::Semaphore::new(3));
    let mut handles = vec![];
    for (remote_path, data) in files {
        let sftp = sftp.clone(); let sem = semaphore.clone(); let app = app.clone(); let sid = session_id.to_string();
        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            use tokio::io::AsyncWriteExt;
            let mut file = sftp.create(&remote_path).await.map_err(|e| format!("Failed to create {}: {}", remote_path, e))?;
            file.write_all(&data).await.map_err(|e| format!("Write failed for {}: {}", remote_path, e))?;
            file.shutdown().await.map_err(|e| format!("Finalize failed for {}: {}", remote_path, e))?;
            let _ = app.emit("upload-file-done", serde_json::json!({"sessionId": sid, "remotePath": remote_path}));
            Ok::<(), String>(())
        }));
    }
    let mut success = 0u32;
    for h in handles {
        match h.await {
            Ok(Ok(())) => success += 1,
            Ok(Err(e)) => { let _ = app.emit("upload-file-error", serde_json::json!({"error": e})); }
            Err(e) => { let _ = app.emit("upload-file-error", serde_json::json!({"error": e.to_string()})); }
        }
    }
    Ok(success)
}

#[tauri::command]
pub async fn ssh_create_dirs_batch(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, session_id: &str, paths: Vec<String>) -> Result<(), String> {
    // ponytail: release lock before SSH exec
    let session = { let mgr = ssh_mgr.lock().await; mgr.get_session(session_id)? };
    if paths.is_empty() { return Ok(()); }
    let escaped: Vec<String> = paths.iter().map(|p| format!("'{}'", p.replace('\'', "'\\''"))).collect();
    let cmd = format!("mkdir -p {}", escaped.join(" "));
    let (_, stderr, exit_code) = ssh::session_exec_with_output(&session, &cmd, 30).await?;
    if exit_code != 0 { return Err(format!("mkdir -p failed: {}", stderr)); }
    Ok(())
}

// ponytail: execute arbitrary SSH command — used for tar extraction after batch upload
#[tauri::command]
pub async fn ssh_exec(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, session_id: &str, command: &str) -> Result<(String, String, i32), String> {
    let session = { let mgr = ssh_mgr.lock().await; mgr.get_session(session_id)? };
    ssh::session_exec_with_output(&session, command, 60).await
}

#[tauri::command]
pub async fn ssh_download_file(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, app: tauri::AppHandle, session_id: &str, url: &str, dest: &str) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    mgr.download_file(session_id, url, dest, &app).await
}

#[tauri::command]
pub async fn ssh_download_to_local(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, app: tauri::AppHandle, session_id: &str, remote_path: &str, file_name: &str) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await; let session = mgr.get_session(session_id)?; drop(mgr);
    ssh::session_download_to_local(&session, remote_path, file_name, &app, session_id).await
}

#[tauri::command]
pub async fn ssh_save_as_local(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, app: tauri::AppHandle, session_id: &str, remote_path: &str, file_name: &str) -> Result<String, String> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel::<Option<tauri_plugin_dialog::FilePath>>();
    let dialog = app.dialog().file();
    dialog.set_file_name(file_name).save_file(move |path| { let _ = tx.send(path); });
    let local_path = match rx.await.map_err(|_| "Dialog cancelled")? {
        Some(p) => p,
        None => return Err("Save cancelled".to_string()),
    };
    let local_str = local_path.to_string();
    // ponytail: grab session then release lock — don't hold global mutex during transfer
    let mgr = ssh_mgr.lock().await; let session = mgr.get_session(session_id)?; drop(mgr);
    // Create transfer control for pause/stop support
    let ctrl = Arc::new(ssh::TransferControl { paused: std::sync::atomic::AtomicBool::new(false), stopped: std::sync::atomic::AtomicBool::new(false) });
    { let mgr = ssh_mgr.lock().await; *mgr.transfer_ctrl.lock().unwrap() = Some(ctrl.clone()); }
    let result = ssh::session_stream_file_to_local(&session, remote_path, &local_str, &app, session_id, ctrl).await;
    { let mgr = ssh_mgr.lock().await; *mgr.transfer_ctrl.lock().unwrap() = None; }
    result?;
    Ok(local_str)
}

#[tauri::command]
pub async fn ssh_save_pause(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    if let Some(ctrl) = mgr.transfer_ctrl.lock().unwrap().as_ref() {
        ctrl.paused.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    Ok(())
}

#[tauri::command]
pub async fn ssh_save_resume(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    if let Some(ctrl) = mgr.transfer_ctrl.lock().unwrap().as_ref() {
        ctrl.paused.store(false, std::sync::atomic::Ordering::Relaxed);
    }
    Ok(())
}

#[tauri::command]
pub async fn ssh_save_stop(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    if let Some(ctrl) = mgr.transfer_ctrl.lock().unwrap().as_ref() {
        ctrl.stopped.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    Ok(())
}

#[tauri::command]
pub async fn ssh_compress(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, app: tauri::AppHandle, session_id: &str, paths: Vec<String>, output: &str, format: &str) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    mgr.compress(session_id, &paths, output, format, &app).await
}

#[tauri::command]
pub async fn ssh_extract(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, app: tauri::AppHandle, session_id: &str, archive_path: &str, dest_dir: &str) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    mgr.extract(session_id, archive_path, dest_dir, &app).await
}

#[tauri::command]
pub async fn ssh_reconnect(
    ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>,
    tunnel_mgr: tauri::State<'_, Arc<AsyncMutex<TunnelManager>>>,
    app: tauri::AppHandle,
    session_id: &str,
) -> Result<(), String> {
    // Close all tunnels for this session (old connection is being dropped)
    tunnel_mgr.lock().await.close_session_tunnels(session_id).await;
    // ponytail: reconnect modifies sessions map, needs mgr lock briefly for disconnect/connect
    let mgr = ssh_mgr.lock().await;
    mgr.reconnect(session_id, app).await
}

#[tauri::command]
pub async fn ssh_generate_keypair(algorithm: String, passphrase: Option<String>) -> Result<server::SshKeyPair, String> {
    let alg = algorithm;
    let pp = passphrase;
    tokio::task::spawn_blocking(move || server::generate_ssh_keypair(&alg, pp.as_deref()))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

#[tauri::command]
pub async fn save_key_to_local(app: tauri::AppHandle, content: &str, file_name: &str) -> Result<String, String> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel::<Option<tauri_plugin_dialog::FilePath>>();
    let dialog = app.dialog().file();
    dialog.set_file_name(file_name).save_file(move |path| { let _ = tx.send(path); });
    let local_path = match rx.await.map_err(|_| "Dialog cancelled")? {
        Some(p) => p,
        None => return Err("Save cancelled".to_string()),
    };
    let local_str = local_path.to_string();
    std::fs::write(&local_str, content).map_err(|e| format!("Failed to write key: {}", e))?;
    #[cfg(unix)]
    { use std::os::unix::fs::PermissionsExt; let _ = std::fs::set_permissions(&local_str, std::fs::Permissions::from_mode(0o600)); }
    Ok(local_str)
}
