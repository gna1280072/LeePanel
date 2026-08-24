use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;
use tauri::State;

use crate::audit;
use crate::ssh::SshManager;
use crate::server::{PortInfo, list_listening_ports, query_port, kill_pid};
use crate::DbPool;

// ===== Port Management Commands =====

/// List all listening ports on the remote server.
#[tauri::command]
pub async fn port_list(
    ssh_mgr: State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
) -> Result<Vec<PortInfo>, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    list_listening_ports(&session, &cache, session_id).await
}

/// Query a specific port's usage on the remote server.
#[tauri::command]
pub async fn port_query(
    ssh_mgr: State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: &str,
    port: u16,
) -> Result<Vec<PortInfo>, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    drop(mgr);
    query_port(&session, &cache, session_id, port).await
}

/// Kill a process by PID on the remote server.
#[tauri::command]
pub async fn port_kill(
    ssh_mgr: State<'_, Arc<AsyncMutex<SshManager>>>,
    db: State<'_, DbPool>,
    session_id: &str,
    pid: i32,
    force: bool,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(session_id)?;
    let cache = mgr.cache.clone();
    let info = session.connect_info.clone();
    drop(mgr);
    let result = kill_pid(&session, pid, force).await;
    // Port usage may have changed — invalidate the cache
    cache.invalidate(session_id, &["ports"]);
    // 审计：进程终止（破坏性操作）
    if let Ok(conn) = db.lock() {
        let sig = if force { "-9" } else { "-15" };
        audit::audit_log(
            &conn, &info.host, &info.username, "port_kill",
            &format!("kill {} {}", sig, pid),
            if result.is_ok() { "success" } else { "error" },
            &result.as_ref().err().cloned().unwrap_or_default(),
        );
    }
    result
}
