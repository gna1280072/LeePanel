use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;
use async_trait::async_trait;
use russh::client::{self, Handler};
use russh::ChannelMsg;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, Mutex};

// ===== SSH Response Cache =====

/// ponytail: in-memory cache for SSH responses, avoids redundant round-trips.
/// Connection-lifetime for static data, short TTL for semi-static data.
pub struct SshCache {
    entries: Mutex<HashMap<(String, String), (String, tokio::time::Instant)>>,
}

impl SshCache {
    pub fn new() -> Self {
        Self { entries: Mutex::new(HashMap::new()) }
    }

    pub async fn get(&self, session_id: &str, key: &str, ttl_secs: u64) -> Option<String> {
        let entries = self.entries.lock().await;
        if let Some((val, at)) = entries.get(&(session_id.to_string(), key.to_string())) {
            if ttl_secs == 0 || at.elapsed().as_secs() < ttl_secs {
                return Some(val.clone());
            }
        }
        None
    }

    pub async fn put(&self, session_id: &str, key: &str, value: String) {
        let mut entries = self.entries.lock().await;
        entries.insert(
            (session_id.to_string(), key.to_string()),
            (value, tokio::time::Instant::now()),
        );
    }

    pub async fn invalidate(&self, session_id: &str, keys: &[&str]) {
        let mut entries = self.entries.lock().await;
        for key in keys {
            entries.remove(&(session_id.to_string(), key.to_string()));
        }
    }

    pub async fn clear_session(&self, session_id: &str) {
        let mut entries = self.entries.lock().await;
        entries.retain(|(sid, _), _| sid != session_id);
    }
}

/// Parse curl -# progress bar output to extract percentage
fn parse_curl_progress(line: &str) -> Option<f64> {
    // curl -# outputs lines like: "### 45.2%" or "#=#=# 100%"
    // Look for percentage pattern
    if let Some(idx) = line.rfind('%') {
        let before = line[..idx].trim_end_matches(|c: char| !c.is_ascii_digit() && c != '.');
        if let Ok(pct) = before.parse::<f64>() {
            return Some(pct);
        }
    }
    None
}

pub struct SshHandler;

#[async_trait]
impl Handler for SshHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh_keys::key::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

#[derive(Clone)]
pub struct ConnectInfo {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: Option<String>,
    pub key_path: Option<String>,
}

struct ChannelOpen {
    reply: tokio::sync::oneshot::Sender<russh::Channel<client::Msg>>,
}

struct SshSession {
    handle: Arc<Mutex<client::Handle<SshHandler>>>,
    input_tx: mpsc::Sender<Vec<u8>>,
    resize_tx: mpsc::Sender<(u32, u32)>,
    channel_open_tx: mpsc::Sender<ChannelOpen>,
    connect_info: ConnectInfo,
    sftp_cache: tokio::sync::Mutex<Option<(Arc<russh_sftp::client::SftpSession>, tokio::time::Instant)>>, // SFTP session cache
}

pub struct SshManager {
    sessions: HashMap<String, SshSession>,
    app_handle: Option<AppHandle>,
    pub cache: SshCache,
}

impl SshManager {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            app_handle: None,
            cache: SshCache::new(),
        }
    }

    pub async fn connect(
        &mut self,
        session_id: String,
        host: String,
        port: u16,
        username: String,
        password: Option<String>,
        key_path: Option<String>,
        app_handle: AppHandle,
    ) -> Result<(), String> {
        let handler = SshHandler;
        let mut ssh_config = client::Config::default();
        // Detect dead connections via keepalive + inactivity timeout
        ssh_config.keepalive_interval = Some(std::time::Duration::from_secs(10));
        ssh_config.keepalive_max = 3;
        ssh_config.inactivity_timeout = Some(std::time::Duration::from_secs(60));
        let config = Arc::new(ssh_config);
        let addr_str = format!("{}:{}", host, port);
        let mut sh = client::connect(config, &addr_str, handler)
            .await
            .map_err(|e| format!("Connection failed: {}", e))?;

        // Authenticate
        if let Some(ref kp) = key_path {
            let key = russh_keys::load_secret_key(kp, None)
                .map_err(|e| format!("Failed to load key: {}", e))?;
            let auth_ok = sh.authenticate_publickey(&username, Arc::new(key))
                .await
                .map_err(|e| format!("Key auth error: {}", e))?;
            if !auth_ok {
                return Err("Key auth failed: server rejected the key".to_string());
            }
        } else if let Some(ref pw) = password {
            let auth_ok = sh.authenticate_password(&username, pw)
                .await
                .map_err(|e| format!("Password auth error: {}", e))?;
            if !auth_ok {
                return Err("Password auth failed: incorrect password".to_string());
            }
        } else {
            return Err("No authentication method provided".to_string());
        }

        let mut channel = sh
            .channel_open_session()
            .await
            .map_err(|e| format!("Failed to open session: {}", e))?;
        channel
            .request_pty(true, "xterm-256color", 80, 24, 0, 0, &[])
            .await
            .map_err(|e| format!("PTY request failed: {}", e))?;
        channel
            .request_shell(true)
            .await
            .map_err(|e| format!("Shell request failed: {}", e))?;

        let (input_tx, mut input_rx) = mpsc::channel::<Vec<u8>>(256);
        let (resize_tx, mut resize_rx) = mpsc::channel::<(u32, u32)>(32);
        let (channel_open_tx, handle_rx) = mpsc::channel::<ChannelOpen>(8);

        let handle = Arc::new(Mutex::new(sh));
        let handle_for_task = handle.clone();

        let sid = session_id.clone();
        let ah = app_handle.clone();

        // Background task: owns shell channel + handles channel open requests
        tokio::spawn(async move {
            let mut handle_rx: Option<mpsc::Receiver<ChannelOpen>> = Some(handle_rx);

            loop {
                tokio::select! {
                    msg = channel.wait() => {
                        match msg {
                            Some(ChannelMsg::Data { data }) => {
                                let text = String::from_utf8_lossy(&data).to_string();
                                let _ = ah.emit(
                                    "ssh-output",
                                    serde_json::json!({ "sessionId": sid, "data": text }),
                                );
                            }
                            Some(ChannelMsg::Close) | Some(ChannelMsg::Eof) | None => {
                                let _ = ah.emit("ssh-disconnected", serde_json::json!({
                                    "sessionId": sid,
                                    "reason": "Connection lost",
                                }));
                                break;
                            }
                            _ => {}
                        }
                    }
                    Some(data) = input_rx.recv() => {
                        if channel.data(&mut Cursor::new(&data)).await.is_err() {
                            let _ = ah.emit("ssh-disconnected", serde_json::json!({
                                "sessionId": sid,
                                "reason": "Send failed",
                            }));
                            break;
                        }
                    }
                    Some((cols, rows)) = resize_rx.recv() => {
                        let _ = channel.window_change(cols, rows, 0, 0).await;
                    }
                    Some(req) = async {
                        handle_rx.as_mut()?.recv().await
                    } => {
                        let h = handle_for_task.lock().await;
                        if let Ok(ch) = h.channel_open_session().await {
                            let _ = req.reply.send(ch);
                        }
                    }
                }
            }
        });

        let connect_info = ConnectInfo {
            host: host.clone(),
            port,
            username: username.clone(),
            password: password.clone(),
            key_path: key_path.clone(),
        };

        let session = SshSession {
            handle,
            input_tx,
            resize_tx,
            channel_open_tx,
            connect_info,
            sftp_cache: tokio::sync::Mutex::new(None), // Initialize empty cache
        };
        self.sessions.insert(session_id, session);
        self.app_handle = Some(app_handle);

        Ok(())
    }

    pub async fn input(&self, session_id: &str, data: &[u8]) -> Result<(), String> {
        if let Some(session) = self.sessions.get(session_id) {
            session
                .input_tx
                .send(data.to_vec())
                .await
                .map_err(|_| "Failed to send input".to_string())
        } else {
            Err("Session not found".to_string())
        }
    }

    pub async fn resize(&self, session_id: &str, cols: u32, rows: u32) -> Result<(), String> {
        if let Some(session) = self.sessions.get(session_id) {
            session
                .resize_tx
                .send((cols, rows))
                .await
                .map_err(|_| "Failed to send resize".to_string())
        } else {
            Err("Session not found".to_string())
        }
    }

    pub fn get_host(&self, session_id: &str) -> Option<String> {
        self.sessions.get(session_id).map(|s| s.connect_info.host.clone())
    }

    pub async fn get_cwd(&self, session_id: &str) -> Result<String, String> {
        let mut channel = self.open_channel(session_id).await?;

        // Execute pwd command
        channel
            .exec(true, "pwd")
            .await
            .map_err(|e| format!("Exec failed: {}", e))?;

        // Read output with timeout
        let mut output = String::new();
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
        loop {
            tokio::select! {
                msg = channel.wait() => {
                    match msg {
                        Some(ChannelMsg::Data { data }) => {
                            output.push_str(&String::from_utf8_lossy(&data));
                        }
                        Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                        _ => {}
                    }
                }
                _ = tokio::time::sleep_until(deadline) => {
                    break;
                }
            }
        }

        let cwd = output.trim().to_string();
        if cwd.is_empty() {
            Err("Empty pwd output".to_string())
        } else {
            Ok(cwd)
        }
    }

    pub async fn open_channel(&self, session_id: &str) -> Result<russh::Channel<client::Msg>, String> {
        let session = self.sessions.get(session_id).ok_or("Session not found")?;
        let (tx, rx) = tokio::sync::oneshot::channel();
        session.channel_open_tx
            .send(ChannelOpen { reply: tx })
            .await
            .map_err(|_| "Background task unavailable".to_string())?;
        rx.await.map_err(|_| "Failed to open channel".to_string())
    }

    /// Execute a command and collect stdout, stderr, and exit code
    pub async fn exec_with_output(
        &self,
        session_id: &str,
        cmd: &str,
        timeout_secs: u64,
    ) -> Result<(String, String, i32), String> {
        let mut channel = self.open_channel(session_id).await?;
        channel
            .exec(true, cmd)
            .await
            .map_err(|e| format!("Exec failed: {}", e))?;

        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut exit_code: i32 = -1;
        let deadline =
            tokio::time::Instant::now() + tokio::time::Duration::from_secs(timeout_secs);

        loop {
            tokio::select! {
                msg = channel.wait() => {
                    match msg {
                        Some(ChannelMsg::Data { data }) => {
                            stdout.push_str(&String::from_utf8_lossy(&data));
                        }
                        Some(ChannelMsg::ExtendedData { data, ext }) => {
                            if ext == 1 {
                                stderr.push_str(&String::from_utf8_lossy(&data));
                            }
                        }
                        Some(ChannelMsg::ExitStatus { exit_status }) => {
                            exit_code = exit_status as i32;
                        }
                        Some(ChannelMsg::Eof) => {
                            // Don't break yet - ExitStatus may arrive after Eof
                            // Wait for Close or timeout
                        }
                        Some(ChannelMsg::Close) | None => break,
                        _ => {}
                    }
                }
                _ = tokio::time::sleep_until(deadline) => {
                    return Err(format!("Command timed out after {}s", timeout_secs));
                }
            }
        }

        Ok((stdout, stderr, exit_code))
    }

    async fn open_sftp(&self, session_id: &str) -> Result<Arc<russh_sftp::client::SftpSession>, String> {
        let session = self.sessions.get(session_id).ok_or("Session not found")?;

        // Check cache
        {
            let cache = session.sftp_cache.lock().await;
            if let Some((sftp, created_at)) = cache.as_ref() {
                // Cache valid for 30 seconds
                if created_at.elapsed().as_secs() < 30 {
                    return Ok(sftp.clone());
                }
            }
        }

        // Create new SFTP session
        let channel = self.open_channel(session_id).await?;
        channel.request_subsystem(true, "sftp").await
            .map_err(|e| format!("SFTP subsystem request failed: {}", e))?;
        let stream = channel.into_stream();
        let config = russh_sftp::client::Config {
            max_packet_len: 64 * 1024,
            max_concurrent_writes: 8,
            request_timeout_secs: 60,
        };
        let sftp = russh_sftp::client::SftpSession::new_with_config(stream, config).await
            .map_err(|e| format!("SFTP init failed: {}", e))?;
        sftp.set_timeout(60);

        // Update cache
        {
            let mut cache = session.sftp_cache.lock().await;
            *cache = Some((Arc::new(sftp), tokio::time::Instant::now()));
        }

        // Return cloned Arc
        let cache = session.sftp_cache.lock().await;
        Ok(cache.as_ref().unwrap().0.clone())
    }

    pub async fn list_dir(&self, session_id: &str, path: &str) -> Result<String, String> {
        let sftp = self.open_sftp(session_id).await?;
        let entries = sftp.read_dir(path).await
            .map_err(|e| format!("Failed to read directory: {}", e))?;
        let mut files: Vec<serde_json::Value> = Vec::new();
        for entry in entries {
            let meta = entry.metadata();
            files.push(serde_json::json!({
                "name": entry.file_name(),
                "isDir": meta.is_dir(),
                "isSymlink": meta.is_symlink(),
                "size": meta.len(),
                "permissions": format!("{}", meta.permissions()),
                "mtime": meta.mtime.unwrap_or(0),
                "owner": meta.user.as_deref().unwrap_or(""),
            }));
        }
        // Don't close SFTP session - keep it alive for reuse via cache
        serde_json::to_string(&files).map_err(|e| format!("JSON error: {}", e))
    }

    pub async fn read_file(&self, session_id: &str, path: &str) -> Result<String, String> {
        let sftp = self.open_sftp(session_id).await?;
        use tokio::io::AsyncReadExt;
        let mut file = sftp.open(path).await
            .map_err(|e| format!("Failed to open file: {}", e))?;
        let mut content = Vec::new();
        file.read_to_end(&mut content).await
            .map_err(|e| format!("Failed to read file: {}", e))?;
        // Don't close SFTP session - keep it alive for reuse via cache
        if content.len() > 1024 * 1024 {
            Ok(String::from_utf8_lossy(&content[..1024 * 1024]).to_string())
        } else {
            Ok(String::from_utf8_lossy(&content).to_string())
        }
    }

    pub async fn write_file(&self, session_id: &str, path: &str, content: &str) -> Result<(), String> {
        let sftp = self.open_sftp(session_id).await?;
        use tokio::io::AsyncWriteExt;
        let mut file = sftp.create(path).await
            .map_err(|e| format!("Failed to create file: {}", e))?;
        file.write_all(content.as_bytes()).await
            .map_err(|e| format!("Failed to write file: {}", e))?;
        file.shutdown().await
            .map_err(|e| format!("Failed to flush file: {}", e))?;
        // Don't close SFTP session - keep it alive for reuse via cache
        Ok(())
    }

    pub async fn delete_file(&self, session_id: &str, path: &str, is_dir: bool) -> Result<String, String> {
        let cmd = if is_dir {
            format!("rm -rfv '{}'", path.replace('\'', "'\\''"))
        } else {
            format!("rm -fv '{}'", path.replace('\'', "'\\''"))
        };
        let (stdout, stderr, _) = self.exec_with_output(session_id, &cmd, 60).await?;
        Ok(format!("{}{}", stdout, stderr))
    }

    /// Batch delete multiple files/directories in a single command
    pub async fn delete_files_batch(
        &self,
        session_id: &str,
        paths: &[String],
        is_dir: bool,
    ) -> Result<String, String> {
        if paths.is_empty() {
            return Ok(String::new());
        }

        // Build rm command: rm -rfv file1 file2 file3 ...
        let escaped_paths: Vec<String> = paths
            .iter()
            .map(|p| p.replace('\'', "'\\''"))
            .collect();

        let cmd = if is_dir {
            format!("rm -rfv {}", escaped_paths.join(" "))
        } else {
            format!("rm -fv {}", escaped_paths.join(" "))
        };

        let (stdout, stderr, _) = self.exec_with_output(session_id, &cmd, 60).await?;
        Ok(format!("{}{}", stdout, stderr))
    }

    pub async fn create_dir(&self, session_id: &str, path: &str) -> Result<(), String> {
        let sftp = self.open_sftp(session_id).await?;
        sftp.create_dir(path).await
            .map_err(|e| format!("Failed to create directory: {}", e))?;
        // Don't close SFTP session - keep it alive for reuse via cache
        Ok(())
    }

    pub async fn rename_file(&self, session_id: &str, old_path: &str, new_path: &str) -> Result<(), String> {
        let sftp = self.open_sftp(session_id).await?;
        sftp.rename(old_path, new_path).await
            .map_err(|e| format!("Failed to rename: {}", e))?;
        // Don't close SFTP session - keep it alive for reuse via cache
        Ok(())
    }

    /// Batch rename multiple files using mv command
    pub async fn rename_files_batch(
        &self,
        session_id: &str,
        renames: &[(String, String)], // (old_path, new_path)
    ) -> Result<(), String> {
        if renames.is_empty() {
            return Ok(());
        }

        // Use mv command for each rename (SFTP rename doesn't support batch)
        for (old_path, new_path) in renames {
            let safe_old = old_path.replace('\'', "'\\''");
            let safe_new = new_path.replace('\'', "'\\''");
            let cmd = format!("mv '{}' '{}'", safe_old, safe_new);

            let (_, stderr, exit_code) = self.exec_with_output(session_id, &cmd, 10).await?;
            if exit_code != 0 {
                return Err(format!("Rename failed for {}: {}", old_path, stderr));
            }
        }

        Ok(())
    }

    /// Batch copy/move multiple files using cp/mv command
    pub async fn copy_files_batch(
        &self,
        session_id: &str,
        sources: &[String], // source paths
        dest_dir: &str,     // destination directory
        is_move: bool,      // true = mv, false = cp
    ) -> Result<String, String> {
        if sources.is_empty() {
            return Ok(String::new());
        }

        let escaped_sources: Vec<String> = sources
            .iter()
            .map(|s| format!("'{}'", s.replace('\'', "'\\''")))
            .collect();
        let safe_dest = dest_dir.replace('\'', "'\\''");

        let cmd = if is_move {
            // mv -v file1 file2 ... dir/
            format!("mv -v {} '{}'", escaped_sources.join(" "), safe_dest)
        } else {
            // cp -v file1 file2 ... dir/
            format!("cp -v {} '{}'", escaped_sources.join(" "), safe_dest)
        };

        let (stdout, stderr, _) = self.exec_with_output(session_id, &cmd, 60).await?;
        Ok(format!("{}{}", stdout, stderr))
    }

    pub async fn copy_file(&self, session_id: &str, src: &str, dst: &str, app_handle: &AppHandle) -> Result<(), String> {
        let mut channel = self.open_channel(session_id).await?;
        let safe_src = src.replace('\'', "'\\''");
        let safe_dst = dst.replace('\'', "'\\''");
        let cmd = format!("cp -v '{}' '{}' 2>&1", safe_src, safe_dst);

        let _ = app_handle.emit("copy-progress", serde_json::json!({
            "sessionId": session_id,
            "line": format!("$ {}", cmd),
            "status": "copying",
        }));

        channel
            .exec(true, cmd)
            .await
            .map_err(|e| format!("Exec failed: {}", e))?;

        let mut stderr = String::new();
        loop {
            match channel.wait().await {
                Some(ChannelMsg::Data { data }) => {
                    let text = String::from_utf8_lossy(&data);
                    for line in text.lines() {
                        if !line.trim().is_empty() {
                            let _ = app_handle.emit("copy-progress", serde_json::json!({
                                "sessionId": session_id,
                                "line": line,
                                "status": "copying",
                            }));
                        }
                    }
                }
                Some(ChannelMsg::ExtendedData { data, ext }) => {
                    if ext == 1 {
                        let text = String::from_utf8_lossy(&data);
                        stderr.push_str(&text);
                        for line in text.lines() {
                            if !line.trim().is_empty() {
                                let _ = app_handle.emit("copy-progress", serde_json::json!({
                                    "sessionId": session_id,
                                    "line": line,
                                    "status": "error",
                                }));
                            }
                        }
                    }
                }
                Some(ChannelMsg::ExitStatus { exit_status }) => {
                    if exit_status != 0 {
                        let err_msg = format!("cp failed (exit {}): {}", exit_status, stderr.trim());
                        let _ = app_handle.emit("copy-progress", serde_json::json!({
                            "sessionId": session_id,
                            "line": err_msg,
                            "status": "error",
                        }));
                        return Err(err_msg);
                    }
                    return Ok(());
                }
                Some(ChannelMsg::Eof) => {}
                None => return Err("Connection lost during copy".to_string()),
                _ => {}
            }
        }
    }

    pub async fn copy_dir(&self, session_id: &str, src: &str, dst: &str, app_handle: &AppHandle) -> Result<(), String> {
        let mut channel = self.open_channel(session_id).await?;
        let safe_src = src.replace('\'', "'\\''");
        let safe_dst = dst.replace('\'', "'\\''");
        // Use cp -rvT to copy directory contents directly (not into existing dir), verbose for progress
        let cmd = format!("cp -rvT '{}' '{}' 2>&1", safe_src, safe_dst);

        let _ = app_handle.emit("copy-progress", serde_json::json!({
            "sessionId": session_id,
            "line": format!("$ {}", cmd),
            "status": "copying",
        }));

        channel
            .exec(true, cmd)
            .await
            .map_err(|e| format!("Exec failed: {}", e))?;

        let mut stderr = String::new();
        loop {
            match channel.wait().await {
                Some(ChannelMsg::Data { data }) => {
                    let text = String::from_utf8_lossy(&data);
                    for line in text.lines() {
                        if !line.trim().is_empty() {
                            let _ = app_handle.emit("copy-progress", serde_json::json!({
                                "sessionId": session_id,
                                "line": line,
                                "status": "copying",
                            }));
                        }
                    }
                }
                Some(ChannelMsg::ExtendedData { data, ext }) => {
                    if ext == 1 {
                        let text = String::from_utf8_lossy(&data);
                        stderr.push_str(&text);
                        for line in text.lines() {
                            if !line.trim().is_empty() {
                                let _ = app_handle.emit("copy-progress", serde_json::json!({
                                    "sessionId": session_id,
                                    "line": line,
                                    "status": "error",
                                }));
                            }
                        }
                    }
                }
                Some(ChannelMsg::ExitStatus { exit_status }) => {
                    if exit_status != 0 {
                        let err_msg = format!("cp -r failed (exit {}): {}", exit_status, stderr.trim());
                        let _ = app_handle.emit("copy-progress", serde_json::json!({
                            "sessionId": session_id,
                            "line": err_msg,
                            "status": "error",
                        }));
                        return Err(err_msg);
                    }
                    return Ok(());
                }
                Some(ChannelMsg::Eof) => {}
                None => return Err("Connection lost during copy".to_string()),
                _ => {}
            }
        }
    }

    pub async fn set_permissions(&self, session_id: &str, path: &str, mode: &str) -> Result<(), String> {
        let mut channel = self.open_channel(session_id).await?;
        let cmd = format!("chmod {} '{}'", mode, path.replace('\'', "'\\''"));
        channel
            .exec(true, cmd)
            .await
            .map_err(|e| format!("Exec failed: {}", e))?;

        let mut stderr = String::new();
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
        loop {
            tokio::select! {
                msg = channel.wait() => {
                    match msg {
                        Some(ChannelMsg::ExtendedData { data, ext }) => {
                            if ext == 1 {
                                stderr.push_str(&String::from_utf8_lossy(&data));
                            }
                        }
                        Some(ChannelMsg::Data { .. }) | Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                        _ => {}
                    }
                }
                _ = tokio::time::sleep_until(deadline) => break,
            }
        }

        if stderr.is_empty() {
            Ok(())
        } else {
            Err(format!("chmod error: {}", stderr.trim()))
        }
    }

    /// Batch set permissions for multiple files using chmod command
    pub async fn set_permissions_batch(
        &self,
        session_id: &str,
        paths: &[String],
        mode: &str,
    ) -> Result<(), String> {
        if paths.is_empty() {
            return Ok(());
        }

        let escaped_paths: Vec<String> = paths
            .iter()
            .map(|p| format!("'{}'", p.replace('\'', "'\\''")))
            .collect();

        let cmd = format!("chmod {} {}", mode, escaped_paths.join(" "));

        let (_, stderr, exit_code) = self.exec_with_output(session_id, &cmd, 10).await?;
        if exit_code != 0 {
            return Err(format!("chmod error: {}", stderr.trim()));
        }
        Ok(())
    }

    /// Check disk space, write permission, and existing files in a directory
    pub async fn check_space(&self, session_id: &str, path: &str) -> Result<String, String> {
        let mut channel = self.open_channel(session_id).await?;
        let safe = path.replace('\'', "'\\''");
        // df -B1 gets available bytes; touch test checks write; ls lists files
        let cmd = format!(
            "df -B1 '{}' | tail -1 | awk '{{print $4}}'; echo '---'; touch '{}/.__wtest__' 2>&1 && rm '{}/.__wtest__' && echo 'OK' || echo 'DENIED'; echo '---'; ls -1 '{}'",
            safe, safe, safe, safe
        );
        channel.exec(true, cmd).await.map_err(|e| format!("Exec failed: {}", e))?;

        let mut output = String::new();
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
        loop {
            tokio::select! {
                msg = channel.wait() => {
                    match msg {
                        Some(ChannelMsg::Data { data }) => output.push_str(&String::from_utf8_lossy(&data)),
                        Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                        _ => {}
                    }
                }
                _ = tokio::time::sleep_until(deadline) => break,
            }
        }

        Ok(output.trim().to_string())
    }

    /// Compress files/folders into an archive on the remote server
    pub async fn compress(
        &self,
        session_id: &str,
        paths: &[String],
        output: &str,
        format: &str,
        app_handle: &AppHandle,
    ) -> Result<(), String> {
        let mut channel = self.open_channel(session_id).await?;

        let safe_output = output.replace('\'', "'\\' '");
        let safe_paths: Vec<String> = paths
            .iter()
            .map(|p| format!("'{}'", p.replace('\'', "'\\' '")))
            .collect();
        let paths_str = safe_paths.join(" ");

        let cmd = match format {
            "tar.gz" => format!("tar -czvf '{}' {} 2>&1", safe_output, paths_str),
            "zip" => format!("zip -r '{}' {} 2>&1", safe_output, paths_str),
            "tar.bz2" => format!("tar -cjvf '{}' {} 2>&1", safe_output, paths_str),
            _ => return Err(format!("Unsupported format: {}", format)),
        };

        channel
            .exec(true, cmd)
            .await
            .map_err(|e| format!("Exec failed: {}", e))?;

        let mut stderr = String::new();
        let deadline =
            tokio::time::Instant::now() + tokio::time::Duration::from_secs(300);
        loop {
            tokio::select! {
                msg = channel.wait() => {
                    match msg {
                        Some(ChannelMsg::Data { data }) => {
                            let text = String::from_utf8_lossy(&data);
                            stderr.push_str(&text);
                            for line in text.lines() {
                                if !line.trim().is_empty() {
                                    let _ = app_handle.emit("archive-progress", serde_json::json!({
                                        "sessionId": session_id,
                                        "line": line,
                                        "status": "compressing",
                                    }));
                                }
                            }
                        }
                        Some(ChannelMsg::ExtendedData { data, ext }) => {
                            if ext == 1 {
                                let text = String::from_utf8_lossy(&data);
                                stderr.push_str(&text);
                                for line in text.lines() {
                                    if !line.trim().is_empty() {
                                        let _ = app_handle.emit("archive-progress", serde_json::json!({
                                            "sessionId": session_id,
                                            "line": line,
                                            "status": "compressing",
                                        }));
                                    }
                                }
                            }
                        }
                        Some(ChannelMsg::ExitStatus { .. })
                        | Some(ChannelMsg::Eof)
                        | Some(ChannelMsg::Close)
                        | None => break,
                        _ => {}
                    }
                }
                _ = tokio::time::sleep_until(deadline) => {
                    return Err("Compress operation timed out".to_string());
                }
            }
        }

        // Emit completion
        let _ = app_handle.emit("archive-progress", serde_json::json!({
            "sessionId": session_id,
            "line": "Compression completed.",
            "status": "done",
        }));

        Ok(())
    }

    /// Extract an archive on the remote server
    pub async fn extract(
        &self,
        session_id: &str,
        archive_path: &str,
        dest_dir: &str,
        app_handle: &AppHandle,
    ) -> Result<(), String> {
        let mut channel = self.open_channel(session_id).await?;

        let safe_archive = archive_path.replace('\'', "'\\' '");
        let safe_dest = dest_dir.replace('\'', "'\\' '");

        // Detect format by extension and extract directly to destination
        let cmd = if archive_path.ends_with(".tar.gz") || archive_path.ends_with(".tgz") {
            format!("tar -xzvf '{}' -C '{}' 2>&1", safe_archive, safe_dest)
        } else if archive_path.ends_with(".tar.bz2") || archive_path.ends_with(".tbz2") {
            format!("tar -xjvf '{}' -C '{}' 2>&1", safe_archive, safe_dest)
        } else if archive_path.ends_with(".tar.xz") || archive_path.ends_with(".txz") {
            format!("tar -xJvf '{}' -C '{}' 2>&1", safe_archive, safe_dest)
        } else if archive_path.ends_with(".tar") {
            format!("tar -xvf '{}' -C '{}' 2>&1", safe_archive, safe_dest)
        } else if archive_path.ends_with(".zip") {
            format!("unzip -o '{}' -d '{}' 2>&1", safe_archive, safe_dest)
        } else {
            return Err(format!("Unsupported archive format: {}", archive_path));
        };

        // Execute extract command (tar/unzip will create dest dir if needed with -C/-d)
        channel
            .exec(true, cmd)
            .await
            .map_err(|e| format!("Exec failed: {}", e))?;

        let mut stderr = String::new();
        let mut exit_ok = true;
        let deadline =
            tokio::time::Instant::now() + tokio::time::Duration::from_secs(300);
        loop {
            tokio::select! {
                msg = channel.wait() => {
                    match msg {
                        Some(ChannelMsg::Data { data }) => {
                            let text = String::from_utf8_lossy(&data);
                            stderr.push_str(&text);
                            for line in text.lines() {
                                if !line.trim().is_empty() {
                                    let _ = app_handle.emit("archive-progress", serde_json::json!({
                                        "sessionId": session_id,
                                        "line": line,
                                        "status": "extracting",
                                    }));
                                }
                            }
                        }
                        Some(ChannelMsg::ExtendedData { data, ext }) => {
                            if ext == 1 {
                                let text = String::from_utf8_lossy(&data);
                                stderr.push_str(&text);
                                for line in text.lines() {
                                    if !line.trim().is_empty() {
                                        let _ = app_handle.emit("archive-progress", serde_json::json!({
                                            "sessionId": session_id,
                                            "line": line,
                                            "status": "extracting",
                                        }));
                                    }
                                }
                            }
                        }
                        Some(ChannelMsg::ExitStatus { exit_status }) => {
                            exit_ok = exit_status == 0;
                        }
                        Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                        _ => {}
                    }
                }
                _ = tokio::time::sleep_until(deadline) => {
                    return Err("Extract operation timed out".to_string());
                }
            }
        }

        // Check if extraction was successful
        if !exit_ok {
            return Err(format!("Extraction failed: {}", stderr.trim()));
        }

        // Log any output for debugging (tar -v outputs to stderr)
        if !stderr.trim().is_empty() {
            eprintln!("Extract output: {}", stderr.trim());
        }

        // Emit completion
        let _ = app_handle.emit("archive-progress", serde_json::json!({
            "sessionId": session_id,
            "line": "Extraction completed.",
            "status": "done",
        }));

        Ok(())
    }

    /// Download a file from URL to remote path using curl, emitting progress events
    pub async fn download_file(
        &self,
        session_id: &str,
        url: &str,
        dest: &str,
        app_handle: &AppHandle,
    ) -> Result<(), String> {
        let mut channel = self.open_channel(session_id).await?;
        let safe_dest = dest.replace('\'', "'\\''");
        let safe_url = url.replace('\'', "'\\''");
        // Use -f to fail on HTTP errors, -S to show errors even with -s/-#
        let cmd = format!(
            "curl -L -f -S -# -o '{}' '{}'",
            safe_dest, safe_url
        );
        channel.exec(true, cmd).await.map_err(|e| format!("Exec failed: {}", e))?;

        let mut stdout_buf = String::new();
        let mut stderr_buf = String::new();
        let mut exit_ok = true;
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(3600);
        loop {
            tokio::select! {
                msg = channel.wait() => {
                    match msg {
                        Some(ChannelMsg::Data { data }) => {
                            stdout_buf.push_str(&String::from_utf8_lossy(&data));
                        }
                        Some(ChannelMsg::ExtendedData { data, ext }) => {
                            if ext == 1 {
                                let chunk = String::from_utf8_lossy(&data);
                                stderr_buf.push_str(&chunk);
                                // curl -# outputs progress lines like: ## 45.2%
                                for line in chunk.split('\r') {
                                    let line = line.trim();
                                    if let Some(pct) = parse_curl_progress(line) {
                                        let _ = app_handle.emit("download-progress", serde_json::json!({
                                            "sessionId": session_id,
                                            "progress": pct,
                                            "status": "downloading",
                                        }));
                                    }
                                }
                            }
                        }
                        Some(ChannelMsg::ExitStatus { exit_status }) => {
                            exit_ok = exit_status == 0;
                        }
                        Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                        _ => {}
                    }
                }
                _ = tokio::time::sleep_until(deadline) => break,
            }
        }

        // Send 100% on success
        if exit_ok {
            let _ = app_handle.emit("download-progress", serde_json::json!({
                "sessionId": session_id,
                "progress": 100,
                "status": "done",
            }));
            Ok(())
        } else {
            // Combine stdout and stderr for better error reporting
            let full_error = format!("{}{}", stdout_buf.trim(), stderr_buf.trim());
            let _ = app_handle.emit("download-progress", serde_json::json!({
                "sessionId": session_id,
                "progress": 0,
                "status": "error",
                "error": full_error,
            }));
            Err(format!("Download failed: {}", full_error))
        }
    }

    pub async fn upload(
        &self,
        session_id: &str,
        remote_path: &str,
        data: &[u8],
        app_handle: &AppHandle,
    ) -> Result<(), String> {
        let channel = self.open_channel(session_id).await?;

        // Explicitly request SFTP subsystem
        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|e| format!("SFTP subsystem request failed: {}", e))?;

        // Convert channel to stream for SFTP
        let stream = channel.into_stream();

        // Create SFTP session with extended timeout
        let config = russh_sftp::client::Config {
            max_packet_len: 64 * 1024,
            max_concurrent_writes: 8,
            request_timeout_secs: 60,
        };
        let sftp = russh_sftp::client::SftpSession::new_with_config(stream, config)
            .await
            .map_err(|e| format!("SFTP init failed: {}", e))?;
        sftp.set_timeout(60);

        let total = data.len();
        let chunk_size = 32 * 1024; // 32KB chunks
        let mut sent: usize = 0;

        // Use create() + chunked write for progress reporting
        let mut file = sftp
            .create(remote_path)
            .await
            .map_err(|e| format!("Failed to create file: {}", e))?;

        use tokio::io::AsyncWriteExt;
        for chunk in data.chunks(chunk_size) {
            file.write_all(chunk)
                .await
                .map_err(|e| format!("Write failed: {}", e))?;
            sent += chunk.len();
            let pct = (sent * 100) / total;
            let _ = app_handle.emit(
                "upload-progress",
                serde_json::json!({
                    "sessionId": session_id,
                    "progress": pct,
                    "sent": sent,
                    "total": total,
                }),
            );
        }

        file.shutdown()
            .await
            .map_err(|e| format!("Failed to finalize: {}", e))?;
        // Don't close SFTP session - keep it alive for reuse via cache

        Ok(())
    }

    /// Write a single chunk at a given offset (for streaming upload)
    pub async fn upload_chunk(
        &self,
        session_id: &str,
        remote_path: &str,
        data: &[u8],
        offset: u64,
    ) -> Result<(), String> {
        let channel = self.open_channel(session_id).await?;
        channel.request_subsystem(true, "sftp").await
            .map_err(|e| format!("SFTP subsystem request failed: {}", e))?;
        let stream = channel.into_stream();
        let config = russh_sftp::client::Config {
            max_packet_len: 64 * 1024,
            max_concurrent_writes: 8,
            request_timeout_secs: 60,
        };
        let sftp = russh_sftp::client::SftpSession::new_with_config(stream, config)
            .await
            .map_err(|e| format!("SFTP init failed: {}", e))?;
        sftp.set_timeout(60);

        use russh_sftp::protocol::OpenFlags;
        let mut file = if offset == 0 {
            sftp.create(remote_path).await
        } else {
            sftp.open_with_flags(remote_path, OpenFlags::APPEND | OpenFlags::WRITE).await
        }.map_err(|e| format!("Failed to open file: {}", e))?;

        use tokio::io::AsyncWriteExt;
        file.write_all(data)
            .await
            .map_err(|e| format!("Write failed: {}", e))?;
        file.shutdown()
            .await
            .map_err(|e| format!("Failed to finalize: {}", e))?;
        // Don't close SFTP session - keep it alive for reuse via cache

        Ok(())
    }

    /// Read remote file as raw bytes (for save-as-local)
    pub async fn read_file_bytes(
        &self,
        session_id: &str,
        remote_path: &str,
    ) -> Result<Vec<u8>, String> {
        use tokio::io::AsyncReadExt;
        let sftp = self.open_sftp(session_id).await?;
        let mut file = sftp.open(remote_path).await
            .map_err(|e| format!("Failed to open remote file: {}", e))?;
        let mut content = Vec::new();
        file.read_to_end(&mut content).await
            .map_err(|e| format!("Failed to read remote file: {}", e))?;
        // Don't close SFTP session - keep it alive for reuse via cache
        Ok(content)
    }

    /// Download a remote file to local temp directory and open with default app
    pub async fn download_to_local(
        &self,
        session_id: &str,
        remote_path: &str,
        file_name: &str,
    ) -> Result<String, String> {
        use tokio::io::AsyncReadExt;
        let sftp = self.open_sftp(session_id).await?;
        let mut file = sftp.open(remote_path).await
            .map_err(|e| format!("Failed to open remote file: {}", e))?;
        let mut content = Vec::new();
        file.read_to_end(&mut content).await
            .map_err(|e| format!("Failed to read remote file: {}", e))?;
        // Don't close SFTP session - keep it alive for reuse via cache

        // Write to local temp directory
        let temp_dir = std::env::temp_dir().join("leepanel-preview");
        std::fs::create_dir_all(&temp_dir)
            .map_err(|e| format!("Failed to create temp dir: {}", e))?;
        let local_path = temp_dir.join(file_name);
        std::fs::write(&local_path, &content)
            .map_err(|e| format!("Failed to write local file: {}", e))?;

        let path_str = local_path.to_string_lossy().to_string();

        // Open with default application
        let _ = open::that(&local_path);

        Ok(path_str)
    }

    pub async fn disconnect(&mut self, session_id: &str) -> Result<(), String> {
        if let Some(session) = self.sessions.remove(session_id) {
            // Use timeout to avoid hanging on dead connections
            let h = session.handle.clone();
            let _ = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                let h = h.lock().await;
                let _ = h.disconnect(russh::Disconnect::ByApplication, "", "en").await;
            }).await;
        }
        Ok(())
    }

    pub fn get_connect_info(&self, session_id: &str) -> Option<ConnectInfo> {
        self.sessions.get(session_id).map(|s| s.connect_info.clone())
    }

    pub async fn reconnect(&mut self, session_id: &str) -> Result<(), String> {
        let info = self.get_connect_info(session_id).ok_or("Session not found")?;
        let app_handle = self.app_handle.clone().ok_or("App handle not available")?;
        // Disconnect old session with timeout (ignore errors - connection may be dead)
        self.disconnect(session_id).await.ok();
        // Connect with same credentials, with overall timeout
        let result = tokio::time::timeout(std::time::Duration::from_secs(30), self.connect(
            session_id.to_string(),
            info.host,
            info.port,
            info.username,
            info.password,
            info.key_path,
            app_handle,
        )).await;
        match result {
            Ok(r) => r,
            Err(_) => Err("Reconnect timed out (30s)".to_string()),
        }
    }
}
