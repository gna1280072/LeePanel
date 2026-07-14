use rusqlite::Connection as SqliteConn;
use rusqlite::params;
use serde::{Deserialize, Serialize};

// ===== Connection =====

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Connection {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(default)]
    pub remember_me: bool,
}

pub struct ConfigManager;

impl ConfigManager {
    pub fn list(conn: &SqliteConn) -> Vec<Connection> {
        let mut stmt = conn
            .prepare("SELECT id, name, host, port, username, auth_type, key_path, password, remember_me FROM connections ORDER BY name")
            .expect("prepare connections list");
        stmt.query_map([], |row| {
            Ok(Connection {
                id: row.get(0)?,
                name: row.get(1)?,
                host: row.get(2)?,
                port: row.get::<_, i64>(3)? as u16,
                username: row.get(4)?,
                auth_type: row.get(5)?,
                key_path: row.get(6)?,
                password: row.get(7)?,
                remember_me: row.get::<_, Option<i64>>(8)?.map(|v| v == 1).unwrap_or(false),
            })
        })
        .expect("query connections")
        .filter_map(|r| r.ok())
        .collect()
    }

    pub fn save(conn: &SqliteConn, c: &Connection) -> Result<(), String> {
        conn.execute(
            "INSERT OR REPLACE INTO connections (id, name, host, port, username, auth_type, key_path, password, remember_me) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![c.id, c.name, c.host, c.port as i64, c.username, c.auth_type, c.key_path, c.password, if c.remember_me { 1 } else { 0 }],
        ).map_err(|e| format!("Save connection failed: {}", e))?;
        Ok(())
    }

    pub fn delete(conn: &SqliteConn, id: &str) -> Result<(), String> {
        conn.execute("DELETE FROM connections WHERE id = ?1", params![id])
            .map_err(|e| format!("Delete connection failed: {}", e))?;
        Ok(())
    }

    pub fn save_credentials(
        conn: &SqliteConn,
        id: &str,
        username: &str,
        auth_type: &str,
        key_path: Option<&str>,
        password: Option<&str>,
        remember_me: bool,
    ) -> Result<(), String> {
        println!("=== DEBUG save_credentials ===");
        println!("id: {}", id);
        println!("username: {}", username);
        println!("auth_type: {}", auth_type);
        println!("key_path: {:?}", key_path);
        println!("password: {:?}", password.as_ref().map(|_| "***"));
        println!("remember_me: {}", remember_me);
        
        let sql = "UPDATE connections SET username = ?1, auth_type = ?2, key_path = ?3, password = ?4, remember_me = ?5 WHERE id = ?6";
        println!("SQL: {}", sql);
        
        let result = conn.execute(
            sql,
            params![username, auth_type, key_path, password, if remember_me { 1 } else { 0 }, id],
        );
        
        match result {
            Ok(rows_affected) => {
                println!("Rows affected: {}", rows_affected);
                
                // Verify the update by reading back
                let mut stmt = conn.prepare("SELECT username, auth_type, key_path, password, remember_me FROM connections WHERE id = ?1")
                    .map_err(|e| format!("Prepare select failed: {}", e))?;
                
                let row_result = stmt.query_row(params![id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, i64>(4)? == 1,
                    ))
                });
                
                match row_result {
                    Ok((db_username, db_auth_type, db_key_path, db_password, db_remember)) => {
                        println!("After update - username: {}, auth_type: {}, key_path: {:?}, password: {:?}, remember_me: {}",
                            db_username, db_auth_type, db_key_path, db_password.as_ref().map(|_| "***"), db_remember);
                    }
                    Err(e) => {
                        println!("Failed to verify update: {}", e);
                    }
                }
                
                Ok(())
            }
            Err(e) => {
                println!("Error executing UPDATE: {}", e);
                Err(format!("Save credentials failed: {}", e))
            }
        }
    }
}

// ===== Favorite =====

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Favorite {
    pub path: String,
    pub name: String,
}

pub struct FavoritesManager;

impl FavoritesManager {
    pub fn list(conn: &SqliteConn) -> Vec<Favorite> {
        let mut stmt = conn
            .prepare("SELECT path, name FROM favorites ORDER BY name")
            .expect("prepare favorites list");
        stmt.query_map([], |row| {
            Ok(Favorite {
                path: row.get(0)?,
                name: row.get(1)?,
            })
        })
        .expect("query favorites")
        .filter_map(|r| r.ok())
        .collect()
    }

    pub fn add(conn: &SqliteConn, fav: &Favorite) -> Result<(), String> {
        conn.execute(
            "INSERT OR IGNORE INTO favorites (path, name) VALUES (?1, ?2)",
            params![fav.path, fav.name],
        ).map_err(|e| format!("Add favorite failed: {}", e))?;
        Ok(())
    }

    pub fn remove(conn: &SqliteConn, path: &str) -> Result<(), String> {
        conn.execute("DELETE FROM favorites WHERE path = ?1", params![path])
            .map_err(|e| format!("Delete favorite failed: {}", e))?;
        Ok(())
    }
}

// ===== Settings =====

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    #[serde(default = "default_true")]
    pub auto_reconnect: bool,
    #[serde(default = "default_reconnect_interval")]
    pub reconnect_interval: u32,
    #[serde(default = "default_max_attempts")]
    pub max_reconnect_attempts: u32,
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl_hours: u32,
    #[serde(default = "default_cache_max_files")]
    pub cache_max_files: u32,
    #[serde(default = "default_true")]
    pub cache_enabled: bool,
    #[serde(default = "default_command_timeout")]
    pub command_timeout_minutes: u32,
    #[serde(default = "default_upload_workers")]
    pub upload_workers: u32,
}

fn default_true() -> bool { true }
fn default_reconnect_interval() -> u32 { 5 }
fn default_max_attempts() -> u32 { 10 }
fn default_cache_ttl() -> u32 { 24 }
fn default_cache_max_files() -> u32 { 500 }
fn default_command_timeout() -> u32 { 30 }
fn default_upload_workers() -> u32 { 3 }

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_reconnect: true,
            reconnect_interval: 5,
            max_reconnect_attempts: 10,
            cache_ttl_hours: 24,
            cache_max_files: 500,
            cache_enabled: true,
            command_timeout_minutes: 30,
            upload_workers: 3,
        }
    }
}

pub struct SettingsManager;

impl SettingsManager {
    pub fn load(conn: &SqliteConn) -> Settings {
        let mut settings = Settings::default();

        if let Ok(val) = Self::get(conn, "auto_reconnect") {
            if let Ok(v) = val.parse::<bool>() { settings.auto_reconnect = v; }
        }
        if let Ok(val) = Self::get(conn, "reconnect_interval") {
            if let Ok(v) = val.parse::<u32>() { settings.reconnect_interval = v; }
        }
        if let Ok(val) = Self::get(conn, "max_reconnect_attempts") {
            if let Ok(v) = val.parse::<u32>() { settings.max_reconnect_attempts = v; }
        }
        if let Ok(val) = Self::get(conn, "cache_ttl_hours") {
            if let Ok(v) = val.parse::<u32>() { settings.cache_ttl_hours = v; }
        }
        if let Ok(val) = Self::get(conn, "cache_max_files") {
            if let Ok(v) = val.parse::<u32>() { settings.cache_max_files = v; }
        }
        if let Ok(val) = Self::get(conn, "cache_enabled") {
            if let Ok(v) = val.parse::<bool>() { settings.cache_enabled = v; }
        }
        if let Ok(val) = Self::get(conn, "command_timeout_minutes") {
            if let Ok(v) = val.parse::<u32>() { settings.command_timeout_minutes = v; }
        }
        if let Ok(val) = Self::get(conn, "upload_workers") {
            if let Ok(v) = val.parse::<u32>() { settings.upload_workers = v; }
        }

        settings
    }

    pub fn save(conn: &SqliteConn, settings: &Settings) -> Result<(), String> {
        Self::set(conn, "auto_reconnect", &settings.auto_reconnect.to_string())?;
        Self::set(conn, "reconnect_interval", &settings.reconnect_interval.to_string())?;
        Self::set(conn, "max_reconnect_attempts", &settings.max_reconnect_attempts.to_string())?;
        Self::set(conn, "cache_ttl_hours", &settings.cache_ttl_hours.to_string())?;
        Self::set(conn, "cache_max_files", &settings.cache_max_files.to_string())?;
        Self::set(conn, "cache_enabled", &settings.cache_enabled.to_string())?;
        Self::set(conn, "command_timeout_minutes", &settings.command_timeout_minutes.to_string())?;
        Self::set(conn, "upload_workers", &settings.upload_workers.to_string())?;
        Ok(())
    }

    fn get(conn: &SqliteConn, key: &str) -> Result<String, String> {
        conn.query_row("SELECT value FROM settings WHERE key = ?1", params![key], |row| row.get(0))
            .map_err(|e| format!("Get setting failed: {}", e))
    }

    fn set(conn: &SqliteConn, key: &str, value: &str) -> Result<(), String> {
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            params![key, value],
        ).map_err(|e| format!("Set setting failed: {}", e))?;
        Ok(())
    }
}
