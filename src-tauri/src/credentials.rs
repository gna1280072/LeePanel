//! 凭据安全存储模块 —— 基于系统钥匙串（keyring v3）。
//!
//! 设计原则：
//! - 明文凭据只在本模块（Rust 进程）内流转，不落 SQLite、不进前端。
//! - 每个连接两类凭据，以 `service="leepanel"` + `user="{config_id}:{kind}"` 标识。
//! - 所有 Entry 访问串行化（keyring 文档明确要求同一 credential 不可并发访问）。
//! - `NoEntry` 视为"凭据不存在"（返回 None），不向上报错。

use keyring::Entry;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// 凭据种类
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CredKind {
    Password,
    Passphrase,
}

impl CredKind {
    fn suffix(self) -> &'static str {
        match self {
            CredKind::Password => "password",
            CredKind::Passphrase => "passphrase",
        }
    }
}

const SERVICE: &str = "leepanel";

/// keyring Entry 内部 store 不支持并发访问同一 credential，全局串行化。
static ENTRY_LOCK: Mutex<()> = Mutex::new(());

/// 测试用静态 mock 数据库：key = "{service}:{config_id}:{kind}"。
/// 不用 keyring 的 mock builder——它的每个 Entry 持有独立 store，
/// set 与 get 跨 Entry 不共享，roundtrip 无法通过。
#[cfg(test)]
static MOCK_DB: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn mock_key(config_id: &str, kind: CredKind) -> String {
    format!("{}:{}:{}", SERVICE, config_id, kind.suffix())
}

fn entry_for(config_id: &str, kind: CredKind) -> Result<Entry, String> {
    let user = format!("{}:{}", config_id, kind.suffix());
    Entry::new(SERVICE, &user).map_err(|e| format!("Failed to access system keyring: {}", e))
}

/// 保存凭据。空字符串视为"无凭据"，直接返回 Ok 不写入。
pub fn store_set(config_id: &str, kind: CredKind, secret: &str) -> Result<(), String> {
    if secret.is_empty() {
        return Ok(());
    }
    let _guard = ENTRY_LOCK.lock().unwrap();
    #[cfg(test)]
    {
        MOCK_DB.lock().unwrap().insert(mock_key(config_id, kind), secret.to_string());
        return Ok(());
    }
    #[cfg(not(test))]
    {
        let entry = entry_for(config_id, kind)?;
        entry
            .set_password(secret)
            .map_err(|e| format!("Failed to save credential to system keyring: {}", e))
    }
}

/// 读取凭据；不存在时返回 `Ok(None)`。
pub fn store_get(config_id: &str, kind: CredKind) -> Result<Option<String>, String> {
    let _guard = ENTRY_LOCK.lock().unwrap();
    #[cfg(test)]
    {
        return Ok(MOCK_DB.lock().unwrap().get(&mock_key(config_id, kind)).cloned());
    }
    #[cfg(not(test))]
    {
        let entry = entry_for(config_id, kind)?;
        match entry.get_password() {
            Ok(pw) => Ok(Some(pw)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!("Failed to read credential from system keyring: {}", e)),
        }
    }
}

/// 删除某连接的单类凭据；不存在视为成功。
pub fn store_delete_single(config_id: &str, kind: CredKind) -> Result<(), String> {
    let _guard = ENTRY_LOCK.lock().unwrap();
    #[cfg(test)]
    {
        MOCK_DB.lock().unwrap().remove(&mock_key(config_id, kind));
        return Ok(());
    }
    #[cfg(not(test))]
    {
        let entry = entry_for(config_id, kind)?;
        match entry.delete_credential() {
            Ok(()) => {}
            Err(keyring::Error::NoEntry) => {}
            Err(e) => {
                return Err(format!(
                    "Failed to delete credential from system keyring: {}",
                    e
                ))
            }
        }
        Ok(())
    }
}

/// 删除某连接的两类凭据；不存在视为成功。
pub fn store_delete(config_id: &str) -> Result<(), String> {
    let _guard = ENTRY_LOCK.lock().unwrap();
    for kind in [CredKind::Password, CredKind::Passphrase] {
        #[cfg(test)]
        {
            MOCK_DB.lock().unwrap().remove(&mock_key(config_id, kind));
        }
        #[cfg(not(test))]
        {
            let entry = entry_for(config_id, kind)?;
            match entry.delete_credential() {
                Ok(()) => {}
                Err(keyring::Error::NoEntry) => {}
                Err(e) => {
                    return Err(format!(
                        "Failed to delete credential from system keyring: {}",
                        e
                    ))
                }
            }
        }
    }
    Ok(())
}

/// 探测系统钥匙串可用性：写入-读取-删除一条固定测试记录。
/// Linux 无 D-Bus Secret Service 等场景返回 false，调用方据此降级。
pub fn store_available() -> bool {
    const PROBE_ID: &str = "__leepanel_probe__";
    let ok = store_set(PROBE_ID, CredKind::Password, "probe").is_ok()
        && store_get(PROBE_ID, CredKind::Password)
            .map(|v| v.as_deref() == Some("probe"))
            .unwrap_or(false);
    let _ = store_delete(PROBE_ID);
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    // cfg(test) 下 store_set/get/delete 自动走静态 MOCK_DB，不触碰真实系统钥匙串。

    #[test]
    fn set_get_roundtrip() {
        store_set("c1", CredKind::Password, "secret").unwrap();
        assert_eq!(
            store_get("c1", CredKind::Password).unwrap(),
            Some("secret".to_string())
        );
        let _ = store_delete("c1");
    }

    #[test]
    fn get_missing_returns_none() {
        assert_eq!(store_get("no-such-id", CredKind::Password).unwrap(), None);
        assert_eq!(store_get("no-such-id", CredKind::Passphrase).unwrap(), None);
    }

    #[test]
    fn delete_removes_both_kinds() {
        store_set("c2", CredKind::Password, "pw").unwrap();
        store_set("c2", CredKind::Passphrase, "pp").unwrap();
        store_delete("c2").unwrap();
        assert_eq!(store_get("c2", CredKind::Password).unwrap(), None);
        assert_eq!(store_get("c2", CredKind::Passphrase).unwrap(), None);
    }

    #[test]
    fn delete_missing_is_ok() {
        store_delete("no-such-id").unwrap();
    }

    #[test]
    fn empty_secret_not_stored() {
        store_set("c3", CredKind::Password, "").unwrap();
        assert_eq!(store_get("c3", CredKind::Password).unwrap(), None);
    }

    #[test]
    fn passphrase_isolated_from_password() {
        store_set("c4", CredKind::Password, "pw").unwrap();
        assert_eq!(store_get("c4", CredKind::Passphrase).unwrap(), None);
        assert_eq!(store_get("c4", CredKind::Password).unwrap(), Some("pw".to_string()));
        let _ = store_delete("c4");
    }
}
