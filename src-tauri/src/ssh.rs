use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use async_trait::async_trait;
use russh::client::{self, Handler};
use russh::ChannelMsg;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{mpsc, Mutex};

use crate::db::KnownHostsManager;
use crate::tunnel::TunnelManager;
use crate::{DbPool, HostKeyPending};

// ===== SSH Response Cache =====

/// ponytail: in-memory cache for SSH responses, avoids redundant round-trips.
/// Connection-lifetime for static data, short TTL for semi-static data.
/// ponytail: std::sync::Mutex — HashMap ops are instant, no need for async lock
pub struct SshCache {
    entries: std::sync::Mutex<HashMap<(String, String), (String, tokio::time::Instant)>>,
}

impl SshCache {
    pub fn new() -> Self {
        Self { entries: std::sync::Mutex::new(HashMap::new()) }
    }

    pub fn get(&self, session_id: &str, key: &str, ttl_secs: u64) -> Option<String> {
        let entries = self.entries.lock().unwrap();
        if let Some((val, at)) = entries.get(&(session_id.to_string(), key.to_string())) {
            if ttl_secs == 0 || at.elapsed().as_secs() < ttl_secs {
                return Some(val.clone());
            }
        }
        None
    }

    pub fn put(&self, session_id: &str, key: &str, value: String) {
        let mut entries = self.entries.lock().unwrap();
        entries.insert(
            (session_id.to_string(), key.to_string()),
            (value, tokio::time::Instant::now()),
        );
    }

    pub fn invalidate(&self, session_id: &str, keys: &[&str]) {
        let mut entries = self.entries.lock().unwrap();
        for key in keys {
            entries.remove(&(session_id.to_string(), key.to_string()));
        }
    }

    pub fn clear_session(&self, session_id: &str) {
        let mut entries = self.entries.lock().unwrap();
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

/// Detect whether a private key file is passphrase-encrypted by inspecting its header.
/// Covers both PEM ("Proc-Type: 4,ENCRYPTED" / "BEGIN ENCRYPTED PRIVATE KEY") and
/// OpenSSH formats ("BEGIN OPENSSH PRIVATE KEY" + ciphername != "none").
fn key_file_is_encrypted(path: &str) -> Result<bool, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read key file: {}", e))?;
    let head = &content[..content.len().min(8192)];
    // PEM (traditional / PKCS#8 encrypted) formats
    if head.contains("Proc-Type: 4,ENCRYPTED") || head.contains("BEGIN ENCRYPTED PRIVATE KEY") {
        return Ok(true);
    }
    // OpenSSH format: header lists the cipher; "none" means unencrypted
    if head.contains("BEGIN OPENSSH PRIVATE KEY") {
        if let Some(idx) = head.find("ciphername") {
            let rest = &head[idx + "ciphername".len()..];
            let trimmed = rest.trim_start_matches(|c: char| c == ':' || c == ' ' || c == '\t');
            let name: String = trimmed.chars().take_while(|c| !c.is_whitespace() && *c != '\n').collect();
            if !name.is_empty() && name != "none" {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// A connection the server forwarded to us (remote forwarding, ssh -R).
/// Handed to the matching remote tunnel via the forwarded_reg channel.
pub struct ForwardedTcpip {
    pub channel: russh::Channel<russh::client::Msg>,
}

pub struct SshHandler {
    /// Remote-forward registrations: server listen port -> tunnel receiver.
    /// The server names the port in server_channel_open_forwarded_tcpip.
    pub forwarded_reg: Arc<std::sync::Mutex<HashMap<u32, mpsc::UnboundedSender<ForwardedTcpip>>>>,
    /// Host key verifier (TOFU known_hosts). Always present for connections made via
    /// do_connect; `None` means verification is disabled → reject by default.
    pub host_key_verifier: Option<HostKeyVerifier>,
}

/// Host key verification state for a single connection attempt (TOFU known_hosts).
#[derive(Clone)]
pub struct HostKeyVerifier {
    pub app_handle: AppHandle,
    pub host: String,
    pub port: u16,
    pub session_id: String,
    /// session_id -> oneshot sender awaiting the user's trust decision (frontend callback).
    pub pending: HostKeyPending,
}

impl HostKeyVerifier {
    /// Verify the server host key against the app's known_hosts store.
    ///
    /// - Already trusted & fingerprint matches  → Ok(true), refresh last_seen
    /// - Known host but fingerprint changed      → emit `host-key-changed`, Ok(false) (hard reject)
    /// - First contact (TOFU)                    → emit `host-key-confirm`, await user decision
    pub async fn check(&self, key: &russh_keys::key::PublicKey) -> Result<bool, russh::Error> {
        let fingerprint = key.fingerprint(); // SHA256 base64 (no "SHA256:" prefix)
        let key_type = key.name().to_string();
        let host = self.host.clone();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let db = self.app_handle.state::<DbPool>();
        let conn = db.inner();

        let known = {
            let guard = conn.lock().unwrap();
            KnownHostsManager::find(&guard, &host, &key_type)
        };

        match known {
            Some(k) if k.fingerprint == fingerprint => {
                // ✅ Already trusted and unchanged — silent pass
                let guard = conn.lock().unwrap();
                let _ = KnownHostsManager::touch(&guard, &host, &key_type, now);
                Ok(true)
            }
            Some(_) => {
                // ❌ Key changed — possible MITM. Hard reject.
                let _ = self.app_handle.emit(
                    "host-key-changed",
                    serde_json::json!({
                        "sessionId": self.session_id,
                        "host": self.host,
                        "port": self.port,
                        "keyType": key_type,
                        "fingerprint": format!("SHA256:{}", fingerprint),
                    }),
                );
                Ok(false)
            }
            None => {
                // 🆕 First contact (TOFU) — ask the user to confirm the fingerprint
                let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
                self.pending.lock().unwrap().insert(self.session_id.clone(), tx);
                let _ = self.app_handle.emit(
                    "host-key-confirm",
                    serde_json::json!({
                        "sessionId": self.session_id,
                        "host": self.host,
                        "port": self.port,
                        "keyType": key_type,
                        "fingerprint": format!("SHA256:{}", fingerprint),
                    }),
                );
                match rx.await {
                    Ok(true) => {
                        // User trusted the key → persist it
                        let key_blob = {
                            use russh_keys::PublicKeyBase64;
                            key.public_key_base64()
                        };
                        let guard = conn.lock().unwrap();
                        let _ = KnownHostsManager::insert(&guard, &host, &key_type, &fingerprint, &key_blob, now);
                        Ok(true)
                    }
                    _ => {
                        // User rejected / dialog closed / channel dropped
                        self.pending.lock().unwrap().remove(&self.session_id);
                        Ok(false)
                    }
                }
            }
        }
    }
}

#[async_trait]
impl Handler for SshHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh_keys::key::PublicKey,
    ) -> Result<bool, Self::Error> {
        // TOFU known_hosts verification; no verifier → reject (secure default).
        match &self.host_key_verifier {
            Some(v) => v.check(server_public_key).await,
            None => Ok(false),
        }
    }

    /// Remote forwarding: the server opens a channel for a new incoming connection.
    /// Route it to the tunnel registered for this port; drop it if none.
    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: russh::Channel<russh::client::Msg>,
        _connected_address: &str,
        connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        if let Some(tx) = self.forwarded_reg.lock().unwrap().get(&connected_port) {
            let _ = tx.send(ForwardedTcpip { channel });
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct ConnectInfo {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: Option<String>,
    pub key_path: Option<String>,
    pub passphrase: Option<String>,
    pub cols: u32,
    pub rows: u32,
    /// 权限模型 v8：'direct_root'（root 直连）/ 'sudo'（普通用户 + sudo）。
    pub auth_mode: String,
}

struct ChannelOpen {
    reply: tokio::sync::oneshot::Sender<russh::Channel<client::Msg>>,
}

#[derive(Clone)]
pub struct SshSession {
    pub handle: Arc<Mutex<client::Handle<SshHandler>>>,
    pub input_tx: mpsc::Sender<Vec<u8>>,
    pub resize_tx: mpsc::Sender<(u32, u32)>,
    pub channel_open_tx: mpsc::Sender<ChannelOpen>,
    pub connect_info: ConnectInfo,
    pub sftp_cache: Arc<tokio::sync::Mutex<Option<(Arc<russh_sftp::client::SftpSession>, tokio::time::Instant)>>>,
    pub forwarded_reg: Arc<std::sync::Mutex<HashMap<u32, mpsc::UnboundedSender<ForwardedTcpip>>>>,
    /// 会话级 sudo 密码缓存（权限模型 v8）：
    /// - auth_mode='sudo' 且 sudo_password_mode='keyring'：连接时从系统钥匙串自动加载；
    /// - 否则为 None，首次 sudo 命令需要密码时由前端弹窗输入（ask 模式）；
    /// - 明文只在本进程内流转，随会话销毁即释放。
    pub sudo_password: Arc<tokio::sync::Mutex<Option<String>>>,
    /// 方案 A：连接生命周期内探测到的"免密 sudo 可用"标记（sudoers NOPASSWD 或
    /// 终端 sudo 凭证缓存 ts 有效）。true 时 sudo 命令走 `sudo -n` 免密路径，
    /// 不弹窗；ts 过期后 sudo -n 失败会自动重置为 false 并回退密码弹窗。
    pub sudo_nopass: Arc<std::sync::atomic::AtomicBool>,
}

/// Controls pause/stop for active file transfers (save-to-local).
pub struct TransferControl {
    pub paused: AtomicBool,
    pub stopped: AtomicBool,
}

pub struct SshManager {
    sessions: std::sync::RwLock<HashMap<String, SshSession>>,
    pub app_handle: Option<AppHandle>,
    pub cache: Arc<SshCache>,
    pub transfer_ctrl: std::sync::Mutex<Option<Arc<TransferControl>>>,
}

impl SshManager {
    pub fn new() -> Self {
        Self {
            sessions: std::sync::RwLock::new(HashMap::new()),
            app_handle: None,
            cache: Arc::new(SshCache::new()),
            transfer_ctrl: std::sync::Mutex::new(None),
        }
    }

    pub async fn connect(
        &self,
        session_id: String,
        host: String,
        port: u16,
        username: String,
        password: Option<String>,
        key_path: Option<String>,
        passphrase: Option<String>,
        auth_mode: String,
        sudo_password: Option<String>,
        app_handle: AppHandle,
        cols: u32,
        rows: u32,
    ) -> Result<(), String> {
        let session = Self::do_connect(session_id.clone(), host, port, username, password, key_path, passphrase, auth_mode, sudo_password, app_handle.clone(), cols, rows).await?;
        self.sessions.write().unwrap().insert(session_id, session);
        Ok(())
    }

    pub fn insert_session(&self, session_id: String, session: SshSession, _app_handle: AppHandle) {
        self.sessions.write().unwrap().insert(session_id, session);
    }

    // ponytail: sync session extraction — std RwLock, no await needed
    pub fn get_session(&self, session_id: &str) -> Result<SshSession, String> {
        self.sessions.read().unwrap()
            .get(session_id)
            .cloned()
            .ok_or_else(|| "Session not found".to_string())
    }

    pub fn get_host(&self, session_id: &str) -> Option<String> {
        self.sessions.read().unwrap()
            .get(session_id)
            .map(|s| s.connect_info.host.clone())
    }

    pub fn remove_session(&self, session_id: &str) -> Option<SshSession> {
        self.sessions.write().unwrap().remove(session_id)
    }

    // Network operations — no lock required
    pub async fn do_connect(
        session_id: String,
        host: String,
        port: u16,
        username: String,
        password: Option<String>,
        key_path: Option<String>,
        passphrase: Option<String>,
        auth_mode: String,
        sudo_password: Option<String>,
        app_handle: AppHandle,
        cols: u32,
        rows: u32,
    ) -> Result<SshSession, String> {
        // ponytail: normalize empty passphrase to None — russh-keys treats Some("") as
        // "try to decrypt", which misparses unencrypted PKCS#8 keys (DER tag error at byte 2)
        let passphrase = passphrase.filter(|p| !p.is_empty());
        let pending = app_handle.state::<HostKeyPending>().inner().clone();
        let handler = SshHandler {
            forwarded_reg: Arc::new(std::sync::Mutex::new(HashMap::new())),
            host_key_verifier: Some(HostKeyVerifier {
                app_handle: app_handle.clone(),
                host: host.clone(),
                port,
                session_id: session_id.clone(),
                pending,
            }),
        };
        let forwarded_reg = handler.forwarded_reg.clone();
        let mut ssh_config = client::Config::default();
        // Detect dead connections via keepalive + inactivity timeout
        ssh_config.keepalive_interval = Some(std::time::Duration::from_secs(10));
        ssh_config.keepalive_max = 3;
        ssh_config.inactivity_timeout = Some(std::time::Duration::from_secs(60));
        let config = Arc::new(ssh_config);
        let addr_str = format!("{}:{}", host, port);
        // Host key verification flow: 1) TCP connect 8s fast-fail; 2) SSH handshake up to 90s
        // (handshake may pause while the user confirms a first-contact host key fingerprint).
        let stream = tokio::time::timeout(
            std::time::Duration::from_secs(8),
            tokio::net::TcpStream::connect(&addr_str),
        )
        .await
        .map_err(|_| format!("Connection timeout: {}:{} unreachable", host, port))?
        .map_err(|e| format!("Connection failed: {}", e))?;

        let mut sh = tokio::time::timeout(
            std::time::Duration::from_secs(90),
            client::connect_stream(config, stream, handler),
        )
        .await
        .map_err(|_| format!("SSH handshake timeout: {}:{}", host, port))?
        .map_err(|e| format!("Connection failed: {}", e))?;

        // Authenticate
        if let Some(ref kp) = key_path {
            // Pre-check: if the key is passphrase-encrypted and no passphrase was provided,
            // fail fast with a clear message (instead of russh's raw "The key is encrypted")
            let key_encrypted = key_file_is_encrypted(kp)
                .map_err(|e| format!("Failed to read key file: {}", e))?;
            if passphrase.is_none() && key_encrypted {
                return Err("Key file is encrypted but no passphrase was provided".to_string());
            }
            let key = russh_keys::load_secret_key(kp, passphrase.as_deref())
                .map_err(|e| {
                    // The key file is intact and passphrase-encrypted, so a load failure with a
                    // passphrase supplied almost certainly means the passphrase is wrong —
                    // surface a friendly, unambiguous error instead of russh's raw message
                    if key_encrypted {
                        format!("Incorrect passphrase: failed to decrypt the key ({})", e)
                    } else {
                        format!("Failed to load key: {}", e)
                    }
                })?;
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
            .request_pty(true, "xterm-256color", cols, rows, 0, 0, &[])
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
                                // Close all tunnels for this session
                                if let Some(tm) = ah.try_state::<Arc<tokio::sync::Mutex<TunnelManager>>>() {
                                    tm.lock().await.close_session_tunnels(&sid).await;
                                }
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
                            if let Some(tm) = ah.try_state::<Arc<tokio::sync::Mutex<TunnelManager>>>() {
                                tm.lock().await.close_session_tunnels(&sid).await;
                            }
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
            passphrase: passphrase.clone(),
            cols,
            rows,
            auth_mode: auth_mode.clone(),
        };

        let session = SshSession {
            handle,
            input_tx,
            resize_tx,
            channel_open_tx,
            connect_info,
            sftp_cache: Arc::new(tokio::sync::Mutex::new(None)),
            forwarded_reg,
            sudo_password: Arc::new(tokio::sync::Mutex::new(sudo_password)),
            sudo_nopass: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };
        Ok(session)
    }

    pub async fn input(&self, session_id: &str, data: &[u8]) -> Result<(), String> {
        let session = self.get_session(session_id)?;
        session
            .input_tx
            .send(data.to_vec())
            .await
            .map_err(|_| "Failed to send input".to_string())
    }

    pub async fn resize(&self, session_id: &str, cols: u32, rows: u32) -> Result<(), String> {
        let session = self.get_session(session_id)?;
        session
            .resize_tx
            .send((cols, rows))
            .await
            .map_err(|_| "Failed to send resize".to_string())
    }

    pub async fn get_cwd(&self, session_id: &str) -> Result<String, String> {
        let session = self.get_session(session_id)?;
        session_open_channel_and_exec(&session, "pwd", 5).await
    }

    pub async fn open_channel(&self, session_id: &str) -> Result<russh::Channel<client::Msg>, String> {
        let session = self.get_session(session_id)?;
        session_open_channel(&session).await
    }

    pub async fn exec_with_output(
        &self,
        session_id: &str,
        cmd: &str,
        timeout_secs: u64,
    ) -> Result<(String, String, i32), String> {
        let session = self.get_session(session_id)?;
        session_exec_with_output(&session, cmd, timeout_secs).await
    }

    /// 权限模型 v8：设置会话级 sudo 密码（ask 模式弹窗输入后调用）。
    /// `remember=true` 且 config_id 存在时同时写入系统钥匙串（keyring 模式持久化）。
    pub async fn set_sudo_password(
        &self,
        session_id: &str,
        password: String,
        config_id: Option<String>,
        remember: bool,
    ) -> Result<(), String> {
        let session = self.get_session(session_id)?;
        {
            let mut slot = session.sudo_password.lock().await;
            *slot = Some(password.clone());
        }
        if remember {
            if let Some(cid) = config_id {
                crate::credentials::store_set(&cid, crate::credentials::CredKind::SudoPassword, &password)?;
            }
        }
        Ok(())
    }

    async fn open_sftp(&self, session_id: &str) -> Result<Arc<russh_sftp::client::SftpSession>, String> {
        let session = self.get_session(session_id)?;
        session_open_sftp(&session).await
    }

    /// ponytail: invalidate cached SFTP session so next open_sftp creates a fresh one
    pub fn sftp_reset(&self, session_id: &str) {
        if let Ok(session) = self.get_session(session_id) {
            if let Ok(mut cache) = session.sftp_cache.try_lock() {
                *cache = None;
            }
        }
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

    /// Check if a path exists and return its type (file/dir)
    pub async fn stat_file(&self, session_id: &str, path: &str) -> Result<serde_json::Value, String> {
        let sftp = self.open_sftp(session_id).await?;
        let meta = sftp.metadata(path).await
            .map_err(|e| format!("Path does not exist: {}", e))?;
        let is_dir = meta.is_dir();
        let is_symlink = meta.is_symlink();
        // If not dir and not symlink, it's a file
        let is_file = !is_dir && !is_symlink;
        Ok(serde_json::json!({
            "exists": true,
            "isDir": is_dir,
            "isFile": is_file,
            "isSymlink": is_symlink,
            "size": meta.len(),
        }))
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

        // Build rm command: rm -rfv 'file1' 'file2' 'file3' ...
        let escaped_paths: Vec<String> = paths
            .iter()
            .map(|p| format!("'{}'", p.replace('\'', "'\\''")))
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
        let safe_src = src.replace('\'', "'\\''");
        let safe_dst = dst.replace('\'', "'\\''");
        let cmd = format!("cp -v '{}' '{}' 2>&1", safe_src, safe_dst);

        let _ = app_handle.emit("copy-progress", serde_json::json!({
            "sessionId": session_id,
            "line": format!("$ {}", cmd),
            "status": "copying",
        }));

        // 权限模型 v8：统一 sudo 包装
        let session = self.get_session(session_id)?;
        let (mut channel, _) = session_exec_channel(&session, &cmd).await?;

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
        let safe_src = src.replace('\'', "'\\''");
        let safe_dst = dst.replace('\'', "'\\''");
        // Use cp -rvT to copy directory contents directly (not into existing dir), verbose for progress
        let cmd = format!("cp -rvT '{}' '{}' 2>&1", safe_src, safe_dst);

        let _ = app_handle.emit("copy-progress", serde_json::json!({
            "sessionId": session_id,
            "line": format!("$ {}", cmd),
            "status": "copying",
        }));

        // 权限模型 v8：统一 sudo 包装
        let session = self.get_session(session_id)?;
        let (mut channel, _) = session_exec_channel(&session, &cmd).await?;

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
        let cmd = format!("chmod {} '{}'", mode, path.replace('\'', "'\\''"));
        // 权限模型 v8：统一 sudo 包装
        let session = self.get_session(session_id)?;
        let (mut channel, _) = session_exec_channel(&session, &cmd).await?;

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
        // 权限模型 v8：附加第 4 段会话模式标记（SUDO_MODE / ROOT_MODE），
        // 供前端在"无写入权限"时给出针对提权模式的解释性提示
        let mode_flag = {
            let session = self.get_session(session_id).ok();
            match session {
                Some(s) if s.connect_info.auth_mode == "sudo" && s.connect_info.username != "root" => "SUDO_MODE",
                _ => "ROOT_MODE",
            }
        };
        // df -B1 gets available bytes; touch test checks write permission
        // find -printf '%f|%y' outputs filename|type directly (d=dir, f=file, l=link)
        let cmd = format!(
            "df -B1 '{}' | tail -1 | awk '{{print $4}}'; echo '---'; touch '{}/.__wtest__' 2>&1 && rm '{}/.__wtest__' && echo 'OK' || echo 'DENIED'; echo '---'; find '{}' -maxdepth 1 -mindepth 1 -printf '%f|%y\n' | grep -v '^\\.|'; echo '---'; echo '{}'",
            safe, safe, safe, safe, mode_flag
        );
        channel.exec(true, cmd).await.map_err(|e| format!("Exec failed: {}", e))?;

        let mut output = String::new();
        let mut stderr = String::new();
        let mut exit_code: Option<u32> = None;
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
        loop {
            tokio::select! {
                msg = channel.wait() => {
                    match msg {
                        Some(ChannelMsg::Data { data }) => {
                            output.push_str(&String::from_utf8_lossy(&data));
                        }
                        Some(ChannelMsg::ExtendedData { data, .. }) => {
                            stderr.push_str(&String::from_utf8_lossy(&data));
                        }
                        Some(ChannelMsg::ExitStatus { exit_status }) => {
                            exit_code = Some(exit_status);
                        }
                        Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) => break,
                        None => break,
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
        if paths.is_empty() {
            return Err("No paths to compress".to_string());
        }

        // Get the common parent directory and relative paths
        let first_path = &paths[0];
        let parent_dir = std::path::Path::new(first_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or(".".to_string());
        
        // Extract relative filenames from full paths
        let rel_names: Vec<String> = paths
            .iter()
            .map(|p| {
                std::path::Path::new(p)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| p.clone())
            })
            .collect();

        let safe_output = output.replace('\'', "'\\' '");
        let safe_parent = parent_dir.replace('\'', "'\\' '");
        let safe_names: Vec<String> = rel_names
            .iter()
            .map(|n| format!("'{}'", n.replace('\'', "'\\' '")))
            .collect();
        let names_str = safe_names.join(" ");

        // 权限模型 v8：tar 用 -C 单命令（sudo 前缀可直接作用于整条）；
        // zip 无 -C 等价，用 bash -c 包装成单命令（sudo -S bash -c "..." 整条以 root 执行）
        let cmd = match format {
            "tar.gz" => format!("tar -czvf '{}' -C '{}' {} 2>&1", safe_output, safe_parent, names_str),
            "zip" => format!("bash -c \"cd '{}' && zip -r '{}' {} 2>&1\"", safe_parent, safe_output, names_str),
            "tar.bz2" => format!("tar -cjvf '{}' -C '{}' {} 2>&1", safe_output, safe_parent, names_str),
            _ => return Err(format!("Unsupported format: {}", format)),
        };

        // 权限模型 v8：统一 sudo 包装
        let session = self.get_session(session_id)?;
        let (mut channel, _) = session_exec_channel(&session, &cmd).await?;

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
        // 权限模型 v8：统一 sudo 包装
        let session = self.get_session(session_id)?;
        let (mut channel, _) = session_exec_channel(&session, &cmd).await?;

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
        let safe_dest = dest.replace('\'', "'\\''");
        let safe_url = url.replace('\'', "'\\''");
        // Use -f to fail on HTTP errors, -S to show errors even with -s/-#
        let cmd = format!(
            "curl -L -f -S -# -o '{}' '{}'",
            safe_dest, safe_url
        );
        // 权限模型 v8：统一 sudo 包装（curl 写远程目标目录可能需要 root）
        let session = self.get_session(session_id)?;
        let (mut channel, _) = session_exec_channel(&session, &cmd).await?;

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
            request_timeout_secs: 15,
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
    /// ponytail: uses cached SFTP session — no new channel/subsystem per chunk
    pub async fn upload_chunk(
        &self,
        session_id: &str,
        remote_path: &str,
        data: &[u8],
        offset: u64,
    ) -> Result<(), String> {
        let sftp = self.open_sftp(session_id).await?;

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

        Ok(())
    }

    pub async fn disconnect(&self, session_id: &str) -> Result<(), String> {
        let session = self.sessions.write().unwrap().remove(session_id);
        if let Some(session) = session {
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
        self.sessions.read().unwrap().get(session_id).map(|s| s.connect_info.clone())
    }

    pub async fn reconnect(&self, session_id: &str, app_handle: AppHandle) -> Result<(), String> {
        let session = self.get_session(session_id).map_err(|_| "Session not found".to_string())?;
        let info = session.connect_info.clone();
        // 权限模型 v8：沿用会话级 sudo 密码缓存（无则 None，ask 模式由前端重新输入）
        let sudo_password = session.sudo_password.lock().await.clone();
        drop(session);
        // ponytail: use AppHandle from command context — self.app_handle is never initialised
        self.disconnect(session_id).await.ok();
        // 120s: TCP (8s) + handshake (90s) may pause on host-key confirmation
        let result = tokio::time::timeout(std::time::Duration::from_secs(120), self.connect(
            session_id.to_string(),
            info.host,
            info.port,
            info.username,
            info.password,
            info.key_path,
            info.passphrase,
            info.auth_mode,
            sudo_password,
            app_handle,
            info.cols,
            info.rows,
        )).await;
        match result {
            Ok(r) => r,
            Err(_) => Err("Reconnect timed out (120s)".to_string()),
        }
    }
}


pub async fn session_list_dir(session: &SshSession, path: &str) -> Result<String, String> {
    let sftp = session_open_sftp(session).await?;
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
    serde_json::to_string(&files).map_err(|e| format!("JSON error: {}", e))
}

pub async fn session_stat_file(session: &SshSession, path: &str) -> Result<serde_json::Value, String> {
    // 1) 首选 SFTP stat（快）
    if let Ok(sftp) = session_open_sftp(session).await {
        if let Ok(meta) = sftp.metadata(path).await {
            let is_dir = meta.is_dir();
            let is_symlink = meta.is_symlink();
            let is_file = !is_dir && !is_symlink;
            return Ok(serde_json::json!({
                "exists": true, "isDir": is_dir, "isFile": is_file,
                "isSymlink": is_symlink, "size": meta.len(),
            }));
        }
    }
    // 2) 兜底：exec `stat`（走 session_exec_with_output，auth_mode=sudo 时自动提权）
    let safe = path.replace('\'', "'\\''");
    let cmd = format!(
        "if [ -e '{}' ] || [ -L '{}' ]; then echo EXISTS; if [ -d '{}' ]; then echo DIR; elif [ -L '{}' ]; then echo SYMLINK; else echo FILE; fi; stat -c %s '{}' 2>/dev/null || echo 0; else echo MISSING; fi",
        safe, safe, safe, safe, safe
    );
    let (stdout, _, code) = session_exec_with_output(session, &cmd, 10).await?;
    if code != 0 {
        return Err(format!("Failed to stat path: {}", stdout.trim()));
    }
    let lines: Vec<&str> = stdout.lines().map(|l| l.trim()).collect();
    let exists = lines.first().map(|l| *l == "EXISTS").unwrap_or(false);
    if !exists {
        return Ok(serde_json::json!({ "exists": false }));
    }
    let kind = lines.get(1).copied().unwrap_or("FILE");
    let size: u64 = lines.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    Ok(serde_json::json!({
        "exists": true,
        "isDir": kind == "DIR",
        "isFile": kind == "FILE",
        "isSymlink": kind == "SYMLINK",
        "size": size,
    }))
}

pub async fn session_read_file(session: &SshSession, path: &str) -> Result<String, String> {
    use tokio::io::AsyncReadExt;
    // 1) 首选 SFTP 直读（快、无额外依赖）
    let sftp = match session_open_sftp(session).await {
        Ok(s) => s,
        Err(_) => return session_read_file_exec(session, path).await,
    };
    let open = sftp.open(path).await;
    if let Ok(mut file) = open {
        let mut buf = Vec::new();
        if file.read_to_end(&mut buf).await.is_ok() {
            if buf.len() > 1024 * 1024 {
                return Ok(String::from_utf8_lossy(&buf[..1024 * 1024]).to_string());
            }
            return Ok(String::from_utf8_lossy(&buf).to_string());
        }
        // 读失败 → 落到 exec 兜底
    }
    // 2) 兜底：exec `cat` 读取（走 session_exec_with_output，auth_mode=sudo 时自动提权）
    session_read_file_exec(session, path).await
}

/// SFTP 不可用时经 exec 读取文件内容（可提权）。输出限制 1MB 与 SFTP 路径一致。
/// 用 `head -c` 直接读文件而非 cat|head 管道——管道会吞掉 cat 的退出码，
/// 权限失败时无法区分"空文件"与"无权限"。
async fn session_read_file_exec(session: &SshSession, path: &str) -> Result<String, String> {
    let safe = path.replace('\'', "'\\''");
    let cmd = format!("head -c 1048576 '{}' 2>&1", safe);
    let (stdout, _, code) = session_exec_with_output(session, &cmd, 15).await?;
    if code != 0 {
        return Err(format!("Failed to read file: {}", stdout.trim()));
    }
    Ok(stdout)
}

pub async fn session_delete_file(session: &SshSession, path: &str, is_dir: bool) -> Result<String, String> {
    let cmd = if is_dir {
        format!("rm -rfv '{}'", path.replace('\'', "'\\''"))
    } else {
        format!("rm -fv '{}'", path.replace('\'', "'\\''"))
    };
    let (stdout, stderr, _) = session_exec_with_output(session, &cmd, 60).await?;
    Ok(format!("{}{}", stdout, stderr))
}

pub async fn session_delete_files_batch(session: &SshSession, paths: &[String], is_dir: bool) -> Result<String, String> {
    if paths.is_empty() { return Ok(String::new()); }
    let escaped: Vec<String> = paths.iter().map(|p| format!("'{}'", p.replace('\'', "'\\''"))).collect();
    let cmd = if is_dir { format!("rm -rfv {}", escaped.join(" ")) } else { format!("rm -fv {}", escaped.join(" ")) };
    let (stdout, stderr, _) = session_exec_with_output(session, &cmd, 60).await?;
    Ok(format!("{}{}", stdout, stderr))
}

pub async fn session_create_dir(session: &SshSession, path: &str) -> Result<(), String> {
    // ponytail: use `mkdir -p` via SSH exec — SFTP create_dir fails when parent dirs don't exist
    let escaped = path.replace('\'', "'\\''");
    let (_, _, code) = session_exec_with_output(session, &format!("mkdir -p '{}'", escaped), 10).await?;
    if code != 0 { return Err(format!("mkdir -p failed with exit code {}", code)); }
    Ok(())
}

pub async fn session_rename_file(session: &SshSession, old_path: &str, new_path: &str) -> Result<(), String> {
    // ponytail: use exec `mv` instead of SFTP rename — SFTP 无法提权，
    // 且与批量重命名（session_rename_files_batch 走 mv）保持一致；
    // mv 走 session_exec_with_output 自动获得 sudo（auth_mode=sudo 时）。
    let cmd = format!(
        "mv '{}' '{}'",
        old_path.replace('\'', "'\\''"),
        new_path.replace('\'', "'\\''")
    );
    let (_, stderr, code) = session_exec_with_output(session, &cmd, 10).await?;
    if code != 0 {
        return Err(format!("Rename failed: {}", stderr.trim()));
    }
    Ok(())
}

pub async fn session_rename_files_batch(session: &SshSession, renames: &[(String, String)]) -> Result<(), String> {
    for (old_path, new_path) in renames {
        let cmd = format!("mv '{}' '{}'", old_path.replace('\'', "'\\''"), new_path.replace('\'', "'\\''"));
        let (_, stderr, code) = session_exec_with_output(session, &cmd, 10).await?;
        if code != 0 { return Err(format!("Rename failed for {}: {}", old_path, stderr)); }
    }
    Ok(())
}

pub async fn session_copy_files_batch(session: &SshSession, sources: &[String], dest_dir: &str, is_move: bool) -> Result<String, String> {
    if sources.is_empty() { return Ok(String::new()); }
    let escaped: Vec<String> = sources.iter().map(|s| format!("'{}'", s.replace('\'', "'\\''"))).collect();
    let safe_dest = dest_dir.replace('\'', "'\\''");
    let cmd = if is_move {
        format!("mv -v {} '{}'", escaped.join(" "), safe_dest)
    } else {
        format!("cp -v {} '{}'", escaped.join(" "), safe_dest)
    };
    let (stdout, stderr, _) = session_exec_with_output(session, &cmd, 60).await?;
    Ok(format!("{}{}", stdout, stderr))
}

pub async fn session_copy_file(session: &SshSession, session_id: &str, src: &str, dst: &str, app_handle: &AppHandle) -> Result<(), String> {
    let cmd = format!("cp -v '{}' '{}' 2>&1", src.replace('\'', "'\\''"), dst.replace('\'', "'\\''"));
    let _ = app_handle.emit("copy-progress", serde_json::json!({"sessionId": session_id, "line": format!("$ {}", cmd), "status": "copying"}));
    // 权限模型 v8：统一 sudo 包装
    let (mut channel, _) = session_exec_channel(session, &cmd).await?;
    let mut stderr = String::new();
    loop {
        match channel.wait().await {
            Some(ChannelMsg::Data { data }) => {
                for line in String::from_utf8_lossy(&data).lines() {
                    if !line.trim().is_empty() {
                        let _ = app_handle.emit("copy-progress", serde_json::json!({"sessionId": session_id, "line": line, "status": "copying"}));
                    }
                }
            }
            Some(ChannelMsg::ExtendedData { data, ext }) if ext == 1 => {
                for line in String::from_utf8_lossy(&data).lines() {
                    if !line.trim().is_empty() {
                        stderr.push_str(line);
                        let _ = app_handle.emit("copy-progress", serde_json::json!({"sessionId": session_id, "line": line, "status": "error"}));
                    }
                }
            }
            Some(ChannelMsg::ExitStatus { exit_status }) => {
                if exit_status != 0 {
                    let err = format!("cp failed (exit {}): {}", exit_status, stderr.trim());
                    let _ = app_handle.emit("copy-progress", serde_json::json!({"sessionId": session_id, "line": err, "status": "error"}));
                    return Err(err);
                }
                return Ok(());
            }
            Some(ChannelMsg::Eof) => {}
            None => return Err("Connection lost during copy".to_string()),
            _ => {}
        }
    }
}

pub async fn session_copy_dir(session: &SshSession, session_id: &str, src: &str, dst: &str, app_handle: &AppHandle) -> Result<(), String> {
    let cmd = format!("cp -rvT '{}' '{}' 2>&1", src.replace('\'', "'\\''"), dst.replace('\'', "'\\''"));
    let _ = app_handle.emit("copy-progress", serde_json::json!({"sessionId": session_id, "line": format!("$ {}", cmd), "status": "copying"}));
    // 权限模型 v8：统一 sudo 包装
    let (mut channel, _) = session_exec_channel(session, &cmd).await?;
    let mut stderr = String::new();
    loop {
        match channel.wait().await {
            Some(ChannelMsg::Data { data }) => {
                for line in String::from_utf8_lossy(&data).lines() {
                    if !line.trim().is_empty() {
                        let _ = app_handle.emit("copy-progress", serde_json::json!({"sessionId": session_id, "line": line, "status": "copying"}));
                    }
                }
            }
            Some(ChannelMsg::ExtendedData { data, ext }) if ext == 1 => {
                for line in String::from_utf8_lossy(&data).lines() {
                    if !line.trim().is_empty() {
                        stderr.push_str(line);
                        let _ = app_handle.emit("copy-progress", serde_json::json!({"sessionId": session_id, "line": line, "status": "error"}));
                    }
                }
            }
            Some(ChannelMsg::ExitStatus { exit_status }) => {
                if exit_status != 0 {
                    let err = format!("cp -r failed (exit {}): {}", exit_status, stderr.trim());
                    let _ = app_handle.emit("copy-progress", serde_json::json!({"sessionId": session_id, "line": err, "status": "error"}));
                    return Err(err);
                }
                return Ok(());
            }
            Some(ChannelMsg::Eof) => {}
            None => return Err("Connection lost during copy".to_string()),
            _ => {}
        }
    }
}

pub async fn session_set_permissions(session: &SshSession, path: &str, mode: &str) -> Result<(), String> {
    let cmd = format!("chmod {} '{}'", mode, path.replace('\'', "'\\''"));
    let (_, stderr, code) = session_exec_with_output(session, &cmd, 10).await?;
    if code != 0 { Err(format!("chmod error: {}", stderr.trim())) } else { Ok(()) }
}

pub async fn session_set_permissions_batch(session: &SshSession, paths: &[String], mode: &str) -> Result<(), String> {
    if paths.is_empty() { return Ok(()); }
    let escaped: Vec<String> = paths.iter().map(|p| format!("'{}'", p.replace('\'', "'\\''"))).collect();
    let cmd = format!("chmod {} {}", mode, escaped.join(" "));
    let (_, stderr, code) = session_exec_with_output(session, &cmd, 10).await?;
    if code != 0 { Err(format!("chmod error: {}", stderr.trim())) } else { Ok(()) }
}

pub async fn session_check_space(session: &SshSession, path: &str) -> Result<String, String> {
    let mut channel = session_open_channel(session).await?;
    let safe = path.replace('\'', "'\\''");
    // 权限模型 v8：附加第 4 段会话模式标记（SUDO_MODE / ROOT_MODE），
    // 供前端在"无写入权限"时给出针对提权模式的解释性提示
    let mode_flag = if session.connect_info.auth_mode == "sudo" && session.connect_info.username != "root" {
        "SUDO_MODE"
    } else {
        "ROOT_MODE"
    };
    let cmd = format!(
        "df -B1 '{}' | tail -1 | awk '{{print $4}}'; echo '---'; touch '{}/.__wtest__' 2>&1 && rm '{}/.__wtest__' && echo 'OK' || echo 'DENIED'; echo '---'; find '{}' -maxdepth 1 -mindepth 1 -printf '%f|%y\n' | grep -v '^\\.|'; echo '---'; echo '{}'",
        safe, safe, safe, safe, mode_flag
    );
    channel.exec(true, cmd).await.map_err(|e| format!("Exec failed: {}", e))?;
    let mut output = String::new();
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
    loop {
        tokio::select! {
            msg = channel.wait() => match msg {
                Some(ChannelMsg::Data { data }) => output.push_str(&String::from_utf8_lossy(&data)),
                Some(ChannelMsg::ExitStatus { .. }) | Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                _ => {}
            },
            _ = tokio::time::sleep_until(deadline) => break,
        }
    }
    Ok(output.trim().to_string())
}

pub async fn session_read_file_bytes(session: &SshSession, path: &str) -> Result<Vec<u8>, String> {
    use tokio::io::AsyncReadExt;
    let sftp = session_open_sftp(session).await?;
    let mut file = sftp.open(path).await.map_err(|e| format!("Failed to open remote file: {}", e))?;
    let mut content = Vec::new();
    file.read_to_end(&mut content).await.map_err(|e| format!("Failed to read remote file: {}", e))?;
    Ok(content)
}

/// Stream remote file to local path in chunks — avoids holding manager lock and caps memory at 256KB.
/// Emits `save-local-progress` events: { sessionId, uploaded, total }
/// Supports pause/stop via TransferControl.
pub async fn session_stream_file_to_local(session: &SshSession, remote_path: &str, local_path: &str, app_handle: &AppHandle, session_id: &str, ctrl: Arc<TransferControl>) -> Result<(), String> {
    use tokio::io::AsyncReadExt;
    let sftp = session_open_sftp(session).await?;
    let total = sftp.metadata(remote_path).await.map(|m| m.len()).unwrap_or(0);
    let mut file = sftp.open(remote_path).await.map_err(|e| format!("Failed to open remote file: {}", e))?;
    let mut out = tokio::fs::File::create(local_path).await.map_err(|e| format!("Failed to create local file: {}", e))?;
    use tokio::io::AsyncWriteExt;
    let mut buf = vec![0u8; 256 * 1024];
    let mut sent: u64 = 0;
    loop {
        // ponytail: check stop/pause flags each chunk
        if ctrl.stopped.load(Ordering::Relaxed) {
            drop(out);
            let _ = tokio::fs::remove_file(local_path).await;
            return Err("Transfer stopped".to_string());
        }
        while ctrl.paused.load(Ordering::Relaxed) {
            if ctrl.stopped.load(Ordering::Relaxed) {
                drop(out);
                let _ = tokio::fs::remove_file(local_path).await;
                return Err("Transfer stopped".to_string());
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let n = file.read(&mut buf).await.map_err(|e| format!("Read failed: {}", e))?;
        if n == 0 { break; }
        out.write_all(&buf[..n]).await.map_err(|e| format!("Write failed: {}", e))?;
        sent += n as u64;
        let _ = app_handle.emit("save-local-progress", serde_json::json!({
            "sessionId": session_id, "uploaded": sent, "total": total
        }));
    }
    out.flush().await.map_err(|e| format!("Flush failed: {}", e))?;
    Ok(())
}

pub async fn session_download_to_local(session: &SshSession, remote_path: &str, file_name: &str, app_handle: &AppHandle, session_id: &str) -> Result<String, String> {
    let temp_dir = std::env::temp_dir().join("leepanel-preview");
    std::fs::create_dir_all(&temp_dir).map_err(|e| format!("Failed to create temp dir: {}", e))?;
    let local_path = temp_dir.join(file_name);
    let local_str = local_path.to_string_lossy().to_string();
    // ponytail: image preview — no pause/stop needed, pass inert control
    let ctrl = Arc::new(TransferControl { paused: AtomicBool::new(false), stopped: AtomicBool::new(false) });
    session_stream_file_to_local(session, remote_path, &local_str, app_handle, session_id, ctrl).await?;
    let _ = open::that(&local_path);
    Ok(local_str)
}

// ===== Free functions for session-level operations (no manager lock required) =====

pub async fn session_open_channel(session: &SshSession) -> Result<russh::Channel<client::Msg>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    session.channel_open_tx
        .send(ChannelOpen { reply: tx })
        .await
        .map_err(|_| "Background task unavailable".to_string())?;
    rx.await.map_err(|_| "Failed to open channel".to_string())
}

/// 方案 A：探测当前会话是否可免密 sudo（sudoers NOPASSWD 或 sudo 凭证缓存 ts 有效）。
/// 用独立 channel 执行 `sudo -n true`，不经 session_exec_with_output（避免递归）。
/// 结果缓存到会话级 sudo_nopass 标记，连接生命周期内复用；ts 过期后由
/// 调用方在 `sudo -n` 失败时重置（见 session_exec_with_output）。
async fn session_try_sudo_nopass(session: &SshSession) -> bool {
    if session.sudo_nopass.load(std::sync::atomic::Ordering::Relaxed) {
        return true;
    }
    let mut channel = match session_open_channel(session).await {
        Ok(c) => c,
        Err(_) => return false,
    };
    if channel.exec(true, "sudo -n true").await.is_err() {
        return false;
    }
    let mut exit_code: i32 = -1;
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
    loop {
        tokio::select! {
            msg = channel.wait() => {
                match msg {
                    Some(russh::ChannelMsg::ExitStatus { exit_status }) => {
                        exit_code = exit_status as i32;
                    }
                    Some(russh::ChannelMsg::Close) | None => break,
                    _ => {}
                }
            }
            _ = tokio::time::sleep_until(deadline) => break,
        }
    }
    if exit_code == 0 {
        session.sudo_nopass.store(true, std::sync::atomic::Ordering::Relaxed);
        true
    } else {
        false
    }
}

/// 权限模型 v8：打开 exec channel 并执行命令（流式输出场景）。
/// auth_mode='sudo' 且非 root 时自动包装 `sudo -S` 并喂入会话级 sudo 密码
/// （密码经 stdin，不进进程列表）。返回 channel 供调用方流式读取进度。
/// 第二项返回值标记本次是否走了 sudo（调用方可据此判断错误语义）。
pub async fn session_exec_channel(
    session: &SshSession,
    cmd: &str,
) -> Result<(russh::Channel<client::Msg>, bool), String> {
    let needs_sudo = session.connect_info.auth_mode == "sudo"
        && session.connect_info.username != "root";
    let mut channel = session_open_channel(session).await?;
    if needs_sudo {
        let pw = session.sudo_password.lock().await.clone();
        if pw.is_none() {
            // 方案 A：密码未配置时先探测免密 sudo（NOPASSWD / 凭证缓存），
            // 可用则走 sudo -n（不弹窗）；不可用再回退弹窗流程。
            if !session_try_sudo_nopass(session).await {
                return Err("SUDO_PASSWORD_REQUIRED".to_string());
            }
            let wrapped = format!("sudo -n {}", cmd);
            channel.exec(true, wrapped.as_str()).await.map_err(|e| format!("Exec failed: {}", e))?;
        } else {
            let wrapped = format!("sudo -S {}", cmd);
            channel.exec(true, wrapped.as_str()).await.map_err(|e| format!("Exec failed: {}", e))?;
            if let Some(pw) = pw {
                let mut data = pw.into_bytes();
                data.push(b'\n');
                let mut cursor = Cursor::new(&data);
                let _ = channel.data(&mut cursor).await;
                let _ = channel.eof().await;
            }
        }
    } else {
        channel.exec(true, cmd).await.map_err(|e| format!("Exec failed: {}", e))?;
    }
    Ok((channel, needs_sudo))
}

pub async fn session_exec_with_output(
    session: &SshSession,
    cmd: &str,
    timeout_secs: u64,
) -> Result<(String, String, i32), String> {
    // 权限模型 v8：auth_mode='sudo' 且非 root 身份 → 命令经 sudo 执行。
    // 密码经 channel stdin 喂入（不拼进命令行，避免出现在远程 ps 进程列表）。
    // 方案 A：sudo 密码未配置时，先探测免密 sudo（NOPASSWD / 凭证缓存 ts），
    // 可用则走 `sudo -n` 免密路径；不可用才返回 SUDO_PASSWORD_REQUIRED 弹窗。
    let needs_sudo = session.connect_info.auth_mode == "sudo"
        && session.connect_info.username != "root";
    let sudo_pw: Option<String> = if needs_sudo {
        let pw = session.sudo_password.lock().await.clone();
        if pw.is_none() {
            // 约定错误码：前端捕获后弹窗输入 sudo 密码，再调用 ssh_set_sudo_password 重试
            if !session_try_sudo_nopass(session).await {
                return Err("SUDO_PASSWORD_REQUIRED".to_string());
            }
            None
        } else {
            pw
        }
    } else {
        None
    };
    let used_sudo_nopass = needs_sudo && sudo_pw.is_none();

    let mut channel = session_open_channel(session).await?;
    if needs_sudo {
        if used_sudo_nopass {
            let wrapped = format!("sudo -n {}", cmd);
            channel.exec(true, wrapped.as_str()).await.map_err(|e| format!("Exec failed: {}", e))?;
        } else {
            let wrapped = format!("sudo -S {}", cmd);
            channel.exec(true, wrapped.as_str()).await.map_err(|e| format!("Exec failed: {}", e))?;
            if let Some(pw) = sudo_pw {
                let mut data = pw.into_bytes();
                data.push(b'\n');
                let mut cursor = Cursor::new(&data);
                let _ = channel.data(&mut cursor).await;
                let _ = channel.eof().await;
            }
        }
    } else {
        channel.exec(true, cmd).await.map_err(|e| format!("Exec failed: {}", e))?;
    }

    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut exit_code: i32 = -1;
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(timeout_secs);

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
                    Some(ChannelMsg::Eof) => {}
                    Some(ChannelMsg::Close) | None => break,
                    _ => {}
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                return Err(format!("Command timed out after {}s", timeout_secs));
            }
        }
    }

    // sudo 密码错误的典型信号：sudo 把提示/错误写到 stderr（如 "incorrect password"）
    if needs_sudo && exit_code != 0 && stderr.to_lowercase().contains("incorrect password") {
        return Err("SUDO_PASSWORD_INCORRECT".to_string());
    }
    // 方案 A：sudo -n 因凭证缓存过期/未配置 NOPASSWD 而失败 → 重置免密标记并回退弹窗
    if used_sudo_nopass && exit_code != 0 && stderr.to_lowercase().contains("a password is required") {
        session.sudo_nopass.store(false, std::sync::atomic::Ordering::Relaxed);
        return Err("SUDO_PASSWORD_REQUIRED".to_string());
    }

    Ok((stdout, stderr, exit_code))
}

pub async fn session_open_channel_and_exec(
    session: &SshSession,
    cmd: &str,
    timeout_secs: u64,
) -> Result<String, String> {
    let (stdout, _, _) = session_exec_with_output(session, cmd, timeout_secs).await?;
    let result = stdout.trim().to_string();
    if result.is_empty() {
        Err(format!("Empty output for: {}", cmd))
    } else {
        Ok(result)
    }
}

pub async fn session_open_sftp(session: &SshSession) -> Result<Arc<russh_sftp::client::SftpSession>, String> {
    // Check cache
    {
        let cache = session.sftp_cache.lock().await;
        if let Some((sftp, created_at)) = cache.as_ref() {
            if created_at.elapsed().as_secs() < 30 {
                return Ok(sftp.clone());
            }
        }
    }

    let channel = session_open_channel(session).await?;
    channel.request_subsystem(true, "sftp").await
        .map_err(|e| format!("SFTP subsystem request failed: {}", e))?;
    let stream = channel.into_stream();
    let config = russh_sftp::client::Config {
        max_packet_len: 64 * 1024,
        max_concurrent_writes: 8,
        request_timeout_secs: 15,
    };
    let sftp = russh_sftp::client::SftpSession::new_with_config(stream, config).await
        .map_err(|e| format!("SFTP init failed: {}", e))?;
    sftp.set_timeout(60);

    {
        let mut cache = session.sftp_cache.lock().await;
        *cache = Some((Arc::new(sftp), tokio::time::Instant::now()));
    }

    let cache = session.sftp_cache.lock().await;
    Ok(cache.as_ref().unwrap().0.clone())
}

pub async fn session_write_file(session: &SshSession, path: &str, content: &str) -> Result<(), String> {
    session_write_file_bytes(session, path, content.as_bytes()).await
}

pub async fn session_write_file_bytes(session: &SshSession, path: &str, content: &[u8]) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    // 1) 首选 SFTP 直写（快、无额外依赖）
    match session_open_sftp(session).await {
        Ok(sftp) => {
            match sftp.create(path).await {
                Ok(mut file) => {
                    let w = file.write_all(content).await;
                    let f = file.shutdown().await;
                    if w.is_ok() && f.is_ok() {
                        return Ok(());
                    }
                    // 写/刷新失败 → 落到 exec 兜底（文件可能部分写入，由兜底整体覆盖）
                }
                Err(_) => {
                    // create 失败（典型：目标文件已存在且为 root 属主 644 残留，
                    // 普通用户无权覆盖）→ 落到 exec 兜底
                }
            }
        }
        Err(_) => {
            // SFTP 会话打不开 → 落到 exec 兜底
        }
    }
    // 2) 兜底：base64 解码写入唯一临时文件后 mv 覆盖目标。
    //    mv/rename 只要求目标【目录】可写（/tmp 等对所有用户可写），不检查
    //    目标文件属主/权限 → 即使执行者非 root 也能覆盖 root 属主残留，
    //    且不依赖 sudoers 白名单放行 bash/base64。
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(content);
    let safe_path = path.replace('\'', "'\\''");
    let inner = format!(
        "echo {} | base64 -d > '/tmp/.leepanel-write-$$' && mv -f '/tmp/.leepanel-write-$$' '{}' || rm -f '/tmp/.leepanel-write-$$'",
        b64, safe_path
    );
    let cmd = format!("bash -c \"{}\"", inner);
    let (_, stderr, code) = session_exec_with_output(session, &cmd, 10).await?;
    if code != 0 {
        return Err(format!("Failed to write file via exec: {}", stderr.trim()));
    }
    Ok(())
}

pub async fn session_disconnect(session: &SshSession) -> Result<(), String> {
    let h = session.handle.clone();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let h = h.lock().await;
        let _ = h.disconnect(russh::Disconnect::ByApplication, "", "en").await;
    }).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== parse_curl_progress =====

    #[test]
    fn parse_curl_progress_percentage() {
        // ponytail: function only handles simple 'N%' patterns; prefix like '#=#=#' is stripped upstream by split('\r')
        assert_eq!(parse_curl_progress("45.2%"), Some(45.2));
        assert_eq!(parse_curl_progress("100%"), Some(100.0));
        assert_eq!(parse_curl_progress("0.5%"), Some(0.5));
    }

    #[test]
    fn parse_curl_progress_no_percent() {
        assert_eq!(parse_curl_progress("downloading..."), None);
        assert_eq!(parse_curl_progress(""), None);
    }

    // ===== SshCache =====

    #[tokio::test]
    async fn cache_put_and_get() {
        let cache = SshCache::new();
        cache.put("s1", "system_info", "ubuntu".to_string());
        assert_eq!(cache.get("s1", "system_info", 60), Some("ubuntu".to_string()));
    }

    #[tokio::test]
    async fn cache_miss_returns_none() {
        let cache = SshCache::new();
        assert_eq!(cache.get("s1", "nonexistent", 60), None);
    }

    #[tokio::test]
    async fn cache_ttl_zero_always_valid() {
        // ttl_secs=0 means no expiry check
        let cache = SshCache::new();
        cache.put("s1", "k", "v".to_string());
        assert_eq!(cache.get("s1", "k", 0), Some("v".to_string()));
    }

    #[tokio::test]
    async fn cache_invalidate_specific_keys() {
        let cache = SshCache::new();
        cache.put("s1", "a", "1".to_string());
        cache.put("s1", "b", "2".to_string());
        cache.invalidate("s1", &["a"]);
        assert_eq!(cache.get("s1", "a", 60), None);
        assert_eq!(cache.get("s1", "b", 60), Some("2".to_string()));
    }

    #[tokio::test]
    async fn cache_clear_session() {
        let cache = SshCache::new();
        cache.put("s1", "k1", "v1".to_string());
        cache.put("s1", "k2", "v2".to_string());
        cache.put("s2", "k1", "other".to_string());
        cache.clear_session("s1");
        assert_eq!(cache.get("s1", "k1", 60), None);
        assert_eq!(cache.get("s1", "k2", 60), None);
        assert_eq!(cache.get("s2", "k1", 60), Some("other".to_string()));
    }

    #[tokio::test]
    async fn cache_session_isolation() {
        let cache = SshCache::new();
        cache.put("s1", "key", "val1".to_string());
        cache.put("s2", "key", "val2".to_string());
        assert_eq!(cache.get("s1", "key", 60), Some("val1".to_string()));
        assert_eq!(cache.get("s2", "key", 60), Some("val2".to_string()));
    }
}
