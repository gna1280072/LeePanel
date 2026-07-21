use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::Mutex as AsyncMutex;
use crate::ssh::{self, SshManager};
use crate::server;

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
pub async fn ssh_download_to_local(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, session_id: &str, remote_path: &str, file_name: &str) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    mgr.download_to_local(session_id, remote_path, file_name).await
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
    let mgr = ssh_mgr.lock().await;
    let bytes = mgr.read_file_bytes(session_id, remote_path).await?;
    std::fs::write(&local_str, &bytes).map_err(|e| format!("Failed to write local file: {}", e))?;
    Ok(local_str)
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
pub async fn ssh_reconnect(ssh_mgr: tauri::State<'_, Arc<AsyncMutex<SshManager>>>, session_id: &str) -> Result<(), String> {
    // ponytail: reconnect modifies sessions map, needs mgr lock briefly for disconnect/connect
    let mgr = ssh_mgr.lock().await;
    mgr.reconnect(session_id).await
}

#[tauri::command]
pub async fn ssh_generate_keypair(algorithm: String) -> Result<server::SshKeyPair, String> {
    tokio::task::spawn_blocking(move || server::generate_ssh_keypair(&algorithm))
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
