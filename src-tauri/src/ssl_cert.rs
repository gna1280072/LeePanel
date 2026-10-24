//! 用户自定义 SSL 证书（P1）—— 上传 / 自签 / 移除。
//!
//! 设计要点：
//! - 证书与私钥由前端读取本地文件后作为 PEM 文本传入，**不做服务器路径引用**
//!   （避免后端获得任意本地/远端文件读取能力）；
//! - 私钥只经 SFTP 以 0600 权限落盘到 `/tmp` 暂存目录，随后由提权脚本
//!   `install -m 600` 安装到最终目录，**绝不拼进 shell 命令行**；
//! - 配置改写（vhost）在 Rust 侧完成纯文本处理，提权脚本只负责落盘 + `nginx -t` +
//!   reload + 失败回滚，脚本正文不含任何用户输入；
//! - 目标目录跟随环境：宝塔面板用 `/www/server/panel/vhost/cert/<domain>`，
//!   标准环境用 `/etc/nginx/ssl/<domain>`。

use crate::ssh::{SshCache, SshSession};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

/// 证书 PEM 上限（含中间证书链）
const MAX_CERT_BYTES: usize = 512 * 1024;
/// 私钥 PEM 上限
const MAX_KEY_BYTES: usize = 64 * 1024;

// ===== 数据结构 =====

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SslTarget {
    pub domain: String,
    pub config_path: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SslCertInfo {
    pub domain: String,
    pub has_cert: bool,
    pub source: String,
    pub cert_path: String,
    pub key_path: String,
    pub subject: String,
    pub issuer: String,
    pub not_before: String,
    pub not_after: String,
    pub days_remaining: i64,
    pub sans: Vec<String>,
    pub key_info: String,
    pub fingerprint: String,
    pub chain_length: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SslCheckItem {
    /// i18n key 后缀（前端映射 `ssl.check.<code>`）
    pub code: String,
    /// ok | warn | error
    pub level: String,
    /// 插值参数（count / notAfter / domains / type）
    pub detail: String,
    /// 附加数值（剩余天数等），无则 -1
    pub days: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SslPrecheck {
    pub ok: bool,
    pub items: Vec<SslCheckItem>,
    pub info: Option<SslCertInfo>,
}

/// openssl 探测结果
#[derive(Default, Debug)]
struct CertProbe {
    subject: String,
    issuer: String,
    not_before: String,
    not_after: String,
    not_after_epoch: i64,
    days_remaining: i64,
    sans: Vec<String>,
    key_info: String,
    fingerprint: String,
    pubkey_cert: String,
    pubkey_key: String,
    chain_length: i32,
}

// ===== 公共入口 =====

/// 批量读取站点当前证书信息（单次 SSH exec，避免逐个站点往返）。
pub async fn list_infos(
    session: &SshSession,
    targets: &[SslTarget],
) -> Result<Vec<SslCertInfo>, String> {
    if targets.is_empty() {
        return Ok(Vec::new());
    }
    let mut script = String::from(
        r#"run_one() {
  echo "@@SITE:$1@@"
  CFG="$2"
  echo "CONFIG=$CFG"
  CERT=$(grep -m1 -E '^[[:space:]]*ssl_certificate[[:space:]]' "$CFG" 2>/dev/null | sed 's/.*ssl_certificate[[:space:]]*//; s/;.*//' | tr -d '"')
  KEY=$(grep -m1 -E '^[[:space:]]*ssl_certificate_key[[:space:]]' "$CFG" 2>/dev/null | sed 's/.*ssl_certificate_key[[:space:]]*//; s/;.*//' | tr -d '"')
  echo "CERT=$CERT"
  echo "KEY=$KEY"
  if [ -n "$CERT" ] && [ -f "$CERT" ]; then
    M="/tmp/.leepanel-ssl-meta-$$"
    openssl x509 -in "$CERT" -noout -text 2>/dev/null > "$M"
    grep -E "^[[:space:]]*(Subject:|Issuer:|Not Before:|Not After[[:space:]]*:|Public Key Algorithm:|Public-Key:|ASN1 OID:)" "$M" 2>/dev/null
    echo "SANATTR=$(grep -A1 'Subject Alternative Name' "$M" 2>/dev/null | tail -n +2 | head -1 | sed 's/^[[:space:]]*//')"
    echo "FP=$(openssl x509 -in "$CERT" -noout -fingerprint -sha256 2>/dev/null | cut -d= -f2)"
    echo "NAE=$(date -u -d "$(openssl x509 -in "$CERT" -noout -enddate 2>/dev/null | cut -d= -f2)" +%s 2>/dev/null || echo 0)"
    echo "CHAIN=$(grep -c 'BEGIN CERTIFICATE' "$CERT" 2>/dev/null || echo 0)"
    rm -f "$M"
  fi
  echo "@@END@@"
}
"#,
    );
    for t in targets {
        script.push_str(&format!(
            "run_one '{}' '{}'\n",
            escape_shell_single(&t.domain),
            escape_shell_single(&t.config_path)
        ));
    }
    let (stdout, _stderr, _code) = crate::ssh::session_exec_with_output(session, &script, 60).await?;

    let mut out: Vec<SslCertInfo> = Vec::new();
    let mut cur: Option<SslCertInfo> = None;
    let mut raw: Vec<String> = Vec::new();
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("@@SITE:") {
            if let Some(info) = cur.take() {
                out.push(finish_info(info, &raw));
            }
            raw.clear();
            let domain = rest.trim_end_matches("@@").to_string();
            cur = Some(SslCertInfo {
                domain,
                source: "none".to_string(),
                ..SslCertInfo::default()
            });
        } else if line.starts_with("@@END@@") {
            if let Some(info) = cur.take() {
                out.push(finish_info(info, &raw));
            }
            raw.clear();
        } else if cur.is_some() {
            raw.push(line.to_string());
        }
    }
    if let Some(info) = cur.take() {
        out.push(finish_info(info, &raw));
    }
    Ok(out)
}

/// 校验用户上传的证书/私钥（不落盘到站点目录，仅 `/tmp` 暂存后立即清理）。
pub async fn precheck(
    session: &SshSession,
    domain: &str,
    cert_pem: &str,
    key_pem: &str,
    chain_pem: Option<&str>,
    site_domains: &[String],
) -> Result<SslPrecheck, String> {
    let mut items: Vec<SslCheckItem> = Vec::new();

    let cert = normalize_pem(cert_pem);
    let key = normalize_pem(key_pem);
    let chain = chain_pem.map(normalize_pem).unwrap_or_default();
    let fullchain = if chain.trim().is_empty() {
        cert.clone()
    } else {
        format!("{}\n{}", cert, chain)
    };

    // --- 本地结构校验 ---
    let mut structure_ok = true;
    match validate_cert_pem(&cert) {
        Ok(n) => items.push(item("certFormat", "ok", &n.to_string())),
        Err(_) => {
            structure_ok = false;
            items.push(item("certFormatError", "error", ""));
        }
    }
    match validate_key_pem(&key) {
        Ok(kind) => items.push(item("keyFormat", "ok", &kind)),
        Err(KeyError::Encrypted) => {
            structure_ok = false;
            items.push(item("keyEncrypted", "error", ""));
        }
        Err(KeyError::Invalid) => {
            structure_ok = false;
            items.push(item("keyFormatError", "error", ""));
        }
    }
    if cert.len() > MAX_CERT_BYTES || key.len() > MAX_KEY_BYTES || chain.len() > MAX_CERT_BYTES {
        structure_ok = false;
        items.push(item("sizeTooLarge", "error", ""));
    }

    if !structure_ok {
        return Ok(SslPrecheck {
            ok: false,
            items,
            info: None,
        });
    }

    // --- 远端 openssl 深度校验 ---
    let staged = stage(session, &fullchain, &key).await?;
    let probe = probe(session, &staged.dir).await;
    cleanup(session, &staged).await;

    let probe = match probe {
        Ok(p) => p,
        Err(_) => {
            items.push(item("opensslMissing", "warn", ""));
            return Ok(SslPrecheck {
                ok: true,
                items,
                info: None,
            });
        }
    };

    if !probe.pubkey_cert.trim().is_empty() && probe.pubkey_cert.trim() == probe.pubkey_key.trim() {
        items.push(item("keyMatch", "ok", ""));
    } else {
        items.push(item("keyMismatch", "error", ""));
    }

    if probe.not_after.is_empty() {
        items.push(item("certReadError", "error", ""));
    } else if probe.days_remaining < 0 {
        items.push(item("expiryUnknown", "warn", &probe.not_after));
    } else if probe.days_remaining <= 0 {
        items.push(item("expired", "error", &probe.not_after));
    } else if probe.days_remaining <= 30 {
        items.push(item_days("expirySoon", "warn", &probe.not_after, probe.days_remaining));
    } else {
        items.push(item_days("validity", "ok", &probe.not_after, probe.days_remaining));
    }

    if !site_domains.is_empty() && !probe.sans.is_empty() {
        let missing: Vec<String> = site_domains
            .iter()
            .filter(|d| !cert_covers(&probe.sans, d))
            .cloned()
            .collect();
        if missing.is_empty() {
            items.push(item("domainCovered", "ok", ""));
        } else {
            items.push(item("domainMissing", "warn", &missing.join(", ")));
        }
    }

    if probe.chain_length <= 1 {
        items.push(item("chainMissing", "warn", ""));
    }

    let info = SslCertInfo {
        domain: domain.to_string(),
        has_cert: true,
        source: "pending".to_string(),
        cert_path: String::new(),
        key_path: String::new(),
        subject: probe.subject.clone(),
        issuer: probe.issuer.clone(),
        not_before: probe.not_before.clone(),
        not_after: probe.not_after.clone(),
        days_remaining: probe.days_remaining,
        sans: probe.sans.clone(),
        key_info: probe.key_info.clone(),
        fingerprint: probe.fingerprint.clone(),
        chain_length: probe.chain_length,
    };

    let ok = !items.iter().any(|i| i.level == "error");
    Ok(SslPrecheck {
        ok,
        items,
        info: Some(info),
    })
}

/// 安装用户上传的自定义证书。
#[allow(clippy::too_many_arguments)]
pub async fn install_custom(
    session: &SshSession,
    _cache: &SshCache,
    session_id: &str,
    domain: &str,
    config_path: &str,
    cert_pem: &str,
    key_pem: &str,
    chain_pem: Option<&str>,
    stop_letsencrypt: bool,
    site_domains: &[String],
    app_handle: &AppHandle,
) -> Result<String, String> {
    let emit = |line: &str, status: &str| {
        let _ = app_handle.emit(
            "ssl-install-progress",
            serde_json::json!({
                "sessionId": session_id,
                "domain": domain,
                "line": line,
                "status": status,
            }),
        );
    };

    emit("Validating certificate...", "installing");
    let pre = precheck(session, domain, cert_pem, key_pem, chain_pem, site_domains).await?;
    if !pre.ok {
        let failed: Vec<String> = pre
            .items
            .iter()
            .filter(|i| i.level == "error")
            .map(|i| i.code.clone())
            .collect();
        let _ = emit(&format!("Validation failed: {}", failed.join(", ")), "error");
        return Err(format!("Certificate validation failed: {}", failed.join(", ")));
    }
    for it in &pre.items {
        if it.level != "ok" {
            emit(&format!("[{}] {}", it.level, it.code), "installing");
        }
    }

    let cert = normalize_pem(cert_pem);
    let key = normalize_pem(key_pem);
    let chain = chain_pem.map(normalize_pem).unwrap_or_default();
    let fullchain = if chain.trim().is_empty() {
        cert.clone()
    } else {
        format!("{}\n{}", cert, chain)
    };

    let (cert_dir, cert_file, key_file) = cert_paths(domain, config_path);
    emit(&format!("Certificate directory: {}", cert_dir), "installing");

    // 1) 读取现有 vhost 并改写
    let config = crate::ssh::session_read_file(session, config_path)
        .await
        .map_err(|e| format!("Failed to read nginx config: {}", e))?;
    let patched = patch_config_for_custom_cert(&config, &cert_file, &key_file)?;
    emit("Nginx vhost prepared (SSL block updated)", "installing");

    // 2) 暂存文件（私钥 0600）+ 安装脚本
    let staged = stage(session, &fullchain, &key).await?;
    let script_path = staged.script_path.clone();
    let log_path = format!("{}/install.log", staged.dir);
    let script = build_install_script(
        &staged.dir,
        &script_path,
        config_path,
        &cert_dir,
        &log_path,
        stop_letsencrypt,
        domain,
        false,
        "",
        "",
    );
    write_script(session, &script_path, &script).await?;
    crate::ssh::session_write_file_bytes_mode(
        session,
        &format!("{}/vhost.conf", staged.dir),
        patched.as_bytes(),
        0o600,
    )
    .await?;

    // 3) 执行（sudo 模式下整条脚本以 root 身份运行）
    emit("Installing certificate and reloading nginx...", "installing");
    let (stdout, stderr, code) = crate::ssh::session_exec_with_output(
        session,
        &format!("bash '{}'", script_path),
        120,
    )
    .await?;
    let combined = format!("{}\n{}", stdout, stderr);
    for line in combined.lines() {
        let l = line.trim();
        if !l.is_empty() && !l.starts_with("__LEEPANEL") {
            emit(l, "installing");
        }
    }

    // 清理暂存（脚本内 trap 已尝试，此处兜底）
    cleanup_dir(session, &staged.dir, &script_path).await;

    if code != 0 || combined.contains("__LEEPANEL_FAIL__") {
        emit("Certificate installation failed, previous config restored", "error");
        return Err(format!(
            "Nginx rejected the new configuration (exit code {}). Previous config has been restored.",
            code
        ));
    }
    if combined.contains("__LEEPANEL_RELOAD_FAIL__") {
        emit("Nginx config OK but reload failed", "error");
        return Err("Nginx configuration is valid but reload failed. Please check nginx status.".to_string());
    }

    if combined.contains("__LEEPANEL_LE_STOPPED__") {
        emit("Let's Encrypt renewal stopped for this domain", "installing");
    }
    emit(&format!("Custom certificate installed for {}", domain), "done");
    Ok(format!("Custom certificate installed for {}", domain))
}

/// 生成自签证书并安装（仅用于测试 / 内网）。
#[allow(clippy::too_many_arguments)]
pub async fn generate_self_signed(
    session: &SshSession,
    _cache: &SshCache,
    session_id: &str,
    domain: &str,
    config_path: &str,
    days: u32,
    site_domains: &[String],
    app_handle: &AppHandle,
) -> Result<String, String> {
    let emit = |line: &str, status: &str| {
        let _ = app_handle.emit(
            "ssl-install-progress",
            serde_json::json!({
                "sessionId": session_id,
                "domain": domain,
                "line": line,
                "status": status,
            }),
        );
    };
    if days == 0 || days > 3650 {
        return Err("Validity must be between 1 and 3650 days".to_string());
    }

    let (cert_dir, cert_file, key_file) = cert_paths(domain, config_path);
    let config = crate::ssh::session_read_file(session, config_path)
        .await
        .map_err(|e| format!("Failed to read nginx config: {}", e))?;
    let patched = patch_config_for_custom_cert(&config, &cert_file, &key_file)?;

    emit("Preparing self-signed certificate...", "installing");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = format!("/tmp/.leepanel-ssl-{}", nanos);
    crate::ssh::session_mkdir_sftp(session, &dir).await?;
    let script_path = format!("{}.sh", dir);

    let mut alts: Vec<String> = site_domains
        .iter()
        .filter(|d| d.as_str() != domain && !d.starts_with("*."))
        .cloned()
        .collect();
    alts.dedup();
    let san = {
        let mut s = format!("DNS:{}", domain);
        for a in &alts {
            s.push_str(&format!(",DNS:{}", a));
        }
        s
    };

    let openssl_cnf = format!(
        "[req]\ndistinguished_name=dn\nx509_extensions=v3\nprompt=no\n[dn]\nCN={}\n[v3]\nsubjectAltName={}\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n",
        domain, san
    );
    crate::ssh::session_write_file_bytes_mode(
        session,
        &format!("{}/vhost.conf", dir),
        patched.as_bytes(),
        0o600,
    )
    .await?;
    crate::ssh::session_write_file_bytes_mode(
        session,
        &format!("{}/openssl.cnf", dir),
        openssl_cnf.as_bytes(),
        0o600,
    )
    .await?;

    let script = build_install_script(
        &dir,
        &script_path,
        config_path,
        &cert_dir,
        &format!("{}/install.log", dir),
        false,
        domain,
        true,
        &days.to_string(),
        &format!("{}/openssl.cnf", dir),
    );
    write_script(session, &script_path, &script).await?;

    emit("Generating certificate with openssl...", "installing");
    let (stdout, stderr, code) =
        crate::ssh::session_exec_with_output(session, &format!("bash '{}'", script_path), 120)
            .await?;
    let combined = format!("{}\n{}", stdout, stderr);
    for line in combined.lines() {
        let l = line.trim();
        if !l.is_empty() && !l.starts_with("__LEEPANEL") {
            emit(l, "installing");
        }
    }
    cleanup_dir(session, &dir, &script_path).await;

    if combined.contains("__LEEPANEL_GEN_FAIL__") || code != 0 || combined.contains("__LEEPANEL_FAIL__") {
        emit("Self-signed certificate generation failed", "error");
        return Err("Failed to generate the self-signed certificate. Check that openssl is installed.".to_string());
    }
    if combined.contains("__LEEPANEL_RELOAD_FAIL__") {
        emit("Nginx config OK but reload failed", "error");
        return Err("Nginx configuration is valid but reload failed. Please check nginx status.".to_string());
    }
    emit(&format!("Self-signed certificate installed for {}", domain), "done");
    Ok(format!("Self-signed certificate installed for {}", domain))
}

/// 移除自定义证书，站点回到 HTTP。
pub async fn remove_custom(
    session: &SshSession,
    _cache: &SshCache,
    session_id: &str,
    domain: &str,
    config_path: &str,
    app_handle: &AppHandle,
) -> Result<String, String> {
    let emit = |line: &str, status: &str| {
        let _ = app_handle.emit(
            "ssl-install-progress",
            serde_json::json!({
                "sessionId": session_id,
                "domain": domain,
                "line": line,
                "status": status,
            }),
        );
    };

    let (cert_dir, cert_file, _key_file) = cert_paths(domain, config_path);
    let config = crate::ssh::session_read_file(session, config_path)
        .await
        .map_err(|e| format!("Failed to read nginx config: {}", e))?;
    let patched = strip_custom_cert_config(&config, &cert_file)?;

    emit("Removing SSL block from nginx vhost...", "installing");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = format!("/tmp/.leepanel-ssl-{}", nanos);
    crate::ssh::session_mkdir_sftp(session, &dir).await?;
    let script_path = format!("{}.sh", dir);
    crate::ssh::session_write_file_bytes_mode(
        session,
        &format!("{}/vhost.conf", dir),
        patched.as_bytes(),
        0o600,
    )
    .await?;

    let backup = format!(
        "{}.leepanel.bak.{}",
        config_path,
        nanos / 1_000_000
    );
    // 移除策略：
    // 1) 若存在早于本次安装的 vhost 备份（备份中不含我们的证书路径），优先整份还原，
    //    这样从 Let's Encrypt 切换过来的站点能回到原证书配置而不是裸 HTTP；
    // 2) 否则使用 Rust 侧剥离后的配置（删掉我们新增的 HTTPS 块）。
    let script = format!(
        r#"set -u
D='{dir}'
S='{script}'
cleanup() {{ rm -rf "$D" "$S"; }}
trap cleanup EXIT
NEW="$D/vhost.conf"
PREV=$(ls -1t '{cfg}'.leepanel.bak.* 2>/dev/null | head -n 1)
if [ -n "$PREV" ] && [ -f "$PREV" ] && ! grep -q '{certfile}' "$PREV" 2>/dev/null; then
  NEW="$PREV"
  echo "Restoring previous vhost backup: $PREV"
else
  echo "Removing LeePanel-managed HTTPS block"
fi
cp -f '{cfg}' '{bak}' || exit 15
install -m 644 "$NEW" '{cfg}' || exit 16
if nginx -t >"$D/nginx-t.log" 2>&1; then
  if systemctl reload nginx >/dev/null 2>&1 || nginx -s reload >/dev/null 2>&1; then
    rm -rf '{certdir}'
    echo "__LEEPANEL_OK__"
  else
    echo "__LEEPANEL_RELOAD_FAIL__"
  fi
else
  cp -f '{bak}' '{cfg}'
  echo "__LEEPANEL_FAIL__"
  cat "$D/nginx-t.log"
  exit 20
fi
"#,
        dir = dir,
        script = script_path,
        cfg = config_path,
        bak = backup,
        certdir = cert_dir,
        certfile = cert_file,
    );
    write_script(session, &script_path, &script).await?;

    let (stdout, stderr, code) =
        crate::ssh::session_exec_with_output(session, &format!("bash '{}'", script_path), 60)
            .await?;
    let combined = format!("{}\n{}", stdout, stderr);
    for line in combined.lines() {
        let l = line.trim();
        if !l.is_empty() && !l.starts_with("__LEEPANEL") {
            emit(l, "installing");
        }
    }
    cleanup_dir(session, &dir, &script_path).await;

    if code != 0 || combined.contains("__LEEPANEL_FAIL__") {
        emit("Failed to remove certificate, previous config restored", "error");
        return Err("Nginx rejected the configuration after removal. Previous config restored.".to_string());
    }
    if combined.contains("__LEEPANEL_RELOAD_FAIL__") {
        emit("Nginx config OK but reload failed", "error");
        return Err("Nginx configuration is valid but reload failed. Please check nginx status.".to_string());
    }
    emit("SSL removed, site is back to HTTP", "done");
    Ok("SSL removed, site is back to HTTP".to_string())
}

// ===== 内部实现 =====

fn item(code: &str, level: &str, detail: &str) -> SslCheckItem {
    SslCheckItem {
        code: code.to_string(),
        level: level.to_string(),
        detail: detail.to_string(),
        days: -1,
    }
}

fn item_days(code: &str, level: &str, detail: &str, days: i64) -> SslCheckItem {
    SslCheckItem {
        code: code.to_string(),
        level: level.to_string(),
        detail: detail.to_string(),
        days,
    }
}

/// 前端传进来的 PEM 可能带 BOM / CRLF，统一归一化。
fn normalize_pem(s: &str) -> String {
    s.trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_string()
}

/// 单引号转义（用于把校验过的路径安全嵌进 shell 单引号串）。
fn escape_shell_single(s: &str) -> String {
    s.replace('\'', "'\\''")
}

enum KeyError {
    Invalid,
    Encrypted,
}

/// 证书 PEM 结构校验，返回证书张数。
fn validate_cert_pem(pem: &str) -> Result<usize, String> {
    let begins = pem.matches("-----BEGIN CERTIFICATE-----").count();
    let ends = pem.matches("-----END CERTIFICATE-----").count();
    if begins == 0 || begins != ends {
        return Err("no certificate block".to_string());
    }
    if pem.contains("PRIVATE KEY-----") {
        return Err("certificate field contains a private key".to_string());
    }
    Ok(begins)
}

/// 私钥 PEM 结构校验，返回密钥类型标签。
fn validate_key_pem(pem: &str) -> Result<String, KeyError> {
    if pem.contains("ENCRYPTED PRIVATE KEY") || pem.contains("Proc-Type: 4,ENCRYPTED") {
        return Err(KeyError::Encrypted);
    }
    let kind = if pem.contains("-----BEGIN RSA PRIVATE KEY-----") {
        "RSA (PKCS#1)"
    } else if pem.contains("-----BEGIN EC PRIVATE KEY-----") {
        "ECDSA (SEC1)"
    } else if pem.contains("-----BEGIN PRIVATE KEY-----") {
        "PKCS#8"
    } else {
        return Err(KeyError::Invalid);
    };
    if !pem.contains("-----END") {
        return Err(KeyError::Invalid);
    }
    Ok(kind.to_string())
}

struct Staged {
    dir: String,
    script_path: String,
}

/// 把证书与私钥暂存到远端 `/tmp`（私钥 0600）。
async fn stage(session: &SshSession, fullchain: &str, key: &str) -> Result<Staged, String> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = format!("/tmp/.leepanel-ssl-{}", nanos);
    crate::ssh::session_mkdir_sftp(session, &dir).await?;
    crate::ssh::session_write_file_bytes_mode(
        session,
        &format!("{}/fullchain.pem", dir),
        fullchain.as_bytes(),
        0o600,
    )
    .await?;
    if let Err(e) = crate::ssh::session_write_file_bytes_mode(
        session,
        &format!("{}/privkey.pem", dir),
        key.as_bytes(),
        0o600,
    )
    .await
    {
        cleanup_dir(session, &dir, "").await;
        return Err(e);
    }
    Ok(Staged {
        script_path: format!("{}.sh", dir),
        dir,
    })
}

async fn write_script(session: &SshSession, path: &str, script: &str) -> Result<(), String> {
    crate::ssh::session_write_file_bytes_mode(session, path, script.as_bytes(), 0o700).await
}

async fn cleanup(session: &SshSession, staged: &Staged) {
    cleanup_dir(session, &staged.dir, "").await;
}

async fn cleanup_dir(session: &SshSession, dir: &str, script: &str) {
    let cmd = if script.is_empty() {
        format!("rm -rf '{}' 2>/dev/null; true", escape_shell_single(dir))
    } else {
        format!(
            "rm -rf '{}' '{}' 2>/dev/null; true",
            escape_shell_single(dir),
            escape_shell_single(script)
        )
    };
    let _ = crate::ssh::session_exec_with_output(session, &cmd, 15).await;
}

/// 远端 openssl 探测：主体/签发者/有效期/SAN/公钥对比/链长度。
async fn probe(session: &SshSession, dir: &str) -> Result<CertProbe, String> {
    let script = format!(
        r#"cd '{dir}' || exit 2
M='{dir}/.meta'
openssl x509 -in fullchain.pem -noout -text 2>/dev/null > "$M"
if [ ! -s "$M" ]; then echo "PROBE_NO_OPENSSL"; exit 3; fi
grep -E "^[[:space:]]*(Subject:|Issuer:|Not Before:|Not After[[:space:]]*:|Public Key Algorithm:|Public-Key:|ASN1 OID:)" "$M" 2>/dev/null
echo "SANATTR=$(grep -A1 'Subject Alternative Name' "$M" 2>/dev/null | tail -n +2 | head -1 | sed 's/^[[:space:]]*//')"
echo "FP=$(openssl x509 -in fullchain.pem -noout -fingerprint -sha256 2>/dev/null | cut -d= -f2)"
echo "NAE=$(date -u -d "$(openssl x509 -in fullchain.pem -noout -enddate 2>/dev/null | cut -d= -f2)" +%s 2>/dev/null || echo 0)"
echo "CHAIN=$(grep -c 'BEGIN CERTIFICATE' fullchain.pem 2>/dev/null || echo 0)"
echo "PUBKEY_CERT_BEGIN"
openssl x509 -in fullchain.pem -noout -pubkey 2>/dev/null
echo "PUBKEY_CERT_END"
echo "PUBKEY_KEY_BEGIN"
openssl pkey -in privkey.pem -pubout 2>/dev/null
echo "PUBKEY_KEY_END"
echo "PROBE_DONE"
"#,
        dir = dir
    );
    let (stdout, _stderr, _code) = crate::ssh::session_exec_with_output(session, &script, 30).await?;
    if stdout.contains("PROBE_NO_OPENSSL") || !stdout.contains("PROBE_DONE") {
        return Err("openssl unavailable".to_string());
    }

    let mut p = CertProbe::default();
    let mut in_cert_key = false;
    let mut in_key_key = false;
    for line in stdout.lines() {
        let t = line.trim();
        if t == "PUBKEY_CERT_BEGIN" {
            in_cert_key = true;
            continue;
        }
        if t == "PUBKEY_CERT_END" {
            in_cert_key = false;
            continue;
        }
        if t == "PUBKEY_KEY_BEGIN" {
            in_key_key = true;
            continue;
        }
        if t == "PUBKEY_KEY_END" {
            in_key_key = false;
            continue;
        }
        if in_cert_key {
            p.pubkey_cert.push_str(t);
            p.pubkey_cert.push('\n');
            continue;
        }
        if in_key_key {
            p.pubkey_key.push_str(t);
            p.pubkey_key.push('\n');
            continue;
        }
        if let Some(v) = t.strip_prefix("Subject:") {
            p.subject = v.trim().to_string();
        } else if let Some(v) = t.strip_prefix("Issuer:") {
            p.issuer = v.trim().to_string();
        } else if let Some(v) = t.strip_prefix("Not Before:") {
            p.not_before = v.trim().to_string();
        } else if t.starts_with("Not After") {
            if let Some(v) = t.split_once(':') {
                if !v.1.trim().is_empty() {
                    p.not_after = v.1.trim().to_string();
                }
            }
        } else if let Some(v) = t.strip_prefix("SANATTR=") {
            p.sans = parse_san(v);
        } else if let Some(v) = t.strip_prefix("FP=") {
            p.fingerprint = v.trim().to_string();
        } else if let Some(v) = t.strip_prefix("NAE=") {
            p.not_after_epoch = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = t.strip_prefix("CHAIN=") {
            p.chain_length = v.trim().parse().unwrap_or(0);
        } else if t.starts_with("Public Key Algorithm:") {
            let alg = t.split(':').nth(1).unwrap_or("").trim();
            p.key_info = match alg {
                "rsaEncryption" => "RSA".to_string(),
                "id-ecPublicKey" => "ECDSA".to_string(),
                "ED25519" | "id-Ed25519" => "Ed25519".to_string(),
                other => other.to_string(),
            };
        } else if t.starts_with("ASN1 OID:") {
            let oid = t.split(':').nth(1).unwrap_or("").trim();
            let curve = match oid {
                "prime256v1" => "P-256",
                "secp384r1" => "P-384",
                "secp521r1" => "P-521",
                "X25519" => "X25519",
                other => other,
            };
            if !p.key_info.is_empty() {
                p.key_info = format!("{} {}", p.key_info, curve);
            } else {
                p.key_info = curve.to_string();
            }
        } else if let Some(v) = t.strip_prefix("Public-Key:") {
            let bits = v
                .trim()
                .trim_start_matches('(')
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string();
            if !bits.is_empty() {
                p.key_info = if p.key_info.is_empty() {
                    format!("{} bit", bits)
                } else {
                    format!("{} {} bit", p.key_info, bits)
                };
            }
        }
    }

    p.days_remaining = if p.not_after_epoch > 0 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        (p.not_after_epoch - now) / 86_400
    } else {
        -1
    };
    if p.chain_length == 0 {
        p.chain_length = 1;
    }
    Ok(p)
}

fn parse_san(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim())
        .filter_map(|s| s.strip_prefix("DNS:").or_else(|| s.strip_prefix("DNS :")))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// 证书 SAN 是否覆盖某个域名（支持 `*.example.com` 通配一级）。
fn cert_covers(sans: &[String], domain: &str) -> bool {
    let d = domain.trim().to_ascii_lowercase();
    if d.is_empty() {
        return true;
    }
    sans.iter().any(|s| {
        let s = s.to_ascii_lowercase();
        if s == d {
            return true;
        }
        if let Some(suffix) = s.strip_prefix("*.") {
            // 通配只匹配一级子域
            if let Some(rest) = d.strip_suffix(suffix) {
                if rest.ends_with('.') && rest.trim_end_matches('.').find('.').is_none() {
                    return !rest.trim_end_matches('.').is_empty();
                }
            }
        }
        false
    })
}

fn finish_info(mut info: SslCertInfo, raw: &[String]) -> SslCertInfo {
    let mut probe = CertProbe::default();
    for line in raw {
        let t = line.trim();
        if let Some(v) = t.strip_prefix("CERT=") {
            info.cert_path = v.trim().to_string();
        } else if let Some(v) = t.strip_prefix("KEY=") {
            info.key_path = v.trim().to_string();
        } else if let Some(v) = t.strip_prefix("Subject:") {
            probe.subject = v.trim().to_string();
        } else if let Some(v) = t.strip_prefix("Issuer:") {
            probe.issuer = v.trim().to_string();
        } else if let Some(v) = t.strip_prefix("Not Before:") {
            probe.not_before = v.trim().to_string();
        } else if t.starts_with("Not After") {
            if let Some((_, v)) = t.split_once(':') {
                if !v.trim().is_empty() {
                    probe.not_after = v.trim().to_string();
                }
            }
        } else if let Some(v) = t.strip_prefix("SANATTR=") {
            probe.sans = parse_san(v);
        } else if let Some(v) = t.strip_prefix("FP=") {
            probe.fingerprint = v.trim().to_string();
        } else if let Some(v) = t.strip_prefix("NAE=") {
            probe.not_after_epoch = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = t.strip_prefix("CHAIN=") {
            probe.chain_length = v.trim().parse().unwrap_or(0);
        } else if t.starts_with("Public Key Algorithm:") {
            let alg = t.split(':').nth(1).unwrap_or("").trim();
            probe.key_info = match alg {
                "rsaEncryption" => "RSA".to_string(),
                "id-ecPublicKey" => "ECDSA".to_string(),
                "ED25519" | "id-Ed25519" => "Ed25519".to_string(),
                other => other.to_string(),
            };
        } else if t.starts_with("ASN1 OID:") {
            let oid = t.split(':').nth(1).unwrap_or("").trim();
            let curve = match oid {
                "prime256v1" => "P-256",
                "secp384r1" => "P-384",
                "secp521r1" => "P-521",
                other => other,
            };
            probe.key_info = if probe.key_info.is_empty() {
                curve.to_string()
            } else {
                format!("{} {}", probe.key_info, curve)
            };
        } else if let Some(v) = t.strip_prefix("Public-Key:") {
            let bits = v
                .trim()
                .trim_start_matches('(')
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string();
            if !bits.is_empty() {
                probe.key_info = if probe.key_info.is_empty() {
                    format!("{} bit", bits)
                } else {
                    format!("{} {} bit", probe.key_info, bits)
                };
            }
        }
    }
    info.has_cert = !info.cert_path.is_empty() && !probe.not_after.is_empty();
    info.subject = probe.subject;
    info.issuer = probe.issuer;
    info.not_before = probe.not_before;
    info.not_after = probe.not_after;
    info.sans = probe.sans;
    info.fingerprint = probe.fingerprint;
    info.key_info = probe.key_info;
    info.chain_length = probe.chain_length;
    info.days_remaining = if probe.not_after_epoch > 0 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        (probe.not_after_epoch - now) / 86_400
    } else {
        -1
    };
    info.source = detect_source(&info);
    info
}

fn detect_source(info: &SslCertInfo) -> String {
    if info.cert_path.is_empty() {
        return "none".to_string();
    }
    if info.cert_path.contains("/etc/letsencrypt/live/") {
        return "letsencrypt".to_string();
    }
    if info.cert_path.starts_with("/etc/nginx/ssl/")
        || info.cert_path.starts_with("/www/server/panel/vhost/cert/")
    {
        return "custom".to_string();
    }
    if !info.subject.is_empty()
        && !info.issuer.is_empty()
        && normalize_dn(&info.subject) == normalize_dn(&info.issuer)
    {
        return "selfsigned".to_string();
    }
    "other".to_string()
}

fn normalize_dn(s: &str) -> String {
    s.split(',')
        .map(|p| p.trim().to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// 计算证书目录与文件路径（宝塔面板走它自己的 cert 目录）。
pub fn cert_paths(domain: &str, config_path: &str) -> (String, String, String) {
    let dir = if config_path.starts_with("/www/server/") {
        format!("/www/server/panel/vhost/cert/{}", domain)
    } else {
        format!("/etc/nginx/ssl/{}", domain)
    };
    let cert = format!("{}/fullchain.pem", dir);
    let key = format!("{}/privkey.pem", dir);
    (dir, cert, key)
}

/// 生成提权安装脚本。脚本正文只含受校验的 domain/path，不含任何密钥内容。
#[allow(clippy::too_many_arguments)]
fn build_install_script(
    dir: &str,
    script_path: &str,
    config_path: &str,
    cert_dir: &str,
    log_path: &str,
    stop_letsencrypt: bool,
    domain: &str,
    self_signed: bool,
    days: &str,
    openssl_cnf: &str,
) -> String {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let backup = format!("{}.leepanel.bak.{}", config_path, stamp);
    let gen_block = if self_signed {
        format!(
            r#"
openssl req -x509 -nodes -newkey rsa:2048 -days {days} -keyout "$D/privkey.pem" -out "$D/fullchain.pem" -config '{cnf}' >"{log}" 2>&1
if [ $? -ne 0 ]; then echo "__LEEPANEL_GEN_FAIL__"; cat "{log}"; exit 10; fi
"#,
            days = days,
            cnf = openssl_cnf,
            log = log_path
        )
    } else {
        String::new()
    };
    let le_block = if stop_letsencrypt {
        format!(
            r#"
if command -v certbot >/dev/null 2>&1 && [ -d '/etc/letsencrypt/live/{d}' ]; then
  if certbot delete --cert-name '{d}' --non-interactive >/dev/null 2>&1; then
    echo "__LEEPANEL_LE_STOPPED__"
  fi
fi
true
"#,
            d = domain
        )
    } else {
        String::new()
    };
    format!(
        r#"set -u
D='{dir}'
S='{script}'
cleanup() {{ rm -rf "$D" "$S"; }}
trap cleanup EXIT
{gen}
mkdir -p '{certdir}' || exit 11
chmod 700 '{certdir}' || exit 12
install -m 644 "$D/fullchain.pem" '{certdir}/fullchain.pem' || exit 13
install -m 600 "$D/privkey.pem" '{certdir}/privkey.pem' || exit 14
chown root:root '{certdir}/privkey.pem' >/dev/null 2>&1 || true
command -v restorecon >/dev/null 2>&1 && restorecon -R '{certdir}' >/dev/null 2>&1 || true
cp -f '{cfg}' '{bak}' || exit 15
install -m 644 "$D/vhost.conf" '{cfg}' || exit 16
if nginx -t >"{log}" 2>&1; then
  if systemctl reload nginx >/dev/null 2>&1 || nginx -s reload >/dev/null 2>&1; then
    echo "__LEEPANEL_OK__"
    {le}
  else
    echo "__LEEPANEL_RELOAD_FAIL__"
    cat "{log}"
    exit 21
  fi
else
  cp -f '{bak}' '{cfg}'
  echo "__LEEPANEL_FAIL__"
  cat "{log}"
  exit 20
fi
"#,
        dir = dir,
        script = script_path,
        gen = gen_block,
        certdir = cert_dir,
        cfg = config_path,
        bak = backup,
        log = log_path,
        le = le_block,
    )
}

// ===== nginx 配置改写（纯文本，可单测）=====

/// 找出配置里所有 `server { ... }` 块的起止行号（含首尾行）。
fn server_block_ranges(lines: &[String]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let t = lines[i].trim_start();
        if t.starts_with("server") && t.contains('{') {
            let mut depth: i32 = 0;
            let mut j = i;
            while j < lines.len() {
                for ch in lines[j].chars() {
                    if ch == '{' {
                        depth += 1;
                    } else if ch == '}' {
                        depth -= 1;
                    }
                }
                if depth <= 0 {
                    break;
                }
                j += 1;
            }
            if j < lines.len() {
                out.push((i, j));
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn has_443_listen(lines: &[String]) -> bool {
    lines.iter().any(|l| {
        let t = l.trim_start();
        t.starts_with("listen ") && t.contains("443")
    })
}

fn cert_directive_value(line: &str, directive: &str) -> Option<String> {
    let t = line.trim();
    if !t.starts_with(directive) {
        return None;
    }
    // 避免把 ssl_certificate_key 当成 ssl_certificate
    let rest = t[directive.len()..].trim_start();
    if !rest.starts_with(|c: char| c.is_whitespace() || c == '/') {
        return None;
    }
    Some(
        rest.trim_end_matches(';')
            .trim()
            .trim_matches('"')
            .to_string(),
    )
}

fn indent_of(line: &str) -> String {
    line.chars().take_while(|c| *c == ' ' || *c == '\t').collect()
}

/// 把站点配置改成使用自定义证书；返回改写后的完整配置。
pub fn patch_config_for_custom_cert(
    config: &str,
    cert_path: &str,
    key_path: &str,
) -> Result<String, String> {
    let mut lines: Vec<String> = config.lines().map(|l| l.to_string()).collect();
    let has_cert = lines
        .iter()
        .any(|l| cert_directive_value(l, "ssl_certificate").is_some());

    if has_cert && has_443_listen(&lines) {
        // 情况 A：已有 HTTPS 块 → 只换证书路径（首次替换，多余的同类指令删除，避免 nginx 重复指令报错）
        let mut replaced_cert = false;
        let mut replaced_key = false;
        let mut out: Vec<String> = Vec::with_capacity(lines.len());
        for line in lines.drain(..) {
            if cert_directive_value(&line, "ssl_certificate_key").is_some() {
                if replaced_key {
                    continue;
                }
                replaced_key = true;
                out.push(format!("{}ssl_certificate_key {};", indent_of(&line), key_path));
                continue;
            }
            if cert_directive_value(&line, "ssl_certificate").is_some() {
                if replaced_cert {
                    continue;
                }
                replaced_cert = true;
                out.push(format!("{}ssl_certificate {};", indent_of(&line), cert_path));
                continue;
            }
            out.push(line);
        }
        // 缺 key 行时补在证书行后面
        if replaced_cert && !replaced_key {
            if let Some(pos) = out
                .iter()
                .position(|l| cert_directive_value(l, "ssl_certificate").is_some())
            {
                let ind = indent_of(&out[pos]);
                out.insert(pos + 1, format!("{}ssl_certificate_key {};", ind, key_path));
            }
        }
        return Ok(out.join("\n") + "\n");
    }

    // 情况 B：只有 HTTP 块 → 克隆出 HTTPS 块
    let ranges = server_block_ranges(&lines);
    let http_block = ranges.iter().find(|(s, e)| {
        lines[*s..=*e].iter().any(|l| {
            let t = l.trim_start();
            t.starts_with("listen ") && t.contains("80") && !t.contains("443")
        })
    });
    let (start, end) = match http_block {
        Some((s, e)) => (*s, *e),
        None => {
            return Err(
                "Cannot locate an HTTP server block for this site; please add it manually."
                    .to_string(),
            )
        }
    };

    let mut block: Vec<String> = lines[start..=end].to_vec();
    // listen 80 → listen 443 ssl
    let mut listen_seen = false;
    for line in block.iter_mut() {
        let t = line.trim_start();
        if t.starts_with("listen ") && t.contains("80") && !t.contains("443") {
            let ind = indent_of(line);
            if t.contains("[::]") {
                *line = format!("{}listen [::]:443 ssl;", ind);
            } else {
                *line = format!("{}listen 443 ssl;", ind);
            }
            listen_seen = true;
        }
    }
    if !listen_seen {
        return Err("Cannot derive an HTTPS listen directive from the existing config.".to_string());
    }
    // server_name 之后插入证书指令
    let sn_pos = block.iter().position(|l| {
        let t = l.trim_start();
        t.starts_with("server_name")
    });
    if let Some(pos) = sn_pos {
        let ind = indent_of(&block[pos]);
        block.insert(pos + 1, format!("{}ssl_certificate {};", ind, cert_path));
        block.insert(pos + 2, format!("{}ssl_certificate_key {};", ind, key_path));
    } else {
        let ind = indent_of(&block[0]);
        block.insert(1, format!("{}    ssl_certificate {};", ind, cert_path));
        block.insert(2, format!("{}    ssl_certificate_key {};", ind, key_path));
    }

    let mut out = lines.clone();
    out.push(String::new());
    out.extend(block);
    Ok(out.join("\n") + "\n")
}

/// 移除使用自定义证书的 HTTPS server 块（站点退回 HTTP）。
pub fn strip_custom_cert_config(config: &str, cert_path: &str) -> Result<String, String> {
    let lines: Vec<String> = config.lines().map(|l| l.to_string()).collect();
    let ranges = server_block_ranges(&lines);
    let mut remove: Vec<(usize, usize)> = Vec::new();
    for (s, e) in &ranges {
        let uses_custom = lines[*s..=*e]
            .iter()
            .any(|l| cert_directive_value(l, "ssl_certificate").as_deref() == Some(cert_path));
        if uses_custom {
            remove.push((*s, *e));
        }
    }
    if remove.is_empty() {
        return Err(
            "No LeePanel-managed certificate block found in this site's config.".to_string(),
        );
    }
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut idx = 0;
    while idx < lines.len() {
        if let Some((_, e)) = remove.iter().find(|(s, _)| *s == idx) {
            idx = *e + 1;
            continue;
        }
        out.push(lines[idx].clone());
        idx += 1;
    }
    // 去掉首尾多余空行
    while out.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
        out.pop();
    }
    Ok(out.join("\n") + "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const HTTP_ONLY: &str = r#"server {
    listen 80;
    listen [::]:80;
    server_name example.com www.example.com;
    root /var/www/example;
    index index.php index.html;

    location / {
        try_files $uri $uri/ /index.php?$query_string;
    }
}
"#;

    const WITH_CERTBOT: &str = r#"server {
    listen 80;
    server_name example.com;
    return 301 https://$host$request_uri;
}

server {
    listen 443 ssl;
    server_name example.com;
    ssl_certificate /etc/letsencrypt/live/example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/example.com/privkey.pem;
    root /var/www/example;
}
"#;

    #[test]
    fn pki_paths_standard_and_bt() {
        let (dir, cert, key) = cert_paths("example.com", "/etc/nginx/sites-available/example.com");
        assert_eq!(dir, "/etc/nginx/ssl/example.com");
        assert_eq!(cert, "/etc/nginx/ssl/example.com/fullchain.pem");
        assert!(key.ends_with("privkey.pem"));

        let (bt_dir, _, _) = cert_paths("a.com", "/www/server/panel/vhost/nginx/a.com.conf");
        assert_eq!(bt_dir, "/www/server/panel/vhost/cert/a.com");
    }

    #[test]
    fn cert_pem_structure_ok() {
        let pem = "-----BEGIN CERTIFICATE-----\nAAA\n-----END CERTIFICATE-----\n";
        assert_eq!(validate_cert_pem(pem).unwrap(), 1);
        let chain = format!("{}{}", pem, pem);
        assert_eq!(validate_cert_pem(&chain).unwrap(), 2);
    }

    #[test]
    fn cert_pem_rejects_garbage_and_keys() {
        assert!(validate_cert_pem("hello").is_err());
        let with_key = "-----BEGIN CERTIFICATE-----\nA\n-----END CERTIFICATE-----\n-----BEGIN PRIVATE KEY-----\nB\n-----END PRIVATE KEY-----\n";
        assert!(validate_cert_pem(with_key).is_err());
    }

    #[test]
    fn key_pem_detects_types_and_encryption() {
        assert!(matches!(
            validate_key_pem("-----BEGIN RSA PRIVATE KEY-----\nA\n-----END RSA PRIVATE KEY-----"),
            Ok(_)
        ));
        assert!(matches!(
            validate_key_pem("-----BEGIN PRIVATE KEY-----\nA\n-----END PRIVATE KEY-----"),
            Ok(_)
        ));
        assert!(matches!(
            validate_key_pem("-----BEGIN ENCRYPTED PRIVATE KEY-----\nA\n-----END ENCRYPTED PRIVATE KEY-----"),
            Err(KeyError::Encrypted)
        ));
        assert!(matches!(validate_key_pem("nope"), Err(KeyError::Invalid)));
    }

    #[test]
    fn patch_creates_https_block_from_http() {
        let out = patch_config_for_custom_cert(
            HTTP_ONLY,
            "/etc/nginx/ssl/example.com/fullchain.pem",
            "/etc/nginx/ssl/example.com/privkey.pem",
        )
        .unwrap();
        assert_eq!(out.matches("listen 443 ssl;").count(), 1);
        assert_eq!(out.matches("listen [::]:443 ssl;").count(), 1);
        assert!(out.contains("ssl_certificate /etc/nginx/ssl/example.com/fullchain.pem;"));
        assert!(out.contains("ssl_certificate_key /etc/nginx/ssl/example.com/privkey.pem;"));
        // HTTP 块必须保留
        assert!(out.contains("listen 80;"));
    }

    #[test]
    fn patch_replaces_letsencrypt_paths_only() {
        let out = patch_config_for_custom_cert(
            WITH_CERTBOT,
            "/etc/nginx/ssl/example.com/fullchain.pem",
            "/etc/nginx/ssl/example.com/privkey.pem",
        )
        .unwrap();
        assert!(!out.contains("/etc/letsencrypt/live/"));
        assert_eq!(out.matches("ssl_certificate /etc/nginx/ssl/example.com").count(), 1);
        assert_eq!(out.matches("listen 443 ssl;").count(), 1);
        assert!(out.contains("return 301 https://$host$request_uri;"));
    }

    #[test]
    fn patch_is_idempotent() {
        let once = patch_config_for_custom_cert(
            HTTP_ONLY,
            "/etc/nginx/ssl/example.com/fullchain.pem",
            "/etc/nginx/ssl/example.com/privkey.pem",
        )
        .unwrap();
        let twice = patch_config_for_custom_cert(
            &once,
            "/etc/nginx/ssl/example.com/fullchain.pem",
            "/etc/nginx/ssl/example.com/privkey.pem",
        )
        .unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn strip_removes_custom_block() {
        let patched = patch_config_for_custom_cert(
            HTTP_ONLY,
            "/etc/nginx/ssl/example.com/fullchain.pem",
            "/etc/nginx/ssl/example.com/privkey.pem",
        )
        .unwrap();
        let back = strip_custom_cert_config(&patched, "/etc/nginx/ssl/example.com/fullchain.pem")
            .unwrap();
        assert!(!back.contains("listen 443"));
        assert!(back.contains("listen 80;"));
        assert!(strip_custom_cert_config(HTTP_ONLY, "/etc/nginx/ssl/x/fullchain.pem").is_err());
    }

    #[test]
    fn san_wildcard_matching() {
        let sans = vec!["example.com".to_string(), "*.example.org".to_string()];
        assert!(cert_covers(&sans, "example.com"));
        assert!(cert_covers(&sans, "a.example.org"));
        assert!(!cert_covers(&sans, "a.b.example.org"));
        assert!(!cert_covers(&sans, "other.com"));
    }

    #[test]
    fn san_parsing() {
        let sans = parse_san("DNS:example.com, DNS:*.example.com, IP Address:1.2.3.4");
        assert_eq!(sans, vec!["example.com".to_string(), "*.example.com".to_string()]);
    }

    #[test]
    fn script_never_contains_pem() {
        let script = build_install_script(
            "/tmp/.leepanel-ssl-1",
            "/tmp/.leepanel-ssl-1.sh",
            "/etc/nginx/sites-available/example.com",
            "/etc/nginx/ssl/example.com",
            "/tmp/.leepanel-ssl-1/install.log",
            true,
            "example.com",
            false,
            "",
            "",
        );
        assert!(!script.contains("PRIVATE KEY"));
        assert!(script.contains("install -m 600"));
        assert!(script.contains("certbot delete --cert-name 'example.com'"));
        assert!(script.contains("__LEEPANEL_FAIL__"));
    }
}
