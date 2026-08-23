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

/// Manually add a pre-trusted host fingerprint (e.g. obtained out-of-band from a
/// cloud console / administrator). The fingerprint may include a "SHA256:" prefix.
#[tauri::command]
pub fn known_hosts_add(db: tauri::State<'_, DbPool>, host: String, key_type: String, fingerprint: String) -> Result<(), String> {
    let host = host.trim();
    let key_type = key_type.trim();
    if host.is_empty() { return Err("Host is required".to_string()); }
    if key_type.is_empty() { return Err("Key type is required".to_string()); }
    let fp = fingerprint.trim();
    let fp = fp.strip_prefix("SHA256:").unwrap_or(fp).trim();
    if fp.is_empty() { return Err("Fingerprint is required".to_string()); }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let conn = db.lock().unwrap();
    crate::db::KnownHostsManager::insert(&conn, host, key_type, fp, "", now)
}

/// Import trusted host keys from the system OpenSSH known_hosts file (~/.ssh/known_hosts).
/// Parses standard entries (`host keytype base64blob`), including `[host]:port` forms and
/// comma-separated aliases; skips hashed hosts, wildcards and @-tagged lines.
/// Existing entries in the app store are never overwritten. Returns the number imported.
#[tauri::command]
pub fn known_hosts_import_from_ssh(db: tauri::State<'_, DbPool>) -> Result<u32, String> {
    use crate::db::KnownHostsManager;

    let home = dirs::home_dir().ok_or_else(|| "Cannot determine home directory".to_string())?;
    let path = home.join(".ssh").join("known_hosts");
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let conn = db.lock().unwrap();
    let mut imported = 0u32;
    for line in content.lines() {
        for (host, key_type, fingerprint, key_blob) in parse_known_hosts_line(line) {
            if KnownHostsManager::insert_if_absent(&conn, &host, &key_type, &fingerprint, &key_blob, now)? {
                imported += 1;
            }
        }
    }
    Ok(imported)
}

/// Parse one OpenSSH known_hosts line into (host, key_type, fingerprint, key_blob) entries.
/// Handles comma-separated host aliases and `[host]:port` / `host:port` forms.
/// Skips: comments, @-tagged lines (@cert-authority/@revoked), hashed hosts (`|1|...`),
/// wildcard hosts, and unparseable key blobs.
fn parse_known_hosts_line(line: &str) -> Vec<(String, String, String, String)> {
    use russh_keys::PublicKeyBase64;
    let mut out = Vec::new();
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with('@') {
        return out;
    }
    let mut parts = line.split_whitespace();
    let host_part = match parts.next() { Some(h) => h, None => return out };
    let key_type_part = match parts.next() { Some(k) => k, None => return out };
    let key_blob = match parts.next() { Some(b) => b, None => return out };
    // 跳过 hashed hosts 与通配符（无法按字面 host 匹配）
    if host_part.starts_with('|') || host_part.contains('*') || host_part.contains('?') {
        return out;
    }
    let key = match russh_keys::parse_public_key_base64(key_blob) {
        Ok(k) => k,
        Err(_) => return out,
    };
    // 仅接受与声明类型一致的 key type（防 blob 与类型字段不符）
    if key.name() != key_type_part {
        return out;
    }
    let fingerprint = key.fingerprint();
    let blob = key.public_key_base64();
    // 一行可能有多个别名 host（逗号分隔）
    for alias in host_part.split(',') {
        // 提取 host：`[host]:port` → host；`host:port` → host；裸 IPv6 保持原样
        let host = if let Some(rest) = alias.strip_prefix('[') {
            rest.split(']').next().unwrap_or("")
        } else if alias.contains(':') && alias.matches(':').count() == 1 && !alias.starts_with('[') {
            alias.split(':').next().unwrap_or(alias)
        } else {
            alias
        };
        if !host.is_empty() {
            out.push((host.to_string(), key_type_part.to_string(), fingerprint.clone(), blob.clone()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::parse_known_hosts_line;

    #[test]
    fn parse_simple_host_line() {
        // 真实 ed25519 公钥（仅作解析验证）
        let blob = "AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ";
        let line = format!("example.com ssh-ed25519 {}", blob);
        let entries = parse_known_hosts_line(&line);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "example.com");
        assert_eq!(entries[0].1, "ssh-ed25519");
        assert_eq!(entries[0].2.len(), 43); // SHA256 base64 无填充
    }

    #[test]
    fn parse_bracketed_port_host() {
        let blob = "AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ";
        let line = format!("[example.com]:2222 ssh-ed25519 {}", blob);
        let entries = parse_known_hosts_line(&line);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "example.com");
    }

    #[test]
    fn parse_comma_aliases() {
        let blob = "AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ";
        let line = format!("host1,host2,host3 ssh-ed25519 {}", blob);
        let entries = parse_known_hosts_line(&line);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].0, "host1");
        assert_eq!(entries[2].0, "host3");
    }

    #[test]
    fn skip_hashed_wildcard_tagged_and_comments() {
        let blob = "AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ";
        assert!(parse_known_hosts_line(&format!("|1|abc== ssh-ed25519 {}", blob)).is_empty());
        assert!(parse_known_hosts_line(&format!("*.example.com ssh-ed25519 {}", blob)).is_empty());
        assert!(parse_known_hosts_line(&format!("@cert-authority example.com ssh-ed25519 {}", blob)).is_empty());
        assert!(parse_known_hosts_line("# comment line").is_empty());
        assert!(parse_known_hosts_line("").is_empty());
    }

    #[test]
    fn skip_bad_blob_and_type_mismatch() {
        let blob = "AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ";
        assert!(parse_known_hosts_line("example.com ssh-ed25519 not-a-valid-blob!").is_empty());
        // blob 是 ed25519，但类型字段声明为 ssh-rsa → 拒绝
        assert!(parse_known_hosts_line(&format!("example.com ssh-rsa {}", blob)).is_empty());
    }
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
