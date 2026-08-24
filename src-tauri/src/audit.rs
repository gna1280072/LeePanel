//! 审计日志模块（P2）—— 记录用户对服务器的管理操作。
//!
//! 设计：
//! - 只记录"管理/写操作"（由各 Tauri command 显式调用），不记录状态轮询等只读查询；
//! - 记录：时间 / 目标服务器(host, username) / 操作类型 / 实际命令 / 结果状态 / 详情；
//! - command 字段仅记录无敏感信息的命令（接入点均为管理操作，不含密码参数）。

use rusqlite::{Connection as SqliteConn, params};
use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
pub struct AuditEntry {
    pub id: i64,
    pub ts: i64,
    pub server_host: String,
    pub server_username: String,
    pub op: String,
    pub command: String,
    pub result: String,
    pub detail: String,
}

/// 写入一条审计记录。写入失败不向上抛错（审计不能阻断管理操作），仅记日志。
pub fn audit_log(
    db: &SqliteConn,
    server_host: &str,
    server_username: &str,
    op: &str,
    command: &str,
    result: &str,
    detail: &str,
) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    if let Err(e) = db.execute(
        "INSERT INTO op_log (ts, server_host, server_username, op, command, result, detail) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![ts, server_host, server_username, op, command, result, detail],
    ) {
        log::warn!("Audit log write failed: {}", e);
    }
}

/// 查询最近 N 条审计记录（倒序）。
pub fn audit_list(db: &SqliteConn, limit: i64) -> Vec<AuditEntry> {
    let limit = limit.clamp(1, 500);
    let mut stmt = match db.prepare(
        "SELECT id, ts, server_host, server_username, op, command, result, detail FROM op_log ORDER BY id DESC LIMIT ?1"
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    stmt.query_map(params![limit], |row| {
        Ok(AuditEntry {
            id: row.get(0)?,
            ts: row.get(1)?,
            server_host: row.get(2)?,
            server_username: row.get(3)?,
            op: row.get(4)?,
            command: row.get(5)?,
            result: row.get(6)?,
            detail: row.get(7)?,
        })
    })
    .map(|rows| rows.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}

/// 清空全部审计记录，返回删除条数。
pub fn audit_clear(db: &SqliteConn) -> usize {
    db.execute("DELETE FROM op_log", [])
        .map(|n| n as usize)
        .unwrap_or(0)
}

/// 审计记录总数（供 UI 显示）。
pub fn audit_count(db: &SqliteConn) -> i64 {
    db.query_row("SELECT COUNT(*) FROM op_log", [], |r| r.get::<_, i64>(0))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> SqliteConn {
        let conn = SqliteConn::open(":memory:").unwrap();
        conn.execute_batch(
            "CREATE TABLE op_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                ts INTEGER NOT NULL,
                server_host TEXT NOT NULL,
                server_username TEXT NOT NULL,
                op TEXT NOT NULL,
                command TEXT NOT NULL,
                result TEXT NOT NULL DEFAULT 'success',
                detail TEXT NOT NULL DEFAULT ''
            );"
        ).unwrap();
        conn
    }

    #[test]
    fn log_then_list_newest_first() {
        let conn = test_conn();
        audit_log(&conn, "1.2.3.4", "root", "service_action", "systemctl restart nginx", "success", "");
        audit_log(&conn, "1.2.3.4", "root", "port_kill", "kill -9 1234", "error", "Permission denied");
        let list = audit_list(&conn, 10);
        assert_eq!(list.len(), 2);
        // 倒序：最新的在前
        assert_eq!(list[0].op, "port_kill");
        assert_eq!(list[0].result, "error");
        assert_eq!(list[1].op, "service_action");
        assert_eq!(list[0].command, "kill -9 1234");
    }

    #[test]
    fn list_respects_limit() {
        let conn = test_conn();
        for i in 0..5 {
            audit_log(&conn, "h", "u", "op", &format!("cmd {}", i), "success", "");
        }
        assert_eq!(audit_list(&conn, 3).len(), 3);
        assert_eq!(audit_list(&conn, 0).len(), 1); // clamp 下限
        assert_eq!(audit_list(&conn, 9999).len(), 5); // clamp 上限
    }

    #[test]
    fn clear_removes_all() {
        let conn = test_conn();
        audit_log(&conn, "h", "u", "op", "cmd", "success", "");
        audit_log(&conn, "h", "u", "op", "cmd2", "success", "");
        assert_eq!(audit_count(&conn), 2);
        assert_eq!(audit_clear(&conn), 2);
        assert_eq!(audit_count(&conn), 0);
    }
}
