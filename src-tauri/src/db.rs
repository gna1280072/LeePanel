use rusqlite::Connection as SqliteConn;
use std::path::PathBuf;
use std::sync::Mutex;

/// Get the SQLite database path: <config_dir>/leepanel/data.db
pub fn db_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("leepanel");
    std::fs::create_dir_all(&path).ok();
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
        );"
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

    // Update schema version to latest
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('schema_version', '3')",
        [],
    ).map_err(|e| format!("Failed to update schema_version: {}", e))?;

    Ok(Mutex::new(conn))
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
             CREATE TABLE custom_software (server_host TEXT NOT NULL, package_name TEXT NOT NULL, display_name TEXT NOT NULL, category TEXT NOT NULL DEFAULT 'other', PRIMARY KEY(server_host, package_name));"
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

    #[test]
    fn site_metadata_list_and_delete() {
        let conn = test_conn();
        SiteMetadataManager::save_or_get_created_at(&conn, "host1", "a.com", 100).unwrap();
        SiteMetadataManager::save_or_get_created_at(&conn, "host1", "b.com", 200).unwrap();
        let list = SiteMetadataManager::list_for_server(&conn, "host1");
        assert_eq!(list.len(), 2);
        SiteMetadataManager::delete(&conn, "host1", "a.com").unwrap();
        let list = SiteMetadataManager::list_for_server(&conn, "host1");
        assert_eq!(list.len(), 1);
    }
}
