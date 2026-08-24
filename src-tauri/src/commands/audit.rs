//! 审计日志 Tauri commands —— 前端查询/清空操作日志。

use crate::audit::{self, AuditEntry};
use crate::DbPool;

/// 查询最近 N 条审计记录（倒序）。
#[tauri::command]
pub fn audit_list(db: tauri::State<'_, DbPool>, limit: i64) -> Vec<AuditEntry> {
    let conn = db.lock().unwrap();
    audit::audit_list(&conn, limit)
}

/// 清空全部审计记录，返回删除条数。
#[tauri::command]
pub fn audit_clear(db: tauri::State<'_, DbPool>) -> usize {
    let conn = db.lock().unwrap();
    audit::audit_clear(&conn)
}

/// 审计记录总数。
#[tauri::command]
pub fn audit_count(db: tauri::State<'_, DbPool>) -> i64 {
    let conn = db.lock().unwrap();
    audit::audit_count(&conn)
}
