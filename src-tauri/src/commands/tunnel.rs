use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;
use tauri::State;

use crate::db::{SavedTunnel, TunnelStore};
use crate::ssh::{SshManager, SshSession};
use crate::tunnel::{TunnelConfig, TunnelInfo, TunnelManager, TunnelType};
use crate::DbPool;

/// Stable identity for a server across sessions: username@host:port.
fn server_key_of(session: &SshSession) -> String {
    let ci = &session.connect_info;
    format!("{}@{}:{}", ci.username, ci.host, ci.port)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn parse_tunnel_type(s: &str) -> Result<TunnelType, String> {
    match s.to_lowercase().as_str() {
        "local" => Ok(TunnelType::Local),
        "remote" => Ok(TunnelType::Remote),
        "dynamic" => Ok(TunnelType::Dynamic),
        _ => Err(format!("Invalid tunnel type: {}", s)),
    }
}

#[tauri::command]
pub async fn tunnel_create(
    ssh_mgr: State<'_, Arc<AsyncMutex<SshManager>>>,
    tunnel_mgr: State<'_, Arc<AsyncMutex<TunnelManager>>>,
    db: State<'_, DbPool>,
    app: tauri::AppHandle,
    session_id: String,
    tunnel_type: String,
    local_host: String,
    local_port: u16,
    remote_host: String,
    remote_port: u16,
    note: String,
) -> Result<String, String> {
    // Get the SSH session
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(&session_id)?;
    let server_key = server_key_of(&session);
    drop(mgr);

    let tt = parse_tunnel_type(&tunnel_type)?;

    let config = TunnelConfig {
        tunnel_type: tt,
        local_host: local_host.clone(),
        local_port,
        remote_host: remote_host.clone(),
        remote_port,
        note: note.clone(),
    };

    let tunnel_id = uuid::Uuid::new_v4().to_string();
    {
        let tunnel_mgr = tunnel_mgr.lock().await;
        tunnel_mgr
            .create_tunnel(tunnel_id.clone(), session_id, session, config, app, None)
            .await?;
    }

    // Persist the configuration so it survives disconnects / app restarts.
    let conn = db.lock().map_err(|e| format!("DB lock failed: {}", e))?;
    TunnelStore::save(&conn, &SavedTunnel {
        id: tunnel_id.clone(),
        server_key,
        tunnel_type,
        local_host,
        local_port,
        remote_host,
        remote_port,
        created_at: now_ms(),
        note,
    })?;
    drop(conn);

    Ok(tunnel_id)
}

#[tauri::command]
pub async fn tunnel_close(
    tunnel_mgr: State<'_, Arc<AsyncMutex<TunnelManager>>>,
    tunnel_id: String,
) -> Result<(), String> {
    // Stop the running tunnel only; the persisted config stays (user can restore).
    let tunnel_mgr = tunnel_mgr.lock().await;
    tunnel_mgr.close_tunnel(&tunnel_id).await
}

#[tauri::command]
pub async fn tunnel_close_batch(
    tunnel_mgr: State<'_, Arc<AsyncMutex<TunnelManager>>>,
    ids: Vec<String>,
) -> Result<(), String> {
    // Stop each running tunnel only; persisted configs stay (user can restore).
    // Block scopes keep the non-Send MutexGuard out of any await point.
    for id in &ids {
        {
            let tunnel_mgr = tunnel_mgr.lock().await;
            tunnel_mgr.close_tunnel(id).await?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn tunnel_delete(
    tunnel_mgr: State<'_, Arc<AsyncMutex<TunnelManager>>>,
    db: State<'_, DbPool>,
    tunnel_id: String,
) -> Result<(), String> {
    // Stop the running tunnel (if any) and remove the persisted config for good.
    {
        let tunnel_mgr = tunnel_mgr.lock().await;
        tunnel_mgr.close_tunnel(&tunnel_id).await?;
    }
    let conn = db.lock().map_err(|e| format!("DB lock failed: {}", e))?;
    TunnelStore::delete(&conn, &tunnel_id)?;
    drop(conn);
    Ok(())
}

#[tauri::command]
pub async fn tunnel_delete_batch(
    tunnel_mgr: State<'_, Arc<AsyncMutex<TunnelManager>>>,
    db: State<'_, DbPool>,
    ids: Vec<String>,
) -> Result<(), String> {
    // Stop each running tunnel (if any) and remove its persisted config.
    // Block scopes keep the non-Send MutexGuard out of any await point.
    for id in &ids {
        {
            let tunnel_mgr = tunnel_mgr.lock().await;
            tunnel_mgr.close_tunnel(id).await?;
        }
        {
            let conn = db.lock().map_err(|e| format!("DB lock failed: {}", e))?;
            TunnelStore::delete(&conn, id)?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn tunnel_restore(
    ssh_mgr: State<'_, Arc<AsyncMutex<SshManager>>>,
    tunnel_mgr: State<'_, Arc<AsyncMutex<TunnelManager>>>,
    db: State<'_, DbPool>,
    app: tauri::AppHandle,
    session_id: String,
    tunnel_id: String,
) -> Result<(), String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(&session_id)?;
    drop(mgr);

    let saved = {
        let conn = db.lock().map_err(|e| format!("DB lock failed: {}", e))?;
        TunnelStore::get(&conn, &tunnel_id)?
            .ok_or_else(|| "Tunnel configuration not found".to_string())?
    };

    let config = TunnelConfig {
        tunnel_type: parse_tunnel_type(&saved.tunnel_type)?,
        local_host: saved.local_host,
        local_port: saved.local_port,
        remote_host: saved.remote_host,
        remote_port: saved.remote_port,
        note: saved.note,
    };
    let created_at = saved.created_at;

    let tunnel_mgr = tunnel_mgr.lock().await;
    tunnel_mgr
        .create_tunnel(saved.id, session_id, session, config, app, Some(created_at))
        .await?;
    Ok(())
}

#[tauri::command]
pub async fn tunnel_restore_batch(
    ssh_mgr: State<'_, Arc<AsyncMutex<SshManager>>>,
    tunnel_mgr: State<'_, Arc<AsyncMutex<TunnelManager>>>,
    db: State<'_, DbPool>,
    app: tauri::AppHandle,
    session_id: String,
    ids: Vec<String>,
) -> Result<(), String> {
    // Restore each persisted tunnel; a failure stops the batch (already
    // restored ones keep running). Block scopes keep non-Send guards out of
    // await points.
    for tunnel_id in ids {
        let session = {
            let mgr = ssh_mgr.lock().await;
            mgr.get_session(&session_id)?
        };
        let saved = {
            let conn = db.lock().map_err(|e| format!("DB lock failed: {}", e))?;
            TunnelStore::get(&conn, &tunnel_id)?
                .ok_or_else(|| "Tunnel configuration not found".to_string())?
        };
        let config = TunnelConfig {
            tunnel_type: parse_tunnel_type(&saved.tunnel_type)?,
            local_host: saved.local_host,
            local_port: saved.local_port,
            remote_host: saved.remote_host,
            remote_port: saved.remote_port,
            note: saved.note,
        };
        let created_at = saved.created_at;
        {
            let tunnel_mgr = tunnel_mgr.lock().await;
            tunnel_mgr
                .create_tunnel(saved.id, session_id.clone(), session, config, app.clone(), Some(created_at))
                .await?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn tunnel_list(
    ssh_mgr: State<'_, Arc<AsyncMutex<SshManager>>>,
    tunnel_mgr: State<'_, Arc<AsyncMutex<TunnelManager>>>,
    db: State<'_, DbPool>,
    session_id: String,
) -> Result<String, String> {
    // Active tunnels for this session
    let tunnels = tunnel_mgr.lock().await.list_tunnels().await;
    let mut result: Vec<TunnelInfo> = tunnels
        .into_iter()
        .filter(|t| t.session_id == session_id)
        .collect();

    // Persisted configs for the same server (status "stopped" unless active)
    let server_key = {
        let mgr = ssh_mgr.lock().await;
        let session = mgr.get_session(&session_id)?;
        server_key_of(&session)
    };
    let conn = db.lock().map_err(|e| format!("DB lock failed: {}", e))?;
    let saved = TunnelStore::list_for_server(&conn, &server_key)?;
    drop(conn);

    let active_ids: HashSet<String> = result.iter().map(|t| t.id.clone()).collect();
    for s in saved {
        if active_ids.contains(&s.id) {
            continue;
        }
        result.push(TunnelInfo {
            id: s.id,
            session_id: String::new(),
            tunnel_type: s.tunnel_type,
            local_host: s.local_host,
            local_port: s.local_port,
            remote_host: s.remote_host,
            remote_port: s.remote_port,
            status: "stopped".to_string(),
            created_at: s.created_at,
            note: s.note,
        });
    }

    // Stable order by creation time so starting/stopping a tunnel does not
    // move it around the list (its status changes, its position does not).
    result.sort_by(|a, b| a.created_at.cmp(&b.created_at).then_with(|| a.id.cmp(&b.id)));

    serde_json::to_string(&result).map_err(|e| format!("JSON error: {}", e))
}

#[tauri::command]
pub async fn tunnel_get(
    tunnel_mgr: State<'_, Arc<AsyncMutex<TunnelManager>>>,
    tunnel_id: String,
) -> Result<Option<String>, String> {
    let tunnel_mgr = tunnel_mgr.lock().await;
    match tunnel_mgr.get_tunnel(&tunnel_id).await {
        Some(info) => serde_json::to_string(&info)
            .map(Some)
            .map_err(|e| format!("JSON error: {}", e)),
        None => Ok(None),
    }
}

#[tauri::command]
pub async fn tunnel_update_note(
    tunnel_mgr: State<'_, Arc<AsyncMutex<TunnelManager>>>,
    db: State<'_, DbPool>,
    tunnel_id: String,
    note: String,
) -> Result<(), String> {
    // Persist first, then mirror into the in-memory tunnel (if active) so the
    // list stays consistent regardless of the tunnel's current status.
    {
        let conn = db.lock().map_err(|e| format!("DB lock failed: {}", e))?;
        TunnelStore::update_note(&conn, &tunnel_id, &note)?;
    }

    let tunnel_mgr = tunnel_mgr.lock().await;
    tunnel_mgr.update_note(&tunnel_id, note).await?;
    Ok(())
}
