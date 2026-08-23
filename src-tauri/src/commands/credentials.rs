//! 凭据 Tauri commands —— 前端通过 IPC 读写系统钥匙串。
//!
//! 安全约定：明文凭据仅在"保存表单提交"与"显示密码（按需）"两个场景
//! 短暂进出前端；SSH 连接链路由 Rust 端内部读取，前端不接触明文。

use crate::credentials::{self, CredKind};

fn parse_kind(s: &str) -> Result<CredKind, String> {
    match s {
        "password" => Ok(CredKind::Password),
        "passphrase" => Ok(CredKind::Passphrase),
        _ => Err(format!("Unknown credential kind: {}", s)),
    }
}

/// 保存单条凭据到系统钥匙串。
#[tauri::command]
pub fn credential_set(config_id: String, kind: String, secret: String) -> Result<(), String> {
    let kind = parse_kind(&kind)?;
    credentials::store_set(&config_id, kind, &secret)
}

/// 按需读取单条凭据（编辑表单"显示密码"、连接链路）。不存在返回 null。
#[tauri::command]
pub fn credential_get(config_id: String, kind: String) -> Result<Option<String>, String> {
    let kind = parse_kind(&kind)?;
    credentials::store_get(&config_id, kind)
}

/// 删除某连接在钥匙串中的全部凭据（连接删除时级联清理）。
#[tauri::command]
pub fn credential_delete(config_id: String) -> Result<(), String> {
    credentials::store_delete(&config_id)
}

/// 探测系统钥匙串可用性（Linux 无 D-Bus Secret Service 时返回 false）。
#[tauri::command]
pub fn credential_available() -> bool {
    credentials::store_available()
}

/// 返回启动迁移时实际搬入钥匙串的凭据条数（迁移完成前为 0）。
/// 前端据此在首次启动时展示"已迁移 N 条凭据"提示。
#[tauri::command]
pub fn credential_migration_count(db: tauri::State<'_, std::sync::Mutex<crate::db::SqliteConn>>) -> Result<u32, String> {
    let conn = db.lock().unwrap();
    conn.query_row(
        "SELECT value FROM settings WHERE key = 'credential_migration_count'",
        [],
        |r| r.get::<_, String>(0),
    )
    .map(|v| v.parse::<u32>().unwrap_or(0))
    .or_else(|_| Ok(0u32))
}
