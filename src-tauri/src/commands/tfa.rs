//! SSH 2FA（v9）管理命令 —— 服务端 TOTP 双因素认证闭环。
//!
//! 设计要点：
//! - 全部操作在已连接会话上执行（复用 session_exec_with_output，自动获得 sudo 提权能力）；
//! - 一期仅支持 direct_root（拍板：sudo 模式暂不开放，避免 sudoers 放行 apt 白名单扩大权限面）；
//! - 配置前强制备份（/etc/leepanel-tfa-backups/<ts>），`sshd -t` 预检失败自动回滚；
//! - TOTP secret 不落库、不进日志，仅经加密 SSH 通道回传前端展示二维码/手动密钥。

use serde::Serialize;
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex as AsyncMutex;
use crate::audit::audit_log;
use crate::ssh::{self, SshManager};
use crate::DbPool;

#[derive(Serialize, Clone, Debug)]
pub struct TfaStatus {
    /// 整体判定：TOTP 全套就绪 或 轻量模式已启用。
    pub enabled: bool,
    pub installed: bool,
    pub pam_configured: bool,
    pub sshd_configured: bool,
    pub secret_initialized: bool,
    /// 轻量双因素（AuthenticationMethods publickey,password）已启用。
    pub light_enabled: bool,
    /// 该服务器是否支持 2FA 配置（一期仅 direct_root + root 身份）。
    pub configurable: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct TfaEnrollResult {
    pub secret: String,
    pub otpauth_uri: String,
    pub backup_codes: Vec<String>,
}

/// 会话级 root 检查：一期仅 direct_root + root 身份允许安装/配置/卸载。
fn require_root(session: &ssh::SshSession) -> Result<(), String> {
    if session.connect_info.auth_mode == "direct_root" && session.connect_info.username == "root" {
        Ok(())
    } else {
        Err("2FA setup requires root (direct_root). Sudo-mode servers are not supported yet; switch the connection to direct_root or run the steps manually."
            .to_string())
    }
}

/// 只读检测：依赖 / PAM / sshd_config / 当前账号 secret 四项状态。
#[tauri::command]
pub async fn tfa_get_status(
    ssh_mgr: State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: String,
) -> Result<TfaStatus, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(&session_id)?;
    let configurable = session.connect_info.auth_mode == "direct_root"
        && session.connect_info.username == "root";
    drop(mgr);

    let script = r#"
command -v google-authenticator >/dev/null 2>&1 && echo "INSTALLED=yes" || echo "INSTALLED=no"
grep -q "pam_google_authenticator" /etc/pam.d/sshd 2>/dev/null && echo "PAM=yes" || echo "PAM=no"
grep -Eq "^[[:space:]]*KbdInteractiveAuthentication[[:space:]]+yes|^[[:space:]]*ChallengeResponseAuthentication[[:space:]]+yes" /etc/ssh/sshd_config 2>/dev/null && echo "KBD=yes" || echo "KBD=no"
grep -q "^[[:space:]]*AuthenticationMethods" /etc/ssh/sshd_config 2>/dev/null && echo "AM=yes" || echo "AM=no"
grep -q "^[[:space:]]*AuthenticationMethods[[:space:]]\+publickey,password" /etc/ssh/sshd_config 2>/dev/null && echo "LIGHT=yes" || echo "LIGHT=no"
[ -f ~/.google_authenticator ] && echo "SECRET=yes" || echo "SECRET=no"
"#;
    let (stdout, _, code) = ssh::session_exec_with_output(&session, script, 10).await?;
    if code != 0 {
        return Err(format!("Failed to probe 2FA status: {}", stdout.trim()));
    }
    let mut st = TfaStatus {
        enabled: false,
        installed: false,
        pam_configured: false,
        sshd_configured: false,
        secret_initialized: false,
        light_enabled: false,
        configurable,
    };
    for line in stdout.lines() {
        if let Some((k, v)) = line.trim().split_once('=') {
            let yes = v.trim() == "yes";
            match k.trim() {
                "INSTALLED" => st.installed = yes,
                "PAM" => st.pam_configured = yes,
                "KBD" | "AM" => if yes { st.sshd_configured = true },
                "LIGHT" => st.light_enabled = yes,
                "SECRET" => st.secret_initialized = yes,
                _ => {}
            }
        }
    }
    let totp_ready = st.installed && st.pam_configured && st.sshd_configured && st.secret_initialized;
    st.enabled = totp_ready || st.light_enabled;
    Ok(st)
}

/// 安装 TOTP 组件（apt-get / dnf / yum / apk 按发行版分支）。
#[tauri::command]
pub async fn tfa_install(
    ssh_mgr: State<'_, Arc<AsyncMutex<SshManager>>>,
    db: State<'_, DbPool>,
    session_id: String,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(&session_id)?;
    require_root(&session)?;
    let host = session.connect_info.host.clone();
    let username = session.connect_info.username.clone();
    drop(mgr);

    let script = r#"
if command -v apt-get >/dev/null 2>&1; then
  apt-get update -qq 2>&1 || true
  DEBIAN_FRONTEND=noninteractive apt-get install -y libpam-google-authenticator 2>&1 || { echo "APT_INSTALL_FAILED"; exit 1; }
elif command -v dnf >/dev/null 2>&1; then
  dnf install -y google-authenticator 2>&1 || { echo "DNF_INSTALL_FAILED"; exit 1; }
elif command -v yum >/dev/null 2>&1; then
  yum install -y google-authenticator 2>&1 || { echo "YUM_INSTALL_FAILED"; exit 1; }
elif command -v apk >/dev/null 2>&1; then
  apk add google-authenticator 2>&1 || { echo "APK_INSTALL_FAILED"; exit 1; }
else
  echo "UNSUPPORTED_DISTRO"; exit 1
fi
command -v google-authenticator >/dev/null 2>&1 && echo "INSTALL_OK" || { echo "BINARY_MISSING"; exit 1; }
"#;
    let (stdout, _, code) = ssh::session_exec_with_output(&session, script, 300).await?;
    if code != 0 {
        audit_log(&db.lock().unwrap(), &host, &username, "tfa_install", "install google-authenticator", "failed", &stdout.trim());
        return Err(format!("2FA install failed: {}", stdout.trim()));
    }
    audit_log(&db.lock().unwrap(), &host, &username, "tfa_install", "install google-authenticator", "success", "");
    Ok(stdout.trim().to_string())
}

/// 写配置（备份 → PAM + sshd_config → `sshd -t` 预检 → reload；预检失败自动回滚）。
#[tauri::command]
pub async fn tfa_configure(
    ssh_mgr: State<'_, Arc<AsyncMutex<SshManager>>>,
    db: State<'_, DbPool>,
    session_id: String,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(&session_id)?;
    require_root(&session)?;
    let host = session.connect_info.host.clone();
    let username = session.connect_info.username.clone();
    drop(mgr);

    let script = r#"
set -e
TS=$(date +%s)
BK=/etc/leepanel-tfa-backups
mkdir -p "$BK"
cp /etc/pam.d/sshd "$BK/sshd.pam.$TS" 2>/dev/null || true
cp /etc/ssh/sshd_config "$BK/sshd_config.$TS" 2>/dev/null || true
echo "$TS" > "$BK/latest"

if ! grep -q "pam_google_authenticator" /etc/pam.d/sshd 2>/dev/null; then
  printf '%s\n' 'auth required pam_google_authenticator.so nullok' >> /etc/pam.d/sshd
fi

if ! grep -Eq "^[[:space:]]*KbdInteractiveAuthentication[[:space:]]+yes|^[[:space:]]*ChallengeResponseAuthentication[[:space:]]+yes" /etc/ssh/sshd_config 2>/dev/null; then
  sed -i '/^[[:space:]]*KbdInteractiveAuthentication/d;/^[[:space:]]*ChallengeResponseAuthentication/d' /etc/ssh/sshd_config
  if ssh -V 2>&1 | grep -Eq "OpenSSH_8\.[7-9]|OpenSSH_[9-9]"; then
    printf '%s\n' 'KbdInteractiveAuthentication yes' >> /etc/ssh/sshd_config
  else
    printf '%s\n' 'ChallengeResponseAuthentication yes' >> /etc/ssh/sshd_config
  fi
fi
sed -i '/^[[:space:]]*AuthenticationMethods/d' /etc/ssh/sshd_config
# 认证路径（2026-08-28 修正）：
#   publickey,keyboard-interactive  → 密钥用户：先公钥、后验证码
#   keyboard-interactive            → 密码用户：PAM 完整栈（密码+验证码）直接经 keyboard-interactive
# 之前用 `password,keyboard-interactive` 要求"先密码成功"才能尝试 keyboard-interactive，
# 客户端直接发起 keyboard-interactive 会被服务端顺序检查拒绝（认证失败）。
printf '%s\n' 'AuthenticationMethods publickey,keyboard-interactive keyboard-interactive' >> /etc/ssh/sshd_config

if ! sshd -t 2>/tmp/leepanel-sshd-t.err; then
  cp "$BK/sshd.pam.$TS" /etc/pam.d/sshd
  cp "$BK/sshd_config.$TS" /etc/ssh/sshd_config
  rm -f "$BK/sshd.pam.$TS" "$BK/sshd_config.$TS"
  echo "SSHD_TEST_FAILED"
  cat /tmp/leepanel-sshd-t.err
  exit 1
fi

systemctl reload sshd 2>/dev/null || service ssh reload 2>/dev/null || service sshd reload 2>/dev/null || /etc/init.d/ssh reload 2>/dev/null || true
echo "CONFIGURED"
"#;
    let (stdout, _, code) = ssh::session_exec_with_output(&session, script, 30).await?;
    if code != 0 {
        audit_log(&db.lock().unwrap(), &host, &username, "tfa_configure", "write PAM/sshd 2FA config", "failed", &stdout.trim());
        return Err(format!("2FA configure failed (config rolled back): {}", stdout.trim()));
    }
    audit_log(&db.lock().unwrap(), &host, &username, "tfa_configure", "write PAM/sshd 2FA config", "success", "");
    Ok(stdout.trim().to_string())
}

/// 轻量双因素（P4）：仅写 sshd_config 强制 `AuthenticationMethods publickey,password`。
/// 无需装包/生成 secret；复用备份 + `sshd -t` 预检 + 自动回滚机制。
#[tauri::command]
pub async fn tfa_configure_light(
    ssh_mgr: State<'_, Arc<AsyncMutex<SshManager>>>,
    db: State<'_, DbPool>,
    session_id: String,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(&session_id)?;
    require_root(&session)?;
    let host = session.connect_info.host.clone();
    let username = session.connect_info.username.clone();
    drop(mgr);

    let script = r#"
set -e
TS=$(date +%s)
BK=/etc/leepanel-tfa-backups
mkdir -p "$BK"
cp /etc/ssh/sshd_config "$BK/sshd_config.$TS" 2>/dev/null || true
echo "$TS" > "$BK/latest"
sed -i '/^[[:space:]]*AuthenticationMethods/d' /etc/ssh/sshd_config
printf '%s\n' 'AuthenticationMethods publickey,password' >> /etc/ssh/sshd_config
if ! sshd -t 2>/tmp/leepanel-sshd-t.err; then
  cp "$BK/sshd_config.$TS" /etc/ssh/sshd_config
  rm -f "$BK/sshd_config.$TS"
  echo "SSHD_TEST_FAILED"
  cat /tmp/leepanel-sshd-t.err
  exit 1
fi
systemctl reload sshd 2>/dev/null || service ssh reload 2>/dev/null || service sshd reload 2>/dev/null || true
echo "CONFIGURED"
"#;
    let (stdout, _, code) = ssh::session_exec_with_output(&session, script, 30).await?;
    if code != 0 {
        audit_log(&db.lock().unwrap(), &host, &username, "tfa_configure_light", "force publickey,password auth", "failed", &stdout.trim());
        return Err(format!("Light 2FA configure failed (config rolled back): {}", stdout.trim()));
    }
    audit_log(&db.lock().unwrap(), &host, &username, "tfa_configure_light", "force publickey,password auth", "success", "");
    Ok(stdout.trim().to_string())
}

/// 初始化 TOTP secret（非交互生成），返回 base32 secret + otpauth URI + 备用码。
/// secret 仅经加密通道回传前端展示，不落库、不进日志。
#[tauri::command]
pub async fn tfa_enroll(
    ssh_mgr: State<'_, Arc<AsyncMutex<SshManager>>>,
    db: State<'_, DbPool>,
    session_id: String,
) -> Result<TfaEnrollResult, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(&session_id)?;
    let host = session.connect_info.host.clone();
    let username = session.connect_info.username.clone();
    drop(mgr);

    // 方案 B（2026-08-28）：
    // - `< /dev/null`：stdin 立即 EOF，避免 google-authenticator 交互阻塞（此前 15s 超时根因）
    // - `-C`（--no-confirm）：跳过"Enter code from app"验证码确认——无 TTY 下该步骤
    //   读 /dev/tty 失败（getline(): Inappropriate ioctl for device）导致退出码非 0
    // - `-Q none`：不输出终端二维码（前端自己绘制），避免 ANSI/UTF8 乱码
    let script = r#"
google-authenticator -t -d -f -r 3 -R 30 -w 3 -C -Q none < /dev/null 2>&1
"#;
    let (stdout, _, code) = ssh::session_exec_with_output(&session, script, 30).await?;
    // 解析：第一行 "Your new secret key is: <BASE32>"；emergency scratch codes 其后 5 个
    let mut secret = String::new();
    let mut backup_codes: Vec<String> = Vec::new();
    let mut in_codes = false;
    for line in stdout.lines() {
        let t = line.trim();
        if let Some(idx) = t.find("Your new secret key is:") {
            if let Some(rest) = t[idx + "Your new secret key is:".len()..].split_whitespace().next() {
                secret = rest.to_string();
            }
        } else if t.contains("emergency scratch codes") {
            in_codes = true;
            continue;
        } else if in_codes {
            let code = t.split_whitespace().next().unwrap_or("");
            if !code.is_empty() {
                backup_codes.push(code.to_string());
            }
            if backup_codes.len() >= 5 {
                break;
            }
        }
    }
    // 退出码检查放在解析之后：secret 写入 ~/.google_authenticator 发生在 ask_code 之前，
    // 即使旧版本 google-authenticator 不支持 -C 导致验证步骤失败（退出码非 0），
    // 只要 secret 已成功解析（文件已落盘）就视为成功。
    if code != 0 && secret.is_empty() {
        return Err(format!("2FA enroll failed: {}", stdout.trim()));
    }
    if secret.is_empty() {
        return Err(format!("Failed to parse TOTP secret from output: {}", stdout.trim()));
    }
    let otpauth_uri = format!(
        "otpauth://totp/LeePanel:{}@{}?secret={}&issuer=LeePanel&period=30&digits=6&algorithm=SHA1",
        username, host, secret
    );
    audit_log(&db.lock().unwrap(), &host, &username, "tfa_enroll", "generate TOTP secret", "success", "");
    Ok(TfaEnrollResult { secret, otpauth_uri, backup_codes })
}

/// 读取已初始化的 TOTP secret 与备用码（~/.google_authenticator）。
/// 用于"查看备用码"与"继续用已有密钥"（向导检测到服务器已有 secret 时重建二维码）。
/// secret 仅经加密通道回传前端展示，不落库、不进日志。
#[tauri::command]
pub async fn tfa_read_secret(
    ssh_mgr: State<'_, Arc<AsyncMutex<SshManager>>>,
    session_id: String,
) -> Result<TfaEnrollResult, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(&session_id)?;
    let host = session.connect_info.host.clone();
    let username = session.connect_info.username.clone();
    drop(mgr);

    let script = r#"
if [ -f ~/.google_authenticator ]; then cat ~/.google_authenticator; else echo "NO_SECRET_FILE"; fi
"#;
    let (stdout, _, code) = ssh::session_exec_with_output(&session, script, 10).await?;
    if code != 0 {
        return Err(format!("Failed to read secret file: {}", stdout.trim()));
    }
    if stdout.trim() == "NO_SECRET_FILE" {
        return Err("NO_SECRET_FILE".to_string());
    }
    // 文件格式：第一行 base32 secret；以 '"' 开头的为配置行（RATE_LIMIT 等）；其余为备用码
    let mut secret = String::new();
    let mut backup_codes: Vec<String> = Vec::new();
    for (i, line) in stdout.lines().enumerate() {
        let t = line.trim();
        if i == 0 {
            secret = t.to_string();
        } else if !t.is_empty() && !t.starts_with('"') {
            backup_codes.push(t.to_string());
        }
    }
    // base32 合法性校验（A-Z2-7）
    if secret.is_empty() || !secret.chars().all(|c| c.is_ascii_uppercase() || ('2'..='7').contains(&c)) {
        return Err(format!("Invalid secret file format: {}", stdout.trim()));
    }
    let otpauth_uri = format!(
        "otpauth://totp/LeePanel:{}@{}?secret={}&issuer=LeePanel&period=30&digits=6&algorithm=SHA1",
        username, host, secret
    );
    Ok(TfaEnrollResult { secret, otpauth_uri, backup_codes })
}

/// 关闭 2FA：恢复备份（无备份则移除面板写入的行）→ 删除 secret → 预检 → reload。
#[tauri::command]
pub async fn tfa_disable(
    ssh_mgr: State<'_, Arc<AsyncMutex<SshManager>>>,
    db: State<'_, DbPool>,
    session_id: String,
) -> Result<String, String> {
    let mgr = ssh_mgr.lock().await;
    let session = mgr.get_session(&session_id)?;
    require_root(&session)?;
    let host = session.connect_info.host.clone();
    let username = session.connect_info.username.clone();
    drop(mgr);

    let script = r#"
BK=/etc/leepanel-tfa-backups
if [ -f "$BK/latest" ]; then
  TS=$(cat "$BK/latest")
  [ -f "$BK/sshd.pam.$TS" ] && cp "$BK/sshd.pam.$TS" /etc/pam.d/sshd
  [ -f "$BK/sshd_config.$TS" ] && cp "$BK/sshd_config.$TS" /etc/ssh/sshd_config
else
  sed -i '/pam_google_authenticator/d' /etc/pam.d/sshd
  sed -i '/^[[:space:]]*AuthenticationMethods/d' /etc/ssh/sshd_config
fi
rm -f ~/.google_authenticator
if ! sshd -t 2>/tmp/leepanel-sshd-t.err; then
  echo "SSHD_TEST_FAILED"
  cat /tmp/leepanel-sshd-t.err
  exit 1
fi
systemctl reload sshd 2>/dev/null || service ssh reload 2>/dev/null || service sshd reload 2>/dev/null || true
rm -rf "$BK"
echo "DISABLED"
"#;
    let (stdout, _, code) = ssh::session_exec_with_output(&session, script, 30).await?;
    if code != 0 {
        audit_log(&db.lock().unwrap(), &host, &username, "tfa_disable", "disable 2FA auth", "failed", &stdout.trim());
        return Err(format!("2FA disable failed: {}", stdout.trim()));
    }
    audit_log(&db.lock().unwrap(), &host, &username, "tfa_disable", "disable 2FA auth", "success", "");
    Ok(stdout.trim().to_string())
}
