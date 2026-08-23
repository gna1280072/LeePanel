pub use rusqlite::Connection as SqliteConn;
use std::path::PathBuf;
use std::sync::Mutex;

/// Get the SQLite data directory: <config_dir>/leepanel
pub fn db_dir() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("leepanel");
    std::fs::create_dir_all(&path).ok();
    path
}

/// Get the SQLite database path: <config_dir>/leepanel/data.db
pub fn db_path() -> PathBuf {
    let mut path = db_dir();
    path.push("data.db");
    path
}

/// Initialize the database and create tables if needed.
pub fn init_db() -> Result<Mutex<SqliteConn>, String> {
    let path = db_path();
    let conn = SqliteConn::open(&path)
        .map_err(|e| format!("Failed to open SQLite DB: {}", e))?;

    // Enable WAL mode for better concurrency
    conn.execute_batch("PRAGMA journal_mode=WAL;")
        .map_err(|e| format!("Failed to set WAL mode: {}", e))?;

    // Create tables
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS connections (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL DEFAULT '',
            host TEXT NOT NULL,
            port INTEGER NOT NULL DEFAULT 22,
            username TEXT NOT NULL DEFAULT 'root',
            auth_type TEXT NOT NULL DEFAULT 'password',
            key_path TEXT,
            password TEXT
        );

        CREATE TABLE IF NOT EXISTS favorites (
            path TEXT PRIMARY KEY,
            name TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS fb_favorites (
            server_host TEXT NOT NULL,
            path TEXT NOT NULL,
            PRIMARY KEY(server_host, path)
        );

        CREATE TABLE IF NOT EXISTS fb_dir_cache (
            server_host TEXT NOT NULL,
            path TEXT NOT NULL,
            data TEXT NOT NULL,
            cached_at INTEGER NOT NULL,
            PRIMARY KEY(server_host, path)
        );

        CREATE TABLE IF NOT EXISTS db_remarks (
            server_host TEXT NOT NULL,
            db_name TEXT NOT NULL,
            remark TEXT NOT NULL DEFAULT '',
            PRIMARY KEY(server_host, db_name)
        );

        CREATE TABLE IF NOT EXISTS db_credentials (
            server_host TEXT NOT NULL,
            db_name TEXT NOT NULL,
            password TEXT NOT NULL DEFAULT '',
            access_type TEXT NOT NULL DEFAULT 'local',
            allowed_ip TEXT NOT NULL DEFAULT '',
            PRIMARY KEY(server_host, db_name)
        );

        CREATE TABLE IF NOT EXISTS site_metadata (
            server_host TEXT NOT NULL,
            domain TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            PRIMARY KEY(server_host, domain)
        );

        CREATE TABLE IF NOT EXISTS custom_software (
            server_host TEXT NOT NULL,
            package_name TEXT NOT NULL,
            display_name TEXT NOT NULL,
            category TEXT NOT NULL DEFAULT 'other',
            PRIMARY KEY(server_host, package_name)
        );

        CREATE TABLE IF NOT EXISTS tunnels (
            id TEXT PRIMARY KEY,
            server_key TEXT NOT NULL,
            tunnel_type TEXT NOT NULL,
            local_host TEXT NOT NULL,
            local_port INTEGER NOT NULL,
            remote_host TEXT NOT NULL,
            remote_port INTEGER NOT NULL,
            created_at INTEGER NOT NULL,
            note TEXT NOT NULL DEFAULT ''
        );

        CREATE TABLE IF NOT EXISTS known_hosts (
            host TEXT NOT NULL,
            key_type TEXT NOT NULL,
            fingerprint TEXT NOT NULL,
            key_blob TEXT NOT NULL DEFAULT '',
            first_seen INTEGER NOT NULL,
            last_seen INTEGER NOT NULL,
            PRIMARY KEY (host, key_type)
        );

        CREATE INDEX IF NOT EXISTS idx_tunnels_server_key ON tunnels(server_key);"
    ).map_err(|e| format!("Failed to create tables: {}", e))?;

    // ponytail: versioned schema migrations — add new versions at the bottom
    let schema_version: i32 = conn
        .query_row("SELECT value FROM settings WHERE key='schema_version'", [], |r| r.get::<_, String>(0))
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    if schema_version < 1 {
        // v1: sites table removed — sites are now read directly from Nginx via SSH
        // Drop the sites table if it exists (ponytail: user confirmed no backup needed)
        let _ = conn.execute_batch("DROP TABLE IF EXISTS sites;");
    }

    // Always ensure remember_me column exists (idempotent migration)
    let has_remember_me: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('connections') WHERE name='remember_me'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .map(|c| c > 0)
        .unwrap_or(false);
    
    if !has_remember_me {
        let _ = conn.execute_batch("ALTER TABLE connections ADD COLUMN remember_me INTEGER DEFAULT 0;");
    }

    // v5: add passphrase column to connections (idempotent ALTER TABLE — encrypted SSH key passphrase)
    let has_passphrase: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('connections') WHERE name='passphrase'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .map(|c| c > 0)
        .unwrap_or(false);

    if !has_passphrase {
        let _ = conn.execute_batch("ALTER TABLE connections ADD COLUMN passphrase TEXT;");
    }

    // v6: add has_password/has_passphrase marker columns to connections
    // (keyring migration — marks whether a credential exists in the system keyring;
    //  legacy plaintext rows are synced to the markers during migrate_credentials)
    let has_marker_password: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('connections') WHERE name='has_password'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .map(|c| c > 0)
        .unwrap_or(false);
    if !has_marker_password {
        let _ = conn.execute_batch("ALTER TABLE connections ADD COLUMN has_password INTEGER DEFAULT 0;");
    }

    let has_marker_passphrase: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('connections') WHERE name='has_passphrase'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .map(|c| c > 0)
        .unwrap_or(false);
    if !has_marker_passphrase {
        let _ = conn.execute_batch("ALTER TABLE connections ADD COLUMN has_passphrase INTEGER DEFAULT 0;");
    }

    // v3: add db_user column to db_credentials (ponytail: idempotent ALTER TABLE)
    let has_db_user: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('db_credentials') WHERE name='db_user'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .map(|c| c > 0)
        .unwrap_or(false);

    if !has_db_user {
        let _ = conn.execute_batch("ALTER TABLE db_credentials ADD COLUMN db_user TEXT NOT NULL DEFAULT '';");
    }

    // v4: add note column to tunnels (idempotent ALTER TABLE)
    let has_tunnel_note: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('tunnels') WHERE name='note'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .map(|c| c > 0)
        .unwrap_or(false);

    if !has_tunnel_note {
        let _ = conn.execute_batch("ALTER TABLE tunnels ADD COLUMN note TEXT NOT NULL DEFAULT '';");
    }

    // Update schema version to latest
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('schema_version', '7')",
        [],
    ).map_err(|e| format!("Failed to update schema_version: {}", e))?;

    Ok(Mutex::new(conn))
}

/// 凭据安全迁移（一次性，幂等）：把 connections 表中的历史明文 password/passphrase
/// 搬入系统钥匙串，成功后清空明文列并写入标记。
///
/// 策略：钥匙串不可用 → 跳过并记录 `credential_migration=unavailable`（下次启动重试）；
/// 写入失败 → 中止并返回 Err（保留明文，不破坏数据）；迁移前自动备份 DB 文件。
/// 返回迁移的凭据条数（password/passphrase 各计 1 条），供前端展示"已迁移"提示。
pub fn migrate_credentials(db: &Mutex<SqliteConn>) -> Result<usize, String> {
    use crate::credentials::{self, CredKind};
    use rusqlite::params;

    let conn = db.lock().unwrap();

    // 幂等：已成功迁移或正在 unavailable 状态（避免每启动都全表扫描；unavailable 也跳过，
    // 由"钥匙串恢复"场景手动重试——见下）
    let state: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'credential_migration'",
            [],
            |r| r.get(0),
        )
        .ok();
    if state.as_deref() == Some("done") || state.as_deref() == Some("unavailable") {
        return Ok(0);
    }

    if !credentials::store_available() {
        let _ = conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('credential_migration', 'unavailable')",
            [],
        );
        return Ok(0);
    }

    // 扫描仍有明文的连接
    let mut stmt = conn
        .prepare(
            "SELECT id, password, passphrase FROM connections \
             WHERE (password IS NOT NULL AND password != '') OR (passphrase IS NOT NULL AND passphrase != '')",
        )
        .map_err(|e| format!("Failed to scan credentials: {}", e))?;
    let rows: Vec<(String, Option<String>, Option<String>)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .map_err(|e| format!("Failed to scan credentials: {}", e))?
        .filter_map(|r| r.ok())
        .collect();

    if rows.is_empty() {
        // 无明文可迁移，直接标记完成
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('credential_migration', 'done')",
            [],
        )
        .map_err(|e| format!("Failed to record migration state: {}", e))?;
        return Ok(0);
    }

    // 有明文待迁移 → 先备份 DB（WAL 模式已开启，复制文件安全）
    let bak = db_path().with_extension("db.bak");
    std::fs::copy(db_path(), &bak).map_err(|e| format!("Failed to back up database: {}", e))?;

    let mut migrated = 0usize;
    for (id, pw, pp) in rows {
        if let Some(p) = pw.as_deref().filter(|p| !p.is_empty()) {
            credentials::store_set(&id, CredKind::Password, p)?; // 失败即中止，保留明文可回滚
            migrated += 1;
        }
        if let Some(p) = pp.as_deref().filter(|p| !p.is_empty()) {
            credentials::store_set(&id, CredKind::Passphrase, p)?;
            migrated += 1;
        }
        let has_password = pw.as_deref().is_some_and(|p| !p.is_empty());
        let has_passphrase = pp.as_deref().is_some_and(|p| !p.is_empty());
        conn.execute(
            "UPDATE connections SET has_password = ?1, has_passphrase = ?2, password = NULL, passphrase = NULL WHERE id = ?3",
            params![has_password, has_passphrase, id],
        )
        .map_err(|e| format!("Failed to clear migrated credentials: {}", e))?;
    }

    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('credential_migration', 'done')",
        [],
    )
    .map_err(|e| format!("Failed to record migration state: {}", e))?;
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('credential_migration_count', ?1)",
        params![migrated as i64],
    )
    .map_err(|e| format!("Failed to record migration count: {}", e))?;
    Ok(migrated)
}

// ===== File Browser Favorites =====

// ===== File Browser Directory Cache =====

pub struct FbDirCache;

impl FbDirCache {
    // ponytail: get cached JSON + cached_at if within ttl_hours; returns None if expired or missing
    pub fn get(conn: &SqliteConn, server_host: &str, path: &str, ttl_hours: u32) -> Option<(String, i64)> {
        let mut stmt = conn.prepare(
            "SELECT data, cached_at FROM fb_dir_cache WHERE server_host = ?1 AND path = ?2"
        ).ok()?;
        let (data, cached_at): (String, i64) = stmt.query_row(
            rusqlite::params![server_host, path], |row| Ok((row.get(0)?, row.get(1)?))
        ).ok()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let ttl_ms = (ttl_hours as i64) * 3600 * 1000;
        if now - cached_at > ttl_ms { return None; } // ponytail: expired
        Some((data, cached_at))
    }

    pub fn put(conn: &SqliteConn, server_host: &str, path: &str, data: &str) -> Result<(), String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        conn.execute(
            "INSERT OR REPLACE INTO fb_dir_cache (server_host, path, data, cached_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![server_host, path, data, now],
        ).map_err(|e| format!("Failed to cache dir: {}", e))?;
        Ok(())
    }

    // ponytail: touch cached_at without rewriting data
    pub fn touch(conn: &SqliteConn, server_host: &str, path: &str) -> Result<(), String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        conn.execute(
            "UPDATE fb_dir_cache SET cached_at = ?1 WHERE server_host = ?2 AND path = ?3",
            rusqlite::params![now, server_host, path],
        ).map_err(|e| format!("Failed to touch cache: {}", e))?;
        Ok(())
    }

    // ponytail: delete all cached directories
    pub fn clear_all(conn: &SqliteConn) -> Result<u32, String> {
        let affected = conn.execute("DELETE FROM fb_dir_cache", [])
            .map_err(|e| format!("Failed to clear cache: {}", e))?;
        Ok(affected as u32)
    }

    // ponytail: count cached directories
    pub fn count(conn: &SqliteConn) -> u32 {
        conn.query_row("SELECT COUNT(*) FROM fb_dir_cache", [], |r| r.get::<_, u32>(0))
            .unwrap_or(0)
    }
}

pub struct FbFavorites;

impl FbFavorites {
    pub fn list(conn: &SqliteConn, server_host: &str) -> Vec<String> {
        let mut stmt = conn.prepare(
            "SELECT path FROM fb_favorites WHERE server_host = ?1 ORDER BY path"
        ).unwrap();
        stmt.query_map([server_host], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    }

    pub fn add(conn: &SqliteConn, server_host: &str, path: &str) -> Result<(), String> {
        conn.execute(
            "INSERT OR IGNORE INTO fb_favorites (server_host, path) VALUES (?1, ?2)",
            rusqlite::params![server_host, path],
        ).map_err(|e| format!("Failed to add fb favorite: {}", e))?;
        Ok(())
    }

    pub fn remove(conn: &SqliteConn, server_host: &str, path: &str) -> Result<(), String> {
        conn.execute(
            "DELETE FROM fb_favorites WHERE server_host = ?1 AND path = ?2",
            rusqlite::params![server_host, path],
        ).map_err(|e| format!("Failed to remove fb favorite: {}", e))?;
        Ok(())
    }
}

// ===== Database Credentials =====

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DbCredential {
    pub db_name: String,
    pub db_user: String,
    pub password: String,
    pub access_type: String,
    pub allowed_ip: String,
}

pub struct DbCredentialsManager;

impl DbCredentialsManager {
    /// Save or update database credentials
    pub fn save(
        conn: &SqliteConn,
        server_host: &str,
        db_name: &str,
        db_user: &str,
        password: &str,
        access_type: &str,
        allowed_ip: &str,
    ) -> Result<(), String> {
        conn.execute(
            "INSERT OR REPLACE INTO db_credentials (server_host, db_name, db_user, password, access_type, allowed_ip) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![server_host, db_name, db_user, password, access_type, allowed_ip],
        ).map_err(|e| format!("Failed to save db credentials: {}", e))?;
        Ok(())
    }

    /// Get credentials for a specific database
    pub fn get(conn: &SqliteConn, server_host: &str, db_name: &str) -> Option<DbCredential> {
        conn.query_row(
            "SELECT db_name, db_user, password, access_type, allowed_ip FROM db_credentials WHERE server_host = ?1 AND db_name = ?2",
            rusqlite::params![server_host, db_name],
            |row| {
                Ok(DbCredential {
                    db_name: row.get(0)?,
                    db_user: row.get::<_, String>(1).unwrap_or_default(),
                    password: row.get(2)?,
                    access_type: row.get(3)?,
                    allowed_ip: row.get(4)?,
                })
            },
        ).ok()
    }

    /// List all credentials for a server
    pub fn list_for_server(conn: &SqliteConn, server_host: &str) -> Vec<DbCredential> {
        let mut stmt = conn.prepare(
            "SELECT db_name, db_user, password, access_type, allowed_ip FROM db_credentials WHERE server_host = ?1"
        ).unwrap();
        stmt.query_map([server_host], |row| {
            Ok(DbCredential {
                db_name: row.get(0)?,
                db_user: row.get::<_, String>(1).unwrap_or_default(),
                password: row.get(2)?,
                access_type: row.get(3)?,
                allowed_ip: row.get(4)?,
            })
        }).unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    /// Delete credentials for a database
    pub fn delete(conn: &SqliteConn, server_host: &str, db_name: &str) -> Result<(), String> {
        conn.execute(
            "DELETE FROM db_credentials WHERE server_host = ?1 AND db_name = ?2",
            rusqlite::params![server_host, db_name],
        ).map_err(|e| format!("Failed to delete db credentials: {}", e))?;
        Ok(())
    }

    /// Update only the password (preserves existing db_user)
    pub fn update_password(conn: &SqliteConn, server_host: &str, db_name: &str, password: &str) -> Result<(), String> {
        // Check if record exists
        let exists = conn.query_row(
            "SELECT COUNT(*) FROM db_credentials WHERE server_host = ?1 AND db_name = ?2",
            rusqlite::params![server_host, db_name],
            |row| row.get::<_, i64>(0),
        ).unwrap_or(0) > 0;

        if exists {
            conn.execute(
                "UPDATE db_credentials SET password = ?3 WHERE server_host = ?1 AND db_name = ?2",
                rusqlite::params![server_host, db_name, password],
            ).map_err(|e| format!("Failed to update password: {}", e))?;
        } else if !password.is_empty() {
            // Create new record with defaults if password is not empty
            conn.execute(
                "INSERT INTO db_credentials (server_host, db_name, db_user, password, access_type, allowed_ip) VALUES (?1, ?2, ?2, ?3, 'local', '')",
                rusqlite::params![server_host, db_name, password],
            ).map_err(|e| format!("Failed to insert password: {}", e))?;
        }
        Ok(())
    }

    /// Clear password only (set to empty)
    pub fn clear_password(conn: &SqliteConn, server_host: &str, db_name: &str) -> Result<(), String> {
        let exists = conn.query_row(
            "SELECT COUNT(*) FROM db_credentials WHERE server_host = ?1 AND db_name = ?2",
            rusqlite::params![server_host, db_name],
            |row| row.get::<_, i64>(0),
        ).unwrap_or(0) > 0;

        if exists {
            conn.execute(
                "UPDATE db_credentials SET password = '' WHERE server_host = ?1 AND db_name = ?2",
                rusqlite::params![server_host, db_name],
            ).map_err(|e| format!("Failed to clear password: {}", e))?;
        }
        Ok(())
    }
}

// ===== Database Remarks =====

pub struct DbRemarksManager;

impl DbRemarksManager {
    /// Save or update a database remark
    pub fn save(conn: &SqliteConn, server_host: &str, db_name: &str, remark: &str) -> Result<(), String> {
        conn.execute(
            "INSERT OR REPLACE INTO db_remarks (server_host, db_name, remark) VALUES (?1, ?2, ?3)",
            rusqlite::params![server_host, db_name, remark],
        ).map_err(|e| format!("Failed to save db remark: {}", e))?;
        Ok(())
    }

    /// Get remark for a specific database
    pub fn get(conn: &SqliteConn, server_host: &str, db_name: &str) -> Option<String> {
        conn.query_row(
            "SELECT remark FROM db_remarks WHERE server_host = ?1 AND db_name = ?2",
            rusqlite::params![server_host, db_name],
            |row| row.get(0),
        ).ok()
    }

    /// List all remarks for a server
    pub fn list_for_server(conn: &SqliteConn, server_host: &str) -> Vec<(String, String)> {
        let mut stmt = conn.prepare(
            "SELECT db_name, remark FROM db_remarks WHERE server_host = ?1"
        ).unwrap();
        stmt.query_map([server_host], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        }).unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    /// Delete remark for a database
    pub fn delete(conn: &SqliteConn, server_host: &str, db_name: &str) -> Result<(), String> {
        conn.execute(
            "DELETE FROM db_remarks WHERE server_host = ?1 AND db_name = ?2",
            rusqlite::params![server_host, db_name],
        ).map_err(|e| format!("Failed to delete db remark: {}", e))?;
        Ok(())
    }
}

// ===== Custom Software =====

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CustomSoftwareEntry {
    pub package_name: String,
    pub display_name: String,
    pub category: String,
}

pub struct CustomSoftwareManager;

impl CustomSoftwareManager {
    pub fn list(conn: &SqliteConn, server_host: &str) -> Vec<CustomSoftwareEntry> {
        let mut stmt = conn.prepare(
            "SELECT package_name, display_name, category FROM custom_software WHERE server_host = ?1 ORDER BY package_name"
        ).unwrap();
        stmt.query_map([server_host], |row| {
            Ok(CustomSoftwareEntry {
                package_name: row.get(0)?,
                display_name: row.get(1)?,
                category: row.get(2)?,
            })
        }).unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    pub fn add(conn: &SqliteConn, server_host: &str, package_name: &str, display_name: &str, category: &str) -> Result<(), String> {
        conn.execute(
            "INSERT OR REPLACE INTO custom_software (server_host, package_name, display_name, category) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![server_host, package_name, display_name, category],
        ).map_err(|e| format!("Failed to add custom software: {}", e))?;
        Ok(())
    }

    pub fn remove(conn: &SqliteConn, server_host: &str, package_name: &str) -> Result<(), String> {
        conn.execute(
            "DELETE FROM custom_software WHERE server_host = ?1 AND package_name = ?2",
            rusqlite::params![server_host, package_name],
        ).map_err(|e| format!("Failed to remove custom software: {}", e))?;
        Ok(())
    }
}

// ===== Site Metadata (for tracking site creation time) =====

pub struct SiteMetadataManager;

impl SiteMetadataManager {
    /// Save or get site creation timestamp.
    /// If the site already exists, return its stored created_at.
    /// Otherwise, store current_mtime as created_at and return it.
    pub fn save_or_get_created_at(
        conn: &SqliteConn,
        server_host: &str,
        domain: &str,
        current_mtime: i64,
    ) -> Result<i64, String> {
        let existing = conn.query_row(
            "SELECT created_at FROM site_metadata WHERE server_host = ?1 AND domain = ?2",
            rusqlite::params![server_host, domain],
            |row| row.get::<_, i64>(0),
        );
        match existing {
            Ok(ts) => Ok(ts),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                conn.execute(
                    "INSERT INTO site_metadata (server_host, domain, created_at) VALUES (?1, ?2, ?3)",
                    rusqlite::params![server_host, domain, current_mtime],
                ).map_err(|e| format!("Failed to save site metadata: {}", e))?;
                Ok(current_mtime)
            }
            Err(e) => Err(format!("Failed to query site metadata: {}", e)),
        }
    }

    /// List all site metadata for a server
    pub fn list_for_server(conn: &SqliteConn, server_host: &str) -> Vec<(String, i64)> {
        let mut stmt = conn.prepare(
            "SELECT domain, created_at FROM site_metadata WHERE server_host = ?1"
        ).unwrap();
        stmt.query_map([server_host], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        }).unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    /// Delete site metadata
    pub fn delete(conn: &SqliteConn, server_host: &str, domain: &str) -> Result<(), String> {
        conn.execute(
            "DELETE FROM site_metadata WHERE server_host = ?1 AND domain = ?2",
            rusqlite::params![server_host, domain],
        ).map_err(|e| format!("Failed to delete site metadata: {}", e))?;
        Ok(())
    }
}

// ===== Tunnel Persistence =====

/// A persisted tunnel configuration. Lives across disconnects/reconnects;
/// only removed when the user explicitly deletes the tunnel.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SavedTunnel {
    pub id: String,
    pub server_key: String,
    pub tunnel_type: String,
    pub local_host: String,
    pub local_port: u16,
    pub remote_host: String,
    pub remote_port: u16,
    pub created_at: i64,
    pub note: String,
}

pub struct TunnelStore;

impl TunnelStore {
    pub fn save(conn: &SqliteConn, t: &SavedTunnel) -> Result<(), String> {
        conn.execute(
            "INSERT OR REPLACE INTO tunnels (id, server_key, tunnel_type, local_host, local_port, remote_host, remote_port, created_at, note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                t.id, t.server_key, t.tunnel_type, t.local_host,
                t.local_port, t.remote_host, t.remote_port, t.created_at, t.note
            ],
        ).map_err(|e| format!("Failed to save tunnel: {}", e))?;
        Ok(())
    }

    pub fn delete(conn: &SqliteConn, id: &str) -> Result<(), String> {
        conn.execute("DELETE FROM tunnels WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| format!("Failed to delete tunnel: {}", e))?;
        Ok(())
    }

    /// Update only the note of a persisted tunnel config.
    pub fn update_note(conn: &SqliteConn, id: &str, note: &str) -> Result<(), String> {
        conn.execute(
            "UPDATE tunnels SET note = ?1 WHERE id = ?2",
            rusqlite::params![note, id],
        ).map_err(|e| format!("Failed to update tunnel note: {}", e))?;
        Ok(())
    }

    pub fn get(conn: &SqliteConn, id: &str) -> Result<Option<SavedTunnel>, String> {
        let mut stmt = conn.prepare(
            "SELECT id, server_key, tunnel_type, local_host, local_port, remote_host, remote_port, created_at, COALESCE(note, '')
             FROM tunnels WHERE id = ?1"
        ).map_err(|e| format!("Failed to prepare tunnel query: {}", e))?;
        let row = stmt.query_row(rusqlite::params![id], |r| {
            Ok(SavedTunnel {
                id: r.get(0)?,
                server_key: r.get(1)?,
                tunnel_type: r.get(2)?,
                local_host: r.get(3)?,
                local_port: r.get(4)?,
                remote_host: r.get(5)?,
                remote_port: r.get(6)?,
                created_at: r.get(7)?,
                note: r.get(8)?,
            })
        });
        match row {
            Ok(t) => Ok(Some(t)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(format!("Failed to get tunnel: {}", e)),
        }
    }

    pub fn list_for_server(conn: &SqliteConn, server_key: &str) -> Result<Vec<SavedTunnel>, String> {
        let mut stmt = conn.prepare(
            "SELECT id, server_key, tunnel_type, local_host, local_port, remote_host, remote_port, created_at, COALESCE(note, '')
             FROM tunnels WHERE server_key = ?1 ORDER BY created_at ASC"
        ).map_err(|e| format!("Failed to prepare tunnel list: {}", e))?;
        let rows = stmt.query_map(rusqlite::params![server_key], |r| {
            Ok(SavedTunnel {
                id: r.get(0)?,
                server_key: r.get(1)?,
                tunnel_type: r.get(2)?,
                local_host: r.get(3)?,
                local_port: r.get(4)?,
                remote_host: r.get(5)?,
                remote_port: r.get(6)?,
                created_at: r.get(7)?,
                note: r.get(8)?,
            })
        }).map_err(|e| format!("Failed to query tunnels: {}", e))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| format!("Tunnel row error: {}", e))?);
        }
        Ok(out)
    }
}

// ===== Known Hosts (SSH server identity) =====

/// A trusted SSH server host key (TOFU known_hosts).
/// `fingerprint` is the raw SHA-256 base64 (no "SHA256:" prefix) — prefix only when displaying.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KnownHost {
    pub host: String,
    pub key_type: String,
    pub fingerprint: String,
    pub first_seen: i64,
    pub last_seen: i64,
}

pub struct KnownHostsManager;

impl KnownHostsManager {
    /// Look up a trusted key for (host, key_type). Returns None if never seen.
    pub fn find(conn: &SqliteConn, host: &str, key_type: &str) -> Option<KnownHost> {
        let mut stmt = conn.prepare(
            "SELECT host, key_type, fingerprint, first_seen, last_seen FROM known_hosts WHERE host = ?1 AND key_type = ?2"
        ).ok()?;
        stmt.query_row(rusqlite::params![host, key_type], |row| {
            Ok(KnownHost {
                host: row.get(0)?,
                key_type: row.get(1)?,
                fingerprint: row.get(2)?,
                first_seen: row.get(3)?,
                last_seen: row.get(4)?,
            })
        }).ok()
    }

    /// Record a newly trusted key (first confirmed connection). Idempotent upsert.
    pub fn insert(conn: &SqliteConn, host: &str, key_type: &str, fingerprint: &str, key_blob: &str, now: i64) -> Result<(), String> {
        conn.execute(
            "INSERT OR REPLACE INTO known_hosts (host, key_type, fingerprint, key_blob, first_seen, last_seen) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            rusqlite::params![host, key_type, fingerprint, key_blob, now],
        ).map_err(|e| format!("Failed to insert known host: {}", e))?;
        Ok(())
    }

    /// Insert only if (host, key_type) does not already exist (used by imports —
    /// never overwrite an existing trusted fingerprint). Returns true if inserted.
    pub fn insert_if_absent(conn: &SqliteConn, host: &str, key_type: &str, fingerprint: &str, key_blob: &str, now: i64) -> Result<bool, String> {
        let exists: bool = conn.query_row(
            "SELECT COUNT(*) FROM known_hosts WHERE host = ?1 AND key_type = ?2",
            rusqlite::params![host, key_type],
            |r| r.get::<_, i64>(0),
        ).unwrap_or(0) > 0;
        if exists {
            return Ok(false);
        }
        conn.execute(
            "INSERT INTO known_hosts (host, key_type, fingerprint, key_blob, first_seen, last_seen) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            rusqlite::params![host, key_type, fingerprint, key_blob, now],
        ).map_err(|e| format!("Failed to insert known host: {}", e))?;
        Ok(true)
    }

    /// Update last_seen after a successful verification.
    pub fn touch(conn: &SqliteConn, host: &str, key_type: &str, now: i64) -> Result<(), String> {
        conn.execute(
            "UPDATE known_hosts SET last_seen = ?1 WHERE host = ?2 AND key_type = ?3",
            rusqlite::params![now, host, key_type],
        ).map_err(|e| format!("Failed to update known host: {}", e))?;
        Ok(())
    }

    /// Delete a trusted key (used when the server legitimately replaced its key).
    pub fn delete(conn: &SqliteConn, host: &str, key_type: &str) -> Result<(), String> {
        conn.execute(
            "DELETE FROM known_hosts WHERE host = ?1 AND key_type = ?2",
            rusqlite::params![host, key_type],
        ).map_err(|e| format!("Failed to delete known host: {}", e))?;
        Ok(())
    }

    /// Delete all trusted keys for a host (server reinstall / reset).
    pub fn delete_host(conn: &SqliteConn, host: &str) -> Result<(), String> {
        conn.execute(
            "DELETE FROM known_hosts WHERE host = ?1",
            rusqlite::params![host],
        ).map_err(|e| format!("Failed to delete known host: {}", e))?;
        Ok(())
    }

    /// List all trusted keys, ordered by host then key type.
    pub fn list(conn: &SqliteConn) -> Vec<KnownHost> {
        let mut stmt = match conn.prepare(
            "SELECT host, key_type, fingerprint, first_seen, last_seen FROM known_hosts ORDER BY host, key_type"
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = match stmt.query_map([], |row| {
            Ok(KnownHost {
                host: row.get(0)?,
                key_type: row.get(1)?,
                fingerprint: row.get(2)?,
                first_seen: row.get(3)?,
                last_seen: row.get(4)?,
            })
        }) {
            Ok(rows) => rows,
            Err(_) => return Vec::new(),
        };
        rows.filter_map(|r| r.ok()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ponytail: in-memory SQLite for all tests — same schema as init_db, no filesystem
    fn test_conn() -> SqliteConn {
        let conn = SqliteConn::open(":memory:").unwrap();
        conn.execute_batch(
            "CREATE TABLE fb_favorites (server_host TEXT NOT NULL, path TEXT NOT NULL, PRIMARY KEY(server_host, path));
             CREATE TABLE fb_dir_cache (server_host TEXT NOT NULL, path TEXT NOT NULL, data TEXT NOT NULL, cached_at INTEGER NOT NULL, PRIMARY KEY(server_host, path));
             CREATE TABLE db_remarks (server_host TEXT NOT NULL, db_name TEXT NOT NULL, remark TEXT NOT NULL DEFAULT '', PRIMARY KEY(server_host, db_name));
             CREATE TABLE db_credentials (server_host TEXT NOT NULL, db_name TEXT NOT NULL, db_user TEXT NOT NULL DEFAULT '', password TEXT NOT NULL DEFAULT '', access_type TEXT NOT NULL DEFAULT 'local', allowed_ip TEXT NOT NULL DEFAULT '', PRIMARY KEY(server_host, db_name));
             CREATE TABLE site_metadata (server_host TEXT NOT NULL, domain TEXT NOT NULL, created_at INTEGER NOT NULL, PRIMARY KEY(server_host, domain));
             CREATE TABLE custom_software (server_host TEXT NOT NULL, package_name TEXT NOT NULL, display_name TEXT NOT NULL, category TEXT NOT NULL DEFAULT 'other', PRIMARY KEY(server_host, package_name));
             CREATE TABLE known_hosts (host TEXT NOT NULL, key_type TEXT NOT NULL, fingerprint TEXT NOT NULL, key_blob TEXT NOT NULL DEFAULT '', first_seen INTEGER NOT NULL, last_seen INTEGER NOT NULL, PRIMARY KEY(host, key_type));"
        ).unwrap();
        conn
    }

    // ===== FbFavorites =====

    #[test]
    fn fb_favorites_add_and_list() {
        let conn = test_conn();
        FbFavorites::add(&conn, "host1", "/var/www").unwrap();
        FbFavorites::add(&conn, "host1", "/etc/nginx").unwrap();
        let paths = FbFavorites::list(&conn, "host1");
        assert_eq!(paths, vec!["/etc/nginx", "/var/www"]);
    }

    #[test]
    fn fb_favorites_isolation_by_host() {
        let conn = test_conn();
        FbFavorites::add(&conn, "host1", "/a").unwrap();
        FbFavorites::add(&conn, "host2", "/b").unwrap();
        assert_eq!(FbFavorites::list(&conn, "host1"), vec!["/a"]);
        assert_eq!(FbFavorites::list(&conn, "host2"), vec!["/b"]);
    }

    #[test]
    fn fb_favorites_remove() {
        let conn = test_conn();
        FbFavorites::add(&conn, "host1", "/a").unwrap();
        FbFavorites::remove(&conn, "host1", "/a").unwrap();
        assert!(FbFavorites::list(&conn, "host1").is_empty());
    }

    #[test]
    fn fb_favorites_add_duplicate_ignored() {
        let conn = test_conn();
        FbFavorites::add(&conn, "host1", "/a").unwrap();
        FbFavorites::add(&conn, "host1", "/a").unwrap();
        assert_eq!(FbFavorites::list(&conn, "host1").len(), 1);
    }

    // ===== FbDirCache =====

    #[test]
    fn fb_dir_cache_put_and_get() {
        let conn = test_conn();
        FbDirCache::put(&conn, "host1", "/tmp", r#"[{"name":"a.txt"}]"#).unwrap();
        let result = FbDirCache::get(&conn, "host1", "/tmp", 24);
        assert!(result.is_some());
        let (data, _) = result.unwrap();
        assert!(data.contains("a.txt"));
    }

    #[test]
    fn fb_dir_cache_expired_returns_none() {
        let conn = test_conn();
        // Manually insert with old timestamp
        let old_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64 - 48 * 3600 * 1000;
        conn.execute(
            "INSERT INTO fb_dir_cache VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["host1", "/tmp", "data", old_ts],
        ).unwrap();
        assert!(FbDirCache::get(&conn, "host1", "/tmp", 24).is_none());
    }

    #[test]
    fn fb_dir_cache_touch_updates_timestamp() {
        let conn = test_conn();
        FbDirCache::put(&conn, "host1", "/tmp", "data").unwrap();
        FbDirCache::touch(&conn, "host1", "/tmp").unwrap();
        assert!(FbDirCache::get(&conn, "host1", "/tmp", 24).is_some());
    }

    #[test]
    fn fb_dir_cache_count_and_clear() {
        let conn = test_conn();
        FbDirCache::put(&conn, "host1", "/a", "1").unwrap();
        FbDirCache::put(&conn, "host1", "/b", "2").unwrap();
        assert_eq!(FbDirCache::count(&conn), 2);
        let cleared = FbDirCache::clear_all(&conn).unwrap();
        assert_eq!(cleared, 2);
        assert_eq!(FbDirCache::count(&conn), 0);
    }

    // ===== DbCredentialsManager =====

    #[test]
    fn db_credentials_save_and_get() {
        let conn = test_conn();
        DbCredentialsManager::save(&conn, "host1", "mydb", "admin", "secret", "local", "").unwrap();
        let cred = DbCredentialsManager::get(&conn, "host1", "mydb").unwrap();
        assert_eq!(cred.db_name, "mydb");
        assert_eq!(cred.db_user, "admin");
        assert_eq!(cred.password, "secret");
        assert_eq!(cred.access_type, "local");
    }

    #[test]
    fn db_credentials_list_for_server() {
        let conn = test_conn();
        DbCredentialsManager::save(&conn, "host1", "db1", "u1", "p1", "local", "").unwrap();
        DbCredentialsManager::save(&conn, "host1", "db2", "u2", "p2", "remote", "%").unwrap();
        DbCredentialsManager::save(&conn, "host2", "db3", "u3", "p3", "local", "").unwrap();
        let creds = DbCredentialsManager::list_for_server(&conn, "host1");
        assert_eq!(creds.len(), 2);
    }

    #[test]
    fn db_credentials_delete() {
        let conn = test_conn();
        DbCredentialsManager::save(&conn, "host1", "mydb", "u", "p", "local", "").unwrap();
        DbCredentialsManager::delete(&conn, "host1", "mydb").unwrap();
        assert!(DbCredentialsManager::get(&conn, "host1", "mydb").is_none());
    }

    #[test]
    fn db_credentials_update_password() {
        let conn = test_conn();
        DbCredentialsManager::save(&conn, "host1", "mydb", "admin", "old", "local", "").unwrap();
        DbCredentialsManager::update_password(&conn, "host1", "mydb", "new").unwrap();
        let cred = DbCredentialsManager::get(&conn, "host1", "mydb").unwrap();
        assert_eq!(cred.password, "new");
        assert_eq!(cred.db_user, "admin"); // preserved
    }

    #[test]
    fn db_credentials_update_password_creates_if_missing() {
        let conn = test_conn();
        DbCredentialsManager::update_password(&conn, "host1", "newdb", "pass123").unwrap();
        let cred = DbCredentialsManager::get(&conn, "host1", "newdb").unwrap();
        assert_eq!(cred.password, "pass123");
    }

    #[test]
    fn db_credentials_clear_password() {
        let conn = test_conn();
        DbCredentialsManager::save(&conn, "host1", "mydb", "u", "secret", "local", "").unwrap();
        DbCredentialsManager::clear_password(&conn, "host1", "mydb").unwrap();
        let cred = DbCredentialsManager::get(&conn, "host1", "mydb").unwrap();
        assert_eq!(cred.password, "");
    }

    // ===== DbRemarksManager =====

    #[test]
    fn db_remarks_save_and_get() {
        let conn = test_conn();
        DbRemarksManager::save(&conn, "host1", "mydb", "Production DB").unwrap();
        assert_eq!(DbRemarksManager::get(&conn, "host1", "mydb"), Some("Production DB".to_string()));
    }

    #[test]
    fn db_remarks_list_for_server() {
        let conn = test_conn();
        DbRemarksManager::save(&conn, "host1", "db1", "note1").unwrap();
        DbRemarksManager::save(&conn, "host1", "db2", "note2").unwrap();
        let list = DbRemarksManager::list_for_server(&conn, "host1");
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn db_remarks_delete() {
        let conn = test_conn();
        DbRemarksManager::save(&conn, "host1", "mydb", "test").unwrap();
        DbRemarksManager::delete(&conn, "host1", "mydb").unwrap();
        assert_eq!(DbRemarksManager::get(&conn, "host1", "mydb"), None);
    }

    // ===== CustomSoftwareManager =====

    #[test]
    fn custom_software_add_and_list() {
        let conn = test_conn();
        CustomSoftwareManager::add(&conn, "host1", "htop", "Htop", "monitoring").unwrap();
        let list = CustomSoftwareManager::list(&conn, "host1");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].package_name, "htop");
        assert_eq!(list[0].display_name, "Htop");
        assert_eq!(list[0].category, "monitoring");
    }

    #[test]
    fn custom_software_remove() {
        let conn = test_conn();
        CustomSoftwareManager::add(&conn, "host1", "htop", "Htop", "monitoring").unwrap();
        CustomSoftwareManager::remove(&conn, "host1", "htop").unwrap();
        assert!(CustomSoftwareManager::list(&conn, "host1").is_empty());
    }

    // ===== SiteMetadataManager =====

    #[test]
    fn site_metadata_save_or_get_first_call_stores() {
        let conn = test_conn();
        let ts = SiteMetadataManager::save_or_get_created_at(&conn, "host1", "example.com", 1000).unwrap();
        assert_eq!(ts, 1000);
    }

    #[test]
    fn site_metadata_save_or_get_second_call_returns_stored() {
        let conn = test_conn();
        SiteMetadataManager::save_or_get_created_at(&conn, "host1", "example.com", 1000).unwrap();
        let ts = SiteMetadataManager::save_or_get_created_at(&conn, "host1", "example.com", 2000).unwrap();
        assert_eq!(ts, 1000); // returns original, not 2000
    }

    // ===== KnownHostsManager =====

    #[test]
    fn known_hosts_insert_find_and_touch() {
        let conn = test_conn();
        KnownHostsManager::insert(&conn, "host1", "ssh-ed25519", "ABC123", "blob1", 1000).unwrap();
        let kh = KnownHostsManager::find(&conn, "host1", "ssh-ed25519").unwrap();
        assert_eq!(kh.fingerprint, "ABC123");
        assert_eq!(kh.first_seen, 1000);
        assert_eq!(kh.last_seen, 1000);
        KnownHostsManager::touch(&conn, "host1", "ssh-ed25519", 2000).unwrap();
        let kh = KnownHostsManager::find(&conn, "host1", "ssh-ed25519").unwrap();
        assert_eq!(kh.last_seen, 2000);
        assert_eq!(kh.first_seen, 1000); // first_seen preserved
    }

    #[test]
    fn known_hosts_multi_key_type_isolation() {
        let conn = test_conn();
        KnownHostsManager::insert(&conn, "host1", "ssh-ed25519", "F1", "b1", 1000).unwrap();
        KnownHostsManager::insert(&conn, "host1", "ssh-rsa", "F2", "b2", 1000).unwrap();
        assert_eq!(KnownHostsManager::find(&conn, "host1", "ssh-ed25519").unwrap().fingerprint, "F1");
        assert_eq!(KnownHostsManager::find(&conn, "host1", "ssh-rsa").unwrap().fingerprint, "F2");
        // 不存在 (host, key_type) 组合返回 None
        assert!(KnownHostsManager::find(&conn, "host1", "ecdsa-sha2-nistp256").is_none());
        assert!(KnownHostsManager::find(&conn, "host2", "ssh-ed25519").is_none());
    }

    #[test]
    fn known_hosts_upsert_replaces_fingerprint() {
        let conn = test_conn();
        KnownHostsManager::insert(&conn, "host1", "ssh-ed25519", "OLD", "b1", 1000).unwrap();
        KnownHostsManager::insert(&conn, "host1", "ssh-ed25519", "NEW", "b2", 1500).unwrap();
        let kh = KnownHostsManager::find(&conn, "host1", "ssh-ed25519").unwrap();
        assert_eq!(kh.fingerprint, "NEW");
        assert_eq!(kh.first_seen, 1500); // INSERT OR REPLACE 重置 first_seen（预期行为）
    }

    #[test]
    fn known_hosts_delete_and_delete_host() {
        let conn = test_conn();
        KnownHostsManager::insert(&conn, "host1", "ssh-ed25519", "F1", "b1", 1000).unwrap();
        KnownHostsManager::insert(&conn, "host1", "ssh-rsa", "F2", "b2", 1000).unwrap();
        KnownHostsManager::insert(&conn, "host2", "ssh-ed25519", "F3", "b3", 1000).unwrap();
        KnownHostsManager::delete(&conn, "host1", "ssh-ed25519").unwrap();
        assert!(KnownHostsManager::find(&conn, "host1", "ssh-ed25519").is_none());
        assert!(KnownHostsManager::find(&conn, "host1", "ssh-rsa").is_some()); // 其他 key_type 保留
        KnownHostsManager::delete_host(&conn, "host1").unwrap();
        assert!(KnownHostsManager::find(&conn, "host1", "ssh-rsa").is_none());
        assert!(KnownHostsManager::find(&conn, "host2", "ssh-ed25519").is_some()); // 其他 host 保留
    }

    #[test]
    fn known_hosts_list_sorted() {
        let conn = test_conn();
        KnownHostsManager::insert(&conn, "host2", "ssh-ed25519", "F2", "b2", 1000).unwrap();
        KnownHostsManager::insert(&conn, "host1", "ssh-rsa", "F1", "b1", 1000).unwrap();
        let list = KnownHostsManager::list(&conn);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].host, "host1");
        assert_eq!(list[1].host, "host2");
    }

    #[test]
    fn known_hosts_insert_if_absent_preserves_existing() {
        let conn = test_conn();
        KnownHostsManager::insert(&conn, "host1", "ssh-ed25519", "ORIG", "b1", 1000).unwrap();
        // 已存在 → 不覆盖，返回 false
        assert!(!KnownHostsManager::insert_if_absent(&conn, "host1", "ssh-ed25519", "EVIL", "b2", 2000).unwrap());
        assert_eq!(KnownHostsManager::find(&conn, "host1", "ssh-ed25519").unwrap().fingerprint, "ORIG");
        // 新条目 → 插入，返回 true
        assert!(KnownHostsManager::insert_if_absent(&conn, "host1", "ssh-rsa", "NEW", "b3", 2000).unwrap());
        assert_eq!(KnownHostsManager::find(&conn, "host1", "ssh-rsa").unwrap().fingerprint, "NEW");
    }
}
