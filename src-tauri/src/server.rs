use crate::ssh::{SshSession, SshCache};
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use russh_keys::key::{KeyPair, SignatureHash};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

// ===== Data Structures =====

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OsInfo {
    pub distro: String,      // e.g. "Ubuntu", "CentOS"
    pub version: String,     // e.g. "22.04", "7"
    pub codename: String,    // e.g. "jammy" (Ubuntu only)
    pub family: String,      // "debian" or "rhel"
    pub kernel: String,
    pub arch: String,
    pub hostname: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DiskInfo {
    pub filesystem: String,
    pub size: String,
    pub used: String,
    pub available: String,
    pub use_percent: String,
    pub mount: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SystemInfo {
    pub os: OsInfo,
    pub uptime: String,
    pub load_avg: String,
    pub cpu_model: String,
    pub cpu_cores: u32,
    #[serde(default)]
    pub cpu_percent: u32,
    pub mem_total_mb: u64,
    pub mem_used_mb: u64,
    pub mem_free_mb: u64,
    pub swap_total_mb: u64,
    pub swap_used_mb: u64,
    pub disks: Vec<DiskInfo>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ServiceStatus {
    pub name: String,
    pub active: bool,
    pub status_text: String, // "active (running)", "inactive (dead)", etc.
    pub version: String,
}

// ===== OS Detection =====

/// Detect the operating system of the remote server
pub async fn detect_os(session: &SshSession, _cache: &SshCache, _session_id: &str) -> Result<OsInfo, String> {
    let (stdout, _, _) = crate::ssh::session_exec_with_output(session,
            r#"
# Detect distro
if [ -f /etc/os-release ]; then
  . /etc/os-release
  echo "DISTRO=$NAME"
  echo "VERSION=$VERSION_ID"
  echo "CODENAME=$VERSION_CODENAME"
  echo "ID=$ID"
  echo "ID_LIKE=$ID_LIKE"
elif [ -f /etc/redhat-release ]; then
  echo "DISTRO=$(cat /etc/redhat-release)"
  echo "VERSION=unknown"
  echo "ID=rhel"
fi
echo "KERNEL=$(uname -r)"
echo "ARCH=$(uname -m)"
echo "HOSTNAME=$(hostname)"
"#,
            15,
        )
        .await?;

    let mut info = OsInfo {
        distro: String::new(),
        version: String::new(),
        codename: String::new(),
        family: String::new(),
        kernel: String::new(),
        arch: String::new(),
        hostname: String::new(),
    };

    for line in stdout.lines() {
        if let Some((key, val)) = line.split_once('=') {
            let val = val.trim().trim_matches('"').to_string();
            match key.trim() {
                "DISTRO" => info.distro = val,
                "VERSION" => info.version = val,
                "CODENAME" => info.codename = val,
                "ID" | "ID_LIKE" => {
                    if info.family.is_empty() {
                        let lower = val.to_lowercase();
                        if lower.contains("debian") || lower.contains("ubuntu") {
                            info.family = "debian".to_string();
                        } else if lower.contains("rhel")
                            || lower.contains("centos")
                            || lower.contains("fedora")
                            || lower.contains("rocky")
                            || lower.contains("alma")
                        {
                            info.family = "rhel".to_string();
                        }
                    }
                }
                "KERNEL" => info.kernel = val,
                "ARCH" => info.arch = val,
                "HOSTNAME" => info.hostname = val,
                _ => {}
            }
        }
    }

    // Fallback: detect family from distro name if ID didn't work
    if info.family.is_empty() {
        let d = info.distro.to_lowercase();
        if d.contains("ubuntu") || d.contains("debian") {
            info.family = "debian".to_string();
        } else if d.contains("centos")
            || d.contains("rhel")
            || d.contains("red hat")
            || d.contains("rocky")
            || d.contains("alma")
        {
            info.family = "rhel".to_string();
        } else {
            info.family = "unknown".to_string();
        }
    }

    Ok(info)
}

// ===== System Info =====

/// Get comprehensive system information
pub async fn get_system_info(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
) -> Result<SystemInfo, String> {
    // ponytail: cache system info for 15s (memory/uptime/load change, but panel switches are fast)
    if let Some(cached) = cache.get(session_id, "system_info", 15) {
        if let Ok(info) = serde_json::from_str::<SystemInfo>(&cached) {
            return Ok(info);
        }
    }
    // ponytail: single SSH round-trip combining OS detection + system info (was 2 calls)
    let (stdout, _, _) = crate::ssh::session_exec_with_output(session,
            r#"
# OS Detection
if [ -f /etc/os-release ]; then
  . /etc/os-release
  echo "DISTRO=$NAME"
  echo "VERSION=$VERSION_ID"
  echo "CODENAME=$VERSION_CODENAME"
  echo "ID=$ID"
  echo "ID_LIKE=$ID_LIKE"
elif [ -f /etc/redhat-release ]; then
  echo "DISTRO=$(cat /etc/redhat-release)"
  echo "VERSION=unknown"
  echo "ID=rhel"
fi
echo "KERNEL=$(uname -r)"
echo "ARCH=$(uname -m)"
echo "HOSTNAME=$(hostname)"
# System info
echo "UPTIME=$(uptime -p 2>/dev/null || uptime)"
echo "LOAD=$(cat /proc/loadavg | awk '{print $1, $2, $3}')"
echo "CPU_MODEL=$(grep 'model name' /proc/cpuinfo | head -1 | cut -d: -f2 | xargs)"
echo "CPU_CORES=$(nproc)"
# CPU usage (quick snapshot from /proc/stat)
CPU_IDLE=$(awk '/^cpu / {print $5}' /proc/stat)
CPU_TOTAL=$(awk '/^cpu / {sum=$2+$3+$4+$5+$6+$7+$8; print sum}' /proc/stat)
if [ "$CPU_TOTAL" -gt 0 ]; then
  CPU_USED=$((100 - ($CPU_IDLE * 100 / $CPU_TOTAL)))
else
  CPU_USED=0
fi
echo "CPU_PERCENT=$CPU_USED"
free -m | awk '/^Mem:/ {print "MEM_TOTAL=" $2; print "MEM_USED=" $3; print "MEM_FREE=" $4}'
free -m | awk '/^Swap:/ {print "SWAP_TOTAL=" $2; print "SWAP_USED=" $3}'
echo "---DISKS---"
df -h --output=source,size,used,avail,pcent,target -x tmpfs -x devtmpfs -x squashfs 2>/dev/null | tail -n +2 || df -h | grep -v tmpfs | tail -n +2
"#,
            15,
        )
        .await?;

    let mut info = SystemInfo {
        os: OsInfo {
            distro: String::new(),
            version: String::new(),
            codename: String::new(),
            family: String::new(),
            kernel: String::new(),
            arch: String::new(),
            hostname: String::new(),
        },
        uptime: String::new(),
        load_avg: String::new(),
        cpu_model: String::new(),
        cpu_cores: 0,
        cpu_percent: 0,
        mem_total_mb: 0,
        mem_used_mb: 0,
        mem_free_mb: 0,
        swap_total_mb: 0,
        swap_used_mb: 0,
        disks: Vec::new(),
    };

    let mut in_disks = false;

    for line in stdout.lines() {
        if line.starts_with("---DISKS---") {
            in_disks = true;
            continue;
        }

        if in_disks {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 6 {
                info.disks.push(DiskInfo {
                    filesystem: parts[0].to_string(),
                    size: parts[1].to_string(),
                    used: parts[2].to_string(),
                    available: parts[3].to_string(),
                    use_percent: parts[4].to_string(),
                    mount: parts[5..].join(" "),
                });
            }
            continue;
        }

        if let Some((key, val)) = line.split_once('=') {
            let val = val.trim().trim_matches('"').to_string();
            match key.trim() {
                "DISTRO" => info.os.distro = val,
                "VERSION" => info.os.version = val,
                "CODENAME" => info.os.codename = val,
                "ID" | "ID_LIKE" => {
                    if info.os.family.is_empty() {
                        let lower = val.to_lowercase();
                        if lower.contains("debian") || lower.contains("ubuntu") {
                            info.os.family = "debian".to_string();
                        } else if lower.contains("rhel")
                            || lower.contains("centos")
                            || lower.contains("fedora")
                            || lower.contains("rocky")
                            || lower.contains("alma")
                        {
                            info.os.family = "rhel".to_string();
                        }
                    }
                }
                "KERNEL" => info.os.kernel = val,
                "ARCH" => info.os.arch = val,
                "HOSTNAME" => info.os.hostname = val,
                "UPTIME" => info.uptime = val.replace("up ", ""),
                "LOAD" => info.load_avg = val,
                "CPU_MODEL" => info.cpu_model = val,
                "CPU_CORES" => info.cpu_cores = val.parse().unwrap_or(0),
                "CPU_PERCENT" => info.cpu_percent = val.parse().unwrap_or(0),
                "MEM_TOTAL" => info.mem_total_mb = val.parse().unwrap_or(0),
                "MEM_USED" => info.mem_used_mb = val.parse().unwrap_or(0),
                "MEM_FREE" => info.mem_free_mb = val.parse().unwrap_or(0),
                "SWAP_TOTAL" => info.swap_total_mb = val.parse().unwrap_or(0),
                "SWAP_USED" => info.swap_used_mb = val.parse().unwrap_or(0),
                _ => {}
            }
        }
    }

    // Fallback: detect family from distro name if ID didn't work
    if info.os.family.is_empty() {
        let d = info.os.distro.to_lowercase();
        if d.contains("ubuntu") || d.contains("debian") {
            info.os.family = "debian".to_string();
        } else if d.contains("centos")
            || d.contains("rhel")
            || d.contains("red hat")
            || d.contains("rocky")
            || d.contains("alma")
        {
            info.os.family = "rhel".to_string();
        } else {
            info.os.family = "unknown".to_string();
        }
    }

    // ponytail: cache system info
    if let Ok(json) = serde_json::to_string(&info) {
        cache.put(session_id, "system_info", json);
    }
    Ok(info)
}

// ===== Service Status =====

/// Check status of LNMP services
pub async fn get_service_statuses(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
) -> Result<Vec<ServiceStatus>, String> {
    // ponytail: cache service statuses for 30s (changes only on start/stop)
    if let Some(cached) = cache.get(session_id, "service_statuses", 30) {
        if let Ok(statuses) = serde_json::from_str::<Vec<ServiceStatus>>(&cached) {
            return Ok(statuses);
        }
    }
    // ponytail: single SSH round-trip for all services (was ~10 sequential calls)
    let (stdout, _, _) = crate::ssh::session_exec_with_output(session,
            r#"
for svc in nginx php-fpm; do
  ACTIVE=$(systemctl is-active $svc 2>/dev/null)
  SUBSTATE=$(systemctl show $svc --property=SubState 2>/dev/null | cut -d= -f2)
  echo "SVC=$svc|ACTIVE=$ACTIVE|SUB=$SUBSTATE"
done
# ponytail: BT Panel installs binaries outside PATH, fallback to /www/server/ paths
_nver=$(nginx -v 2>&1 || /www/server/nginx/sbin/nginx -v 2>&1 || echo '')
echo "NGINX_VER=$(echo "$_nver" | grep -oP '[\d.]+' || echo '')"

_pver=$(php -v 2>/dev/null || $(ls /www/server/php/*/bin/php 2>/dev/null | tail -1) -v 2>/dev/null || echo '')
echo "PHP_VER=$(echo "$_pver" | head -1 | grep -oP '[\d]+\.[\d]+\.[\d]+' | head -1 || echo '')"
"#,
            15,
        )
        .await?;

    let mut statuses = Vec::new();
    let mut nginx_ver = String::new();
    let mut php_ver = String::new();

    for line in stdout.lines() {
        if line.starts_with("SVC=") {
            // Parse: SVC=name|ACTIVE=status|SUB=substate
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() >= 3 {
                let name = parts[0].strip_prefix("SVC=").unwrap_or("");
                let active_str = parts[1].strip_prefix("ACTIVE=").unwrap_or("inactive");
                let substate = parts[2].strip_prefix("SUB=").unwrap_or("");
                let active = active_str.trim() == "active";
                let status_text = if !substate.is_empty() {
                    substate.to_string()
                } else if active {
                    "running".to_string()
                } else {
                    "inactive".to_string()
                };
                // Skip non-existent services (systemctl returns "unknown" for is-active)
                if active_str.trim() != "unknown" || active {
                    statuses.push(ServiceStatus {
                        name: name.to_string(),
                        active,
                        status_text,
                        version: String::new(), // filled below
                    });
                }
            }
        } else if let Some((key, val)) = line.split_once('=') {
            let val = val.trim().to_string();
            match key.trim() {
                "NGINX_VER" => nginx_ver = val,
                "PHP_VER" => php_ver = val,
                _ => {}
            }
        }
    }

    // Assign versions
    for s in &mut statuses {
        s.version = match s.name.as_str() {
            "nginx" => nginx_ver.clone(),
            "php-fpm" => php_ver.clone(),
            _ => String::new(),
        };
    }

    // ponytail: cache service statuses
    if let Ok(json) = serde_json::to_string(&statuses) {
        cache.put(session_id, "service_statuses", json);
    }
    Ok(statuses)
}

// ===== LNMP Install =====

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LnmpInstallConfig {
    pub install_nginx: bool,
    pub install_php: bool,
    // ponytail: php_version removed — system package manager picks the version
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LnmpStatus {
    pub nginx_installed: bool,
    pub php_installed: bool,
    pub nginx_version: String,
    pub php_version: String,
}

/// Check which LNMP components are currently installed
pub async fn check_lnmp_status(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
) -> Result<LnmpStatus, String> {
    // ponytail: cache LNMP status for connection lifetime (changes only on install/uninstall)
    if let Some(cached) = cache.get(session_id, "lnmp_status", 0) {
        if let Ok(status) = serde_json::from_str::<LnmpStatus>(&cached) {
            return Ok(status);
        }
    }
    let (stdout, _, _) = crate::ssh::session_exec_with_output(session,
            r#"
echo "NGINX=$(( command -v nginx || [ -x /www/server/nginx/sbin/nginx ] ) >/dev/null 2>&1 && echo yes || echo no)"
echo "PHP=$(( command -v php || ls /www/server/php/*/bin/php >/dev/null 2>&1 ) >/dev/null 2>&1 && echo yes || echo no)"
echo "NGINX_VER=$(nginx -v 2>&1 || /www/server/nginx/sbin/nginx -v 2>&1 | grep -oP '[\d.]+' || echo '')"
echo "PHP_VER=$(php -v 2>/dev/null || $(ls /www/server/php/*/bin/php 2>/dev/null | tail -1) -v 2>/dev/null | head -1 | grep -oP '[\d]+\.[\d]+\.[\d]+' | head -1 || echo '')"
"#,
            15,
        )
        .await?;

    let mut status = LnmpStatus {
        nginx_installed: false,
        php_installed: false,
        nginx_version: String::new(),
        php_version: String::new(),
    };

    for line in stdout.lines() {
        if let Some((key, val)) = line.split_once('=') {
            match key.trim() {
                "NGINX" => status.nginx_installed = val.trim() == "yes",
                "PHP" => status.php_installed = val.trim() == "yes",
                "NGINX_VER" => status.nginx_version = val.trim().to_string(),
                "PHP_VER" => status.php_version = val.trim().to_string(),
                _ => {}
            }
        }
    }

    // ponytail: cache LNMP status
    if let Ok(json) = serde_json::to_string(&status) {
        cache.put(session_id, "lnmp_status", json);
    }
    Ok(status)
}

/// Generate an OS-appropriate LNMP install script
fn generate_install_script(os: &OsInfo, config: &LnmpInstallConfig) -> String {
    let mut script = String::new();

    script.push_str("#!/bin/bash\n");
    script.push_str("set -e\n");
    script.push_str("export DEBIAN_FRONTEND=noninteractive\n");
    script.push_str(r#"log() { echo "[$(date +%H:%M:%S)] $*"; }"#);
    script.push('\n');
    script.push_str(r#"err() { log "ERROR: $*"; exit 1; }"#);
    script.push('\n');
    script.push_str("log 'Starting LNMP installation...'\n");
    script.push_str(&format!("log 'System: {} {} ({})'\n", os.distro, os.version, os.family));

    if os.family == "debian" {
        script.push_str("\n# Update package index\n");
        script.push_str("log 'Updating package index...'\n");
        script.push_str("apt-get update -y --allow-releaseinfo-change || true\n");

        if config.install_nginx {
            script.push_str("\n# Install Nginx\n");
            script.push_str("log 'Installing Nginx...'\n");
            script.push_str("apt-get install -y nginx || err 'Failed to install Nginx'\n");
            script.push_str("systemctl enable nginx\n");
            script.push_str("systemctl start nginx\n");
            script.push_str("log 'Nginx installed successfully'\n");
        }

        if config.install_php {
            script.push_str("\n# Install PHP\n");
            script.push_str("log 'Installing PHP...'\n");
            script.push_str("apt-get install -y php-fpm php-mysql php-curl php-gd php-mbstring php-xml php-zip || err 'Failed to install PHP'\n");
            script.push_str("systemctl enable php-fpm\n");
            script.push_str("systemctl start php-fpm\n");
            script.push_str("log 'PHP installed successfully'\n");
        }
    } else if os.family == "rhel" {
        // CentOS / RHEL / Rocky / Alma
        let pkg_mgr = if os.version.starts_with('9') || os.version.starts_with("8.") {
            "dnf"
        } else {
            "yum"
        };

        script.push_str("\n# Install EPEL repository\n");
        script.push_str("log 'Installing EPEL repository...'\n");
        script.push_str(&format!("{} install -y epel-release || true\n", pkg_mgr));

        if config.install_nginx {
            script.push_str("\n# Install Nginx\n");
            script.push_str("log 'Installing Nginx...'\n");
            script.push_str(&format!("{} install -y nginx || err 'Failed to install Nginx'\n", pkg_mgr));
            script.push_str("systemctl enable nginx\n");
            script.push_str("systemctl start nginx\n");
            script.push_str("log 'Nginx installed successfully'\n");
        }

        if config.install_php {
            script.push_str("\n# Install PHP\n");
            script.push_str("log 'Installing PHP...'\n");
            script.push_str(&format!("{} install -y php-fpm php-mysqlnd php-gd php-mbstring php-xml php-zip || err 'Failed to install PHP'\n", pkg_mgr));
            script.push_str("systemctl enable php-fpm\n");
            script.push_str("systemctl start php-fpm\n");
            script.push_str("log 'PHP installed successfully'\n");
        }
    } else {
        return format!("#!/bin/bash\necho 'ERROR: Unsupported OS family: {}'\nexit 1\n", os.family);
    }

    // Firewall configuration hints
    script.push_str("\n# Configure firewall\n");
    if os.family == "debian" {
        script.push_str("if command -v ufw >/dev/null 2>&1; then\n");
        script.push_str("  log 'Configuring UFW firewall...'\n");
        if config.install_nginx {
            script.push_str("  ufw allow 'Nginx Full' || true\n");
        }
        script.push_str("fi\n");
    } else {
        script.push_str("if command -v firewall-cmd >/dev/null 2>&1; then\n");
        script.push_str("  log 'Configuring firewalld...'\n");
        if config.install_nginx {
            script.push_str("  firewall-cmd --permanent --add-service=http || true\n");
            script.push_str("  firewall-cmd --permanent --add-service=https || true\n");
        }
        script.push_str("  firewall-cmd --reload || true\n");
        script.push_str("fi\n");
    }

    script.push_str("\nlog 'LNMP installation completed successfully!'\n");
    script.push_str("echo 'INSTALL_SUCCESS'\n");

    script
}

/// Install LNMP stack on the remote server, emitting progress events
pub async fn install_lnmp(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
    config: &LnmpInstallConfig,
    app_handle: &AppHandle,
) -> Result<String, String> {
    // Detect OS first
    let os = detect_os(session, cache, session_id).await?;

    if os.family == "unknown" {
        return Err(format!("Unsupported operating system: {}", os.distro));
    }

    // Generate install script
    let script = generate_install_script(&os, config);

    // Write script to remote server
    crate::ssh::session_write_file(session, "/tmp/lnmp-install.sh", &script)
        .await?;

    // Make it executable and run it
    // Use a shell channel to stream output
    let mut channel = crate::ssh::session_open_channel(session).await?;
    channel
        .exec(true, "bash /tmp/lnmp-install.sh")
        .await
        .map_err(|e| format!("Failed to start install script: {}", e))?;

    let mut full_output = String::new();
    let mut exit_code: i32 = -1;
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(1800); // 30 min timeout

    loop {
        tokio::select! {
            msg = channel.wait() => {
                match msg {
                    Some(russh::ChannelMsg::Data { data }) => {
                        let text = String::from_utf8_lossy(&data);
                        full_output.push_str(&text);
                        // Emit every output line to the UI in real time
                        for line in text.lines() {
                            if !line.trim().is_empty() {
                                let _ = app_handle.emit("lnmp-install-progress", serde_json::json!({
                                    "sessionId": session_id,
                                    "line": line,
                                    "status": "installing",
                                }));
                            }
                        }
                    }
                    Some(russh::ChannelMsg::ExtendedData { data, ext }) => {
                        if ext == 1 {
                            let text = String::from_utf8_lossy(&data);
                            full_output.push_str(&text);
                            for line in text.lines() {
                                if !line.trim().is_empty() {
                                    let _ = app_handle.emit("lnmp-install-progress", serde_json::json!({
                                        "sessionId": session_id,
                                        "line": line,
                                        "status": "installing",
                                    }));
                                }
                            }
                        }
                    }
                    Some(russh::ChannelMsg::ExitStatus { exit_status }) => {
                        exit_code = exit_status as i32;
                    }
                    Some(russh::ChannelMsg::Eof) | Some(russh::ChannelMsg::Close) | None => break,
                    _ => {}
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                return Err("LNMP installation timed out (30 minutes)".to_string());
            }
        }
    }

    // Emit final status
    // ponytail: russh may deliver ExitStatus after Eof/Close, so exit_code stays -1.
    // Fall back to checking the script's own success marker in output.
    let script_succeeded = full_output.contains("INSTALL_SUCCESS");

    if exit_code == 0 || script_succeeded {
        let _ = app_handle.emit("lnmp-install-progress", serde_json::json!({
            "sessionId": session_id,
            "line": "Installation completed successfully!",
            "status": "done",
        }));
        Ok(full_output)
    } else {
        let _ = app_handle.emit("lnmp-install-progress", serde_json::json!({
            "sessionId": session_id,
            "line": format!("Installation failed (exit code {})", exit_code),
            "status": "error",
        }));
        Err(format!("Installation failed (exit code {}):\n{}", exit_code, full_output))
    }
}

// ===== Service Management =====

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ServiceInfo {
    pub name: String,
    pub display_name: String,
    pub active: bool,
    pub status_text: String,
    pub version: String,
    pub pid: String,
    pub memory: String,
    pub uptime: String,
    pub config_path: String,
}

/// Get detailed info for a specific service
pub async fn get_service_info(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    service: &str,
) -> Result<ServiceInfo, String> {
    // ponytail: single SSH round-trip combining status + version + config detection (was 2-3 calls)
    let cmd = match service {
        "nginx" => format!(r#"
echo "ACTIVE=$(systemctl is-active {svc} 2>/dev/null || echo inactive)"
echo "STATUS=$(systemctl show {svc} --property=ActiveState,SubState,MainPID,MemoryCurrent 2>/dev/null | paste -sd ',' -)"
echo "UPTIME=$(systemctl show {svc} --property=ActiveEnterTimestamp 2>/dev/null | cut -d= -f2-)"
echo "VER=$(nginx -v 2>&1 | grep -oP '[\d.]+' || echo '')"
"#, svc = service),
        "mysqld" | "mysql" | "mariadb" => format!(r#"
echo "ACTIVE=$(systemctl is-active {svc} 2>/dev/null || echo inactive)"
echo "STATUS=$(systemctl show {svc} --property=ActiveState,SubState,MainPID,MemoryCurrent 2>/dev/null | paste -sd ',' -)"
echo "UPTIME=$(systemctl show {svc} --property=ActiveEnterTimestamp 2>/dev/null | cut -d= -f2-)"
echo "VER=$(mysql --version 2>/dev/null | head -1 || echo '')"
echo "CFG=$(mysql --help 2>/dev/null | grep 'Default options' -A 1 | tail -1 | xargs || echo '')"
"#, svc = service),
        _ => format!(r#"
echo "ACTIVE=$(systemctl is-active {svc} 2>/dev/null || echo inactive)"
echo "STATUS=$(systemctl show {svc} --property=ActiveState,SubState,MainPID,MemoryCurrent 2>/dev/null | paste -sd ',' -)"
echo "UPTIME=$(systemctl show {svc} --property=ActiveEnterTimestamp 2>/dev/null | cut -d= -f2-)"
echo "VER=$(php -v 2>/dev/null | head -1 | grep -oP '[\d]+\.[\d]+\.[\d]+' | head -1 || echo '')"
echo "CFG=$(php -i 2>/dev/null | grep 'Loaded Configuration File' | head -1 | cut -d= -f2- | xargs || echo '')"
"#, svc = service),
    };

    let (stdout, _, _) = crate::ssh::session_exec_with_output(session, &cmd, 15)
        .await?;

    let mut info = ServiceInfo {
        name: service.to_string(),
        display_name: service.to_string(),
        active: false,
        status_text: String::new(),
        version: String::new(),
        pid: String::new(),
        memory: String::new(),
        uptime: String::new(),
        config_path: String::new(),
    };

    for line in stdout.lines() {
        if let Some((key, val)) = line.split_once('=') {
            let val = val.trim().to_string();
            match key.trim() {
                "ACTIVE" => {
                    info.active = val == "active";
                    info.status_text = val;
                }
                "STATUS" => {
                    for prop in val.split(',') {
                        if let Some((k, v)) = prop.split_once('=') {
                            match k {
                                "MainPID" => info.pid = v.to_string(),
                                "MemoryCurrent" => {
                                    if let Ok(bytes) = v.parse::<u64>() {
                                        info.memory = format!("{} MB", bytes / 1024 / 1024);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                "UPTIME" => info.uptime = val,
                "VER" => info.version = val,
                "CFG" => info.config_path = val,
                _ => {}
            }
        }
    }

    // Set display name and config path defaults based on service type
    match service {
        "nginx" => {
            info.display_name = "Nginx".to_string();
            if info.config_path.is_empty() {
                info.config_path = "/etc/nginx/nginx.conf".to_string();
            }
        }
        "mysqld" | "mysql" => {
            info.display_name = "MySQL".to_string();
            if info.config_path.is_empty() || !info.config_path.starts_with('/') {
                info.config_path = info.config_path.split_whitespace()
                    .next().unwrap_or("/etc/my.cnf").to_string();
            }
            if info.config_path.is_empty() {
                info.config_path = "/etc/my.cnf".to_string();
            }
        }
        "mariadb" => {
            info.display_name = "MariaDB".to_string();
            if info.config_path.is_empty() || !info.config_path.starts_with('/') {
                info.config_path = info.config_path.split_whitespace()
                    .next().unwrap_or("/etc/my.cnf").to_string();
            }
            if info.config_path.is_empty() {
                info.config_path = "/etc/my.cnf".to_string();
            }
        }
        "php-fpm" => {
            info.display_name = "PHP-FPM".to_string();
            if info.config_path.is_empty() {
                info.config_path = "/etc/php.ini".to_string();
            }
        }
        _ => {}
    }

    Ok(info)
}

/// Find the active MySQL/MariaDB service name in a single SSH call (was ~9 sequential calls)
pub async fn find_mysql_service(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
) -> Result<(String, ServiceInfo), String> {
    let (stdout, _, _) = crate::ssh::session_exec_with_output(session,
            r#"
for svc in mysqld mariadb mysql; do
  ACTIVE=$(systemctl is-active $svc 2>/dev/null || echo inactive)
  echo "SVC=$svc|ACTIVE=$ACTIVE"
done
"#,
            10,
        )
        .await?;

    // Find first active service, or first that exists
    let mut first_existing = String::new();
    let mut found_active: Option<String> = None;

    for line in stdout.lines() {
        if line.starts_with("SVC=") {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() >= 2 {
                let name = parts[0].strip_prefix("SVC=").unwrap_or("");
                let active = parts[1].strip_prefix("ACTIVE=").unwrap_or("inactive");
                if first_existing.is_empty() && active != "unknown" {
                    first_existing = name.to_string();
                }
                if active == "active" && found_active.is_none() {
                    found_active = Some(name.to_string());
                }
            }
        }
    }

    let service_name = found_active.unwrap_or(first_existing);
    if service_name.is_empty() {
        return Err("No MySQL/MariaDB service found".to_string());
    }

    let info = get_service_info(session, cache, session_id, &service_name).await?;
    Ok((service_name, info))
}

/// Find the active PHP-FPM service name in a single SSH call (was ~18 sequential calls)
pub async fn find_php_service(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
) -> Result<(String, ServiceInfo), String> {
    let (stdout, _, _) = crate::ssh::session_exec_with_output(session,
            r#"
for svc in php-fpm php8.4-fpm php8.3-fpm php8.2-fpm php8.1-fpm php8.0-fpm; do
  ACTIVE=$(systemctl is-active $svc 2>/dev/null || echo inactive)
  echo "SVC=$svc|ACTIVE=$ACTIVE"
done
"#,
            10,
        )
        .await?;

    let mut first_existing = String::new();
    let mut found_active: Option<String> = None;

    for line in stdout.lines() {
        if line.starts_with("SVC=") {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() >= 2 {
                let name = parts[0].strip_prefix("SVC=").unwrap_or("");
                let active = parts[1].strip_prefix("ACTIVE=").unwrap_or("inactive");
                if first_existing.is_empty() && active != "unknown" {
                    first_existing = name.to_string();
                }
                if active == "active" && found_active.is_none() {
                    found_active = Some(name.to_string());
                }
            }
        }
    }

    let service_name = found_active.unwrap_or(first_existing);
    if service_name.is_empty() {
        return Err("No PHP-FPM service found".to_string());
    }

    let info = get_service_info(session, cache, session_id, &service_name).await?;
    Ok((service_name, info))
}

/// Find the FPM pool config path in a single SSH call (was up to 6 sequential reads)
pub async fn find_php_fpm_config(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
) -> Result<(String, String), String> {
    let (stdout, _, _) = crate::ssh::session_exec_with_output(session,
            r#"
for p in /etc/php-fpm.d/www.conf /etc/php/8.4/fpm/pool.d/www.conf /etc/php/8.3/fpm/pool.d/www.conf /etc/php/8.2/fpm/pool.d/www.conf /etc/php/8.1/fpm/pool.d/www.conf /etc/php/8.0/fpm/pool.d/www.conf; do
  if [ -f "$p" ]; then
    echo "PATH=$p"
    cat "$p"
    exit 0
  fi
done
echo "NOT_FOUND"
"#,
            10,
        )
        .await?;

    if stdout.contains("NOT_FOUND") {
        return Err("FPM pool config not found".to_string());
    }

    let mut path = String::new();
    let mut content = String::new();
    for line in stdout.lines() {
        if line.starts_with("PATH=") {
            path = line.strip_prefix("PATH=").unwrap_or("").to_string();
        } else if !path.is_empty() {
            content.push_str(line);
            content.push('\n');
        }
    }

    if path.is_empty() {
        return Err("FPM pool config not found".to_string());
    }

    Ok((path, content))
}

/// Read a remote file's content via SSH exec
pub async fn read_remote_file(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    path: &str,
) -> Result<String, String> {
    let safe = path.replace('\'', "'\\''");
    let (stdout, stderr, code) = crate::ssh::session_exec_with_output(session, &format!("cat '{}'", safe), 10)
        .await?;
    if code != 0 {
        Err(format!("Failed to read {}: {}", path, stderr.trim()))
    } else {
        Ok(stdout)
    }
}

/// Write content to a remote file via SFTP
pub async fn write_remote_file(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    path: &str,
    content: &str,
) -> Result<(), String> {
    crate::ssh::session_write_file(session, path, content).await
}

/// Get recent log lines from a file
pub async fn get_log_lines(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    path: &str,
    lines: u32,
) -> Result<String, String> {
    let safe = path.replace('\'', "'\\''");
    let (stdout, _, _) = crate::ssh::session_exec_with_output(session, &format!("tail -{} '{}'", lines, safe), 10)
        .await?;
    Ok(stdout)
}

/// List Nginx virtual hosts
pub async fn list_nginx_vhosts(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
) -> Result<Vec<String>, String> {
    let (stdout, _, _) = crate::ssh::session_exec_with_output(session,
            r#"
for dir in /etc/nginx/conf.d /etc/nginx/sites-enabled /etc/nginx/sites-available /www/server/panel/vhost/nginx /www/server/nginx/conf/vhost; do
  [ -d "$dir" ] && find "$dir" -maxdepth 1 -type f 2>/dev/null
done | sort -u
"#,
            10,
        )
        .await?;

    let vhosts: Vec<String> = stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .filter(|l| {
            let name = l.rsplit('/').next().unwrap_or(l);
            name != "default"
        })
        .collect();

    Ok(vhosts)
}

/// Get Nginx configuration test result
pub async fn test_nginx_config(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
) -> Result<(bool, String), String> {
    let (stdout, stderr, code) = crate::ssh::session_exec_with_output(session, "nginx -t 2>&1", 10)
        .await?;
    let combined = format!("{} {}", stdout, stderr);
    let ok = code == 0 || combined.contains("test is successful") || combined.contains("syntax is ok");
    Ok((ok, combined.trim().to_string()))
}

/// Get MySQL global variables
pub async fn get_mysql_variables(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
) -> Result<Vec<(String, String)>, String> {
    let (stdout, _, _) = crate::ssh::session_exec_with_output(session,
            "mysql -e 'SHOW GLOBAL VARIABLES' 2>/dev/null | head -80",
            10,
        )
        .await?;

    let mut vars = Vec::new();
    for line in stdout.lines().skip(1) {
        if let Some((name, value)) = line.split_once('\t') {
            vars.push((name.trim().to_string(), value.trim().to_string()));
        }
    }
    Ok(vars)
}

/// Get MySQL process list
pub async fn get_mysql_processes(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
) -> Result<String, String> {
    let (stdout, _, _) = crate::ssh::session_exec_with_output(session,
            "mysql -e 'SHOW PROCESSLIST' 2>/dev/null",
            10,
        )
        .await?;
    Ok(stdout)
}

/// Execute a MySQL query
pub async fn exec_mysql_query(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    query: &str,
) -> Result<String, String> {
    let safe_query = query.replace('\'', "'\\''");
    let (stdout, stderr, code) = crate::ssh::session_exec_with_output(session,
            &format!("mysql -e '{}' 2>&1", safe_query),
            15,
        )
        .await?;
    let combined = format!("{} {}", stdout, stderr);
    // mysql errors start with "ERROR" (e.g. "ERROR 1045 (28000): Access denied")
    let has_error = combined.contains("ERROR ") || combined.contains("ERROR:");
    if code != 0 && has_error {
        Err(combined.trim().to_string())
    } else {
        Ok(stdout)
    }
}

// ===== Site Management =====

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SiteInfo {
    pub domain: String,
    pub domains: String,             // Space-separated list of all server_names
    pub root: String,
    pub config_path: String,
    pub ssl: bool,
    pub ssl_cert_path: Option<String>,
    pub ssl_key_path: Option<String>,
    pub php_version: String,
    pub running_dir: String,
    pub open_basedir: bool,
    pub enabled: bool,
    pub index_files: String,         // Space-separated index file list
    pub proxy_target: String,        // Detected proxy_pass URL
    pub hotlink_enabled: bool,       // Hotlink protection enabled
    pub hotlink_extensions: String,  // Comma-separated file extensions
    pub hotlink_allowed_domains: String, // Newline-separated allowed domains
    pub hotlink_response: String,    // Response code or path
    pub hotlink_allow_empty_referer: bool, // Allow empty referer
    pub created_at: i64,
}

/// List installed PHP-FPM versions on the server
pub async fn list_php_versions(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
) -> Result<Vec<String>, String> {
    // ponytail: cache PHP versions for connection lifetime
    if let Some(cached) = cache.get(session_id, "php_versions", 0) {
        if let Ok(versions) = serde_json::from_str::<Vec<String>>(&cached) {
            return Ok(versions);
        }
    }
    let (stdout, _, _) = crate::ssh::session_exec_with_output(session,
            r#"
# ponytail: dynamic scan — no hardcoded version list
# From systemd unit files
systemctl list-unit-files --type=service 2>/dev/null | grep -oE 'php[0-9]+\.[0-9]+-fpm' | sed -E 's/^php([0-9]+\.[0-9]+)-fpm$/\1/' | sort -V | uniq
# BT Panel: versions not in systemd
if [ -d /www/server/php ]; then
  for d in /www/server/php/*/; do
    [ -d "$d" ] || continue
    v=$(basename "$d")
    echo "$v" | grep -qE '^[0-9]+\.[0-9]+$' || continue
    [ -x "/www/server/php/$v/sbin/php-fpm" ] || continue
    echo "$v"
  done | sort -V | uniq
fi
"#,
            10,
        )
        .await?;

    let versions: Vec<String> = stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    // ponytail: cache PHP versions
    if let Ok(json) = serde_json::to_string(&versions) {
        cache.put(session_id, "php_versions", json);
    }
    Ok(versions)
}

/// List immediate subdirectories of a given path
pub async fn list_subdirs(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    path: &str,
) -> Result<Vec<String>, String> {
    let safe_path = path.replace('\'', "'\\''");
    let (stdout, _, _) = crate::ssh::session_exec_with_output(session,
            &format!("find '{}' -maxdepth 1 -mindepth 1 -type d -printf '%f\\n' 2>/dev/null | sort", safe_path),
            10,
        )
        .await?;

    let dirs: Vec<String> = stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    Ok(dirs)
}

/// List all configured sites from Nginx
pub async fn list_sites(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
) -> Result<Vec<SiteInfo>, String> {
    let (stdout, _, _) = crate::ssh::session_exec_with_output(session,
            r##"
# Parse nginx vhost configs — scan both enabled and disabled
seen=""

# 1) Scan enabled configs (standard + BT Panel vhost paths)
for dir in /etc/nginx/sites-enabled /etc/nginx/conf.d /www/server/panel/vhost/nginx /www/server/nginx/conf/vhost; do
  if [ -d "$dir" ]; then
    for f in "$dir"/*; do
      [ -f "$f" ] || continue
      # Skip default config
      case "$(basename "$f")" in default) continue;; esac
      # Skip .disabled files in conf.d (not enabled)
      case "$f" in *.conf.disabled) continue;; esac
      echo "===TIME:$(stat -c %Y "$f" 2>/dev/null || echo 0)==="
      echo "===FILE:$f==="
      cat "$f" 2>/dev/null
      # ponytail: detect PHP version from includes and running sockets
      _pv=$(grep -ohP 'php\K[0-9]+\.[0-9]+' "$f" 2>/dev/null | head -1)
      if [ -z "$_pv" ]; then
        _inc=$(grep -oP 'include\s+\K[^;]+' "$f" 2>/dev/null | tr -d " '" | while read _ip; do
          for _g in $_ip; do [ -f "$_g" ] && grep -ohP 'php\K[0-9]+\.[0-9]+' "$_g" 2>/dev/null; done
        done | head -1)
        [ -n "$_inc" ] && _pv="$_inc"
      fi
      if [ -z "$_pv" ] && grep -q 'fastcgi_pass' "$f" 2>/dev/null; then
        _sk=$(ls /run/php/php*-fpm.sock /var/run/php/php*-fpm.sock 2>/dev/null | head -1)
        [ -n "$_sk" ] && _pv=$(echo "$_sk" | grep -oP 'php\K[0-9]+\.[0-9]+')
      fi
      [ -n "$_pv" ] && echo "# __PHP_FPM:$_pv"
      # ponytail: explicit SSL marker for reliable detection
      grep -q 'ssl_certificate' "$f" 2>/dev/null && echo '# __SSL:1' || echo '# __SSL:0'
      # Record the server_name for dedup
      sn=$(grep -E '^[[:space:]]*server_name[[:space:]]+' "$f" 2>/dev/null | head -1 | sed 's/.*server_name[[:space:]]*//' | sed 's/;.*//' | awk '{print $1}')
      [ -n "$sn" ] && seen="$seen $sn"
    done
  fi
done

# 2) Scan disabled configs (sites-available without symlink + conf.d/*.conf.disabled)

# sites-available
if [ -d /etc/nginx/sites-available ]; then
  for f in /etc/nginx/sites-available/*; do
    [ -f "$f" ] || continue
    # Skip default config
    case "$(basename "$f")" in default) continue;; esac
    sn=$(grep -E '^[[:space:]]*server_name[[:space:]]+' "$f" 2>/dev/null | head -1 | sed 's/.*server_name[[:space:]]*//' | sed 's/;.*//' | awk '{print $1}')
    [ -z "$sn" ] && continue
    # Skip if already seen as enabled
    echo "$seen" | grep -qw "$sn" && continue
    echo "===TIME:$(stat -c %Y "$f" 2>/dev/null || echo 0)==="
    echo "===FILE:$f==="
    cat "$f" 2>/dev/null
    _pv=$(grep -ohP 'php\K[0-9]+\.[0-9]+' "$f" 2>/dev/null | head -1)
    if [ -z "$_pv" ]; then
      _inc=$(grep -oP 'include\s+\K[^;]+' "$f" 2>/dev/null | tr -d " '" | while read _ip; do
        for _g in $_ip; do [ -f "$_g" ] && grep -ohP 'php\K[0-9]+\.[0-9]+' "$_g" 2>/dev/null; done
      done | head -1)
      [ -n "$_inc" ] && _pv="$_inc"
    fi
    if [ -z "$_pv" ] && grep -q 'fastcgi_pass' "$f" 2>/dev/null; then
      _sk=$(ls /run/php/php*-fpm.sock /var/run/php/php*-fpm.sock 2>/dev/null | head -1)
      [ -n "$_sk" ] && _pv=$(echo "$_sk" | grep -oP 'php\K[0-9]+\.[0-9]+')
    fi
    [ -n "$_pv" ] && echo "# __PHP_FPM:$_pv"
    grep -q 'ssl_certificate' "$f" 2>/dev/null && echo '# __SSL:1' || echo '# __SSL:0'
  done
fi

# conf.d .disabled files
if [ -d /etc/nginx/conf.d ]; then
  for f in /etc/nginx/conf.d/*.conf.disabled; do
    [ -f "$f" ] || continue
    echo "===TIME:$(stat -c %Y "$f" 2>/dev/null || echo 0)==="
    echo "===FILE:$f==="
    cat "$f" 2>/dev/null
    _pv=$(grep -ohP 'php\K[0-9]+\.[0-9]+' "$f" 2>/dev/null | head -1)
    if [ -z "$_pv" ]; then
      _inc=$(grep -oP 'include\s+\K[^;]+' "$f" 2>/dev/null | tr -d " '" | while read _ip; do
        for _g in $_ip; do [ -f "$_g" ] && grep -ohP 'php\K[0-9]+\.[0-9]+' "$_g" 2>/dev/null; done
      done | head -1)
      [ -n "$_inc" ] && _pv="$_inc"
    fi
    if [ -z "$_pv" ] && grep -q 'fastcgi_pass' "$f" 2>/dev/null; then
      _sk=$(ls /run/php/php*-fpm.sock /var/run/php/php*-fpm.sock 2>/dev/null | head -1)
      [ -n "$_sk" ] && _pv=$(echo "$_sk" | grep -oP 'php\K[0-9]+\.[0-9]+')
    fi
    [ -n "$_pv" ] && echo "# __PHP_FPM:$_pv"
    grep -q 'ssl_certificate' "$f" 2>/dev/null && echo '# __SSL:1' || echo '# __SSL:0'
  done
fi
"##,
            15,
        )
        .await?;

    let mut sites: Vec<SiteInfo> = Vec::new();
    let mut current_file = String::new();
    let mut current_content = String::new();

    for line in stdout.lines() {
        if line.starts_with("===TIME:") && line.ends_with("===") {
            // ponytail: TIME lines parsed but ignored — creation time now tracked in site_metadata DB
            continue;
        } else if line.starts_with("===FILE:") && line.ends_with("===") {
            // Process previous file
            if !current_file.is_empty() {
                if let Some(site) = parse_site_config(&current_file, &current_content) {
                    sites.push(site);
                }
            }
            current_file = line
                .trim_start_matches("===FILE:")
                .trim_end_matches("===")
                .to_string();
            current_content.clear();
        } else {
            current_content.push_str(line);
            current_content.push('\n');
        }
    }
    // Process last file
    if !current_file.is_empty() {
        if let Some(site) = parse_site_config(&current_file, &current_content) {
            sites.push(site);
        }
    }

    // Dedup by domain (keep first occurrence = enabled)
    let mut seen_domains = std::collections::HashSet::new();
    sites.retain(|s| seen_domains.insert(s.domain.clone()));

    Ok(sites)
}

/// Parse a single nginx vhost config to extract site info
fn parse_site_config(path: &str, content: &str) -> Option<SiteInfo> {
    // ponytail: extract all server_names from all server_name directives, dedupe to handle Certbot's multiple server blocks
    use std::collections::HashSet;
    let domains: Vec<String> = content
        .lines()
        .filter(|l| l.trim().starts_with("server_name"))
        .flat_map(|l| {
            l.trim()
                .strip_prefix("server_name")
                .map(|s| s.trim().trim_end_matches(';').trim())
                .unwrap_or("")
                .split_whitespace()
                .map(String::from)
                .collect::<Vec<_>>()
        })
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    
    let domains_str = if domains.is_empty() {
        "unknown".to_string()
    } else {
        domains.join(" ")
    };
    
    let domain = domains.first().cloned().unwrap_or_else(|| "unknown".to_string());

    // Skip if no server_name found and filename is default-like
    if domain == "unknown" && (path.contains("default") || path.contains("default.conf")) {
        return None;
    }

    let root = content
        .lines()
        .find(|l| l.trim().starts_with("root "))
        .and_then(|l| {
            l.trim()
                .strip_prefix("root ")
                .map(|s| s.trim().trim_end_matches(';').trim().to_string())
        })
        .unwrap_or_else(|| format!("/var/www/{}", domain));

    // ponytail: use explicit SSL marker from shell script, fall back to content scan
    let ssl = content.lines().any(|l| l.trim() == "# __SSL:1")
        || content.contains("ssl_certificate")
        || content.contains("listen 443");

    // ponytail: parse SSL cert/key paths from config
    let ssl_cert_path = content.lines()
        .find(|l| l.trim().starts_with("ssl_certificate "))
        .and_then(|l| l.trim().strip_prefix("ssl_certificate ").map(|s| s.trim().trim_end_matches(';').trim().to_string()));
    let ssl_key_path = content.lines()
        .find(|l| l.trim().starts_with("ssl_certificate_key "))
        .and_then(|l| l.trim().strip_prefix("ssl_certificate_key ").map(|s| s.trim().trim_end_matches(';').trim().to_string()));

    let php_version = content
        .lines()
        .find(|l| l.starts_with("# __PHP_FPM:"))
        .map(|l| l.trim_start_matches("# __PHP_FPM:").trim().to_string())
        .or_else(|| {
            // Fallback: scan content for php{ver}-fpm patterns
            let lower = content.to_lowercase();
            for pat in &["php-fpm", "php_fpm"] {
                let mut start = 0;
                while let Some(idx) = lower[start..].find(pat) {
                    let abs = start + idx + pat.len();
                    let rest: String = lower[abs..].chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
                    if !rest.is_empty() && rest.contains('.') {
                        return Some(rest);
                    }
                    start = abs;
                }
            }
            None
        })
        .unwrap_or_default();

    // ponytail: a site is disabled if config ends with .disabled; otherwise enabled if in sites-enabled or conf.d
    let enabled = !path.ends_with(".disabled") && (path.contains("sites-enabled") || path.contains("conf.d"));

    // Detect running_dir: from comment marker, default to "/"
    let running_dir = content
        .lines()
        .find(|l| l.starts_with("# __RUNNING_DIR:"))
        .map(|l| l.trim_start_matches("# __RUNNING_DIR:").trim().to_string())
        .unwrap_or_else(|| "/".to_string());

    // Strip running_dir from nginx root to get the true web root
    let web_root = if running_dir != "/" {
        let suffix = running_dir.trim_start_matches('/');
        root.strip_suffix(&format!("/{}", suffix)).unwrap_or(&root).to_string()
    } else {
        root.clone()
    };

    // Detect open_basedir: check if PHP_ADMIN_VALUE open_basedir exists in config
    let open_basedir = content.contains("PHP_ADMIN_VALUE") && content.contains("open_basedir");

    // ponytail: parse index files from the index directive
    let index_files = content.lines()
        .find(|l| l.trim().starts_with("index "))
        .and_then(|l| l.trim().strip_prefix("index ").map(|s| s.trim().trim_end_matches(';').trim().to_string()))
        .unwrap_or_else(|| "index.php index.html index.htm".to_string());

    // ponytail: detect proxy_pass URL from config (first occurrence)
    let proxy_target = content.lines()
        .find(|l| l.trim().starts_with("proxy_pass "))
        .and_then(|l| l.trim().strip_prefix("proxy_pass ").map(|s| s.trim().trim_end_matches(';').trim().to_string()))
        .unwrap_or_default();

    // ponytail: parse hotlink protection from config markers
    let hotlink_enabled = content.contains("# Hotlink Protection Start");
    
    let (hotlink_extensions, hotlink_allowed_domains, hotlink_response, hotlink_allow_empty_referer) = if hotlink_enabled {
        // Extract the hotlink block
        let mut in_hotlink_block = false;
        let mut hotlink_content = String::new();
        for line in content.lines() {
            if line.contains("# Hotlink Protection Start") {
                in_hotlink_block = true;
                continue;
            }
            if line.contains("# Hotlink Protection End") {
                break;
            }
            if in_hotlink_block {
                hotlink_content.push_str(line);
                hotlink_content.push('\n');
            }
        }
        
        // Parse extensions from location ~* \.(ext)$ pattern
        let extensions = hotlink_content
            .lines()
            .find(|l| l.contains("location ~* \\.") && l.contains("$"))
            .and_then(|l| {
                l.split("\\.").nth(1)
                    .and_then(|s| s.split('$').next())
                    .map(|s| {
                        // Remove parentheses and convert | to commas
                        s.replace('(', "")
                            .replace(')', "")
                            .replace('|', ",")
                    })
            })
            .unwrap_or_else(|| "jpg,jpeg,gif,png,js,css".to_string());
        
        // Parse valid_referers to extract allowed domains
        let referers_line = hotlink_content
            .lines()
            .find(|l| l.trim().starts_with("valid_referers"));
        
        eprintln!("[DEBUG] referers_line: {:?}", referers_line);
        
        let mut allowed_domains_list: Vec<String> = Vec::new();
        let mut allow_empty = false;
        
        if let Some(ref_line) = referers_line {
            let parts: Vec<&str> = ref_line
                .trim()  // Remove leading/trailing whitespace first
                .strip_prefix("valid_referers")
                .unwrap_or("")
                .trim()
                .trim_end_matches(';')
                .split_whitespace()
                .collect();
            
            eprintln!("[DEBUG] parts: {:?}", parts);
            
            for part in parts {
                if part == "none" {
                    allow_empty = true;
                } else if part.starts_with("*.") {
                    // *.example.com -> example.com
                    let domain = part.strip_prefix("*.").unwrap_or(part).to_string();
                    eprintln!("[DEBUG] Adding wildcard domain: {}", domain);
                    if !allowed_domains_list.contains(&domain) {
                        allowed_domains_list.push(domain);
                    }
                } else if part != "blocked" && part != "server_names" {
                    let domain_str = part.to_string();
                    eprintln!("[DEBUG] Adding regular domain: {}", domain_str);
                    if !allowed_domains_list.contains(&domain_str) {
                        allowed_domains_list.push(domain_str);
                    }
                }
            }
        }
        
        // Join domains with newlines (preserving order)
        let allowed_domains = allowed_domains_list.join("\n");
        eprintln!("[DEBUG] Final allowed_domains: {:?}", allowed_domains);
        
        // Parse response directive
        let response = hotlink_content
            .lines()
            .find(|l| l.trim().starts_with("return "))
            .and_then(|l| {
                let trimmed = l.trim().strip_prefix("return ")?.trim_end_matches(';').trim();
                // Extract just the code or path
                if let Some(code) = trimmed.split_whitespace().next() {
                    // If it's a number, return it; otherwise return the full directive
                    if code.parse::<u16>().is_ok() {
                        Some(code.to_string())
                    } else {
                        Some(trimmed.to_string())
                    }
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "403".to_string());
        
        (extensions, allowed_domains, response, allow_empty)
    } else {
        ("".to_string(), "".to_string(), "".to_string(), false)
    };

    Some(SiteInfo {
        domain,
        domains: domains_str,
        root: web_root,
        config_path: path.to_string(),
        ssl,
        ssl_cert_path,
        ssl_key_path,
        php_version,
        running_dir,
        open_basedir,
        enabled,
        index_files,
        proxy_target,
        hotlink_enabled,
        hotlink_extensions,
        hotlink_allowed_domains,
        hotlink_response,
        hotlink_allow_empty_referer,
        created_at: 0, // set by caller from site_metadata DB
    })
}

/// Create a new site with Nginx vhost configuration
pub async fn create_site(
    session: &SshSession,
    _cache: &SshCache,
    session_id: &str,
    domain: &str,
    root: &str,
    php_version: &str,
    running_dir: &str,
    open_basedir: bool,
    use_ssl: bool,
    create_db: bool,
    db_name: &str,
    db_user: &str,
    db_pass: &str,
    app_handle: &AppHandle,
) -> Result<(String, String), String> {
    // Check if nginx is installed (comprehensive check for standard + BT Panel installations)
    let emit = |line: &str| {
        let _ = app_handle.emit("site-create-progress", serde_json::json!({
            "sessionId": session_id,
            "domain": domain,
            "line": line,
            "status": "running",
        }));
    };
    
    emit(&format!("Starting site creation for {}...", domain));
    
    // Check if nginx is installed
    let nginx_check_cmd = r#"which nginx 2>/dev/null || command -v nginx 2>/dev/null || [ -x /www/server/nginx/sbin/nginx ] && echo 'found' || echo ''"#;
    emit(&format!("Command: {}", nginx_check_cmd));
    let (nginx_check_out, nginx_check_err, _nginx_check_code) = crate::ssh::session_exec_with_output(session, nginx_check_cmd, 5)
        .await?;
    if !nginx_check_out.trim().is_empty() {
        emit("STDOUT: Nginx found");
    }
    if !nginx_check_err.trim().is_empty() {
        emit(&format!("STDERR: {}", nginx_check_err.trim()));
    }
    if nginx_check_out.trim().is_empty() {
        return Err("Please install nginx first before creating a site.".to_string());
    }
    emit("✓ Nginx detected");

    let safe_domain = domain.replace('\'', "'\\''");
    let safe_root = root.replace('\'', "'\\''");

    // Detect OS, nginx user, vhost layout, PHP socket
    let detect_cmd = r#"
# Detect OS
if [ -f /etc/os-release ]; then
  . /etc/os-release
  echo "FAMILY=$ID_LIKE"
  echo "ID=$ID"
fi

# Detect nginx user
NGINX_USER=$(ps aux | grep -E '^www-data ' 2>/dev/null | head -1)
if [ -n "$NGINX_USER" ]; then
  echo "NGINX_USER=www-data"
else
  NGINX_USER=$(ps aux | grep -E '^nginx ' 2>/dev/null | head -1)
  if [ -n "$NGINX_USER" ]; then
    echo "NGINX_USER=nginx"
  else
    # Fallback: check /etc/nginx/nginx.conf
    echo "NGINX_USER=$(grep -E '^user[[:space:]]+' /etc/nginx/nginx.conf 2>/dev/null | awk '{print $2}' | tr -d ';' || echo 'www-data')"
  fi
fi

# Detect vhost layout
echo "VHOST_DIR=$([ -d /etc/nginx/sites-available ] && echo 'sites-available' || echo 'conf.d')"

# Detect PHP-FPM socket
if command -v php-fpm &>/dev/null || command -v php-fpm$(php -v 2>/dev/null | head -1 | grep -oP '[\d]+\.[\d]+' | head -1) &>/dev/null; then
  SOCK=$(ls /run/php/php*-fpm.sock /var/run/php/php*-fpm.sock /run/php-fpm/www.sock /var/run/php-fpm/www.sock 2>/dev/null | head -1)
  echo "PHP_SOCK=$SOCK"
fi

# Check if nginx snippets exist
echo "HAS_FCGI_SNIPPET=$([ -f /etc/nginx/snippets/fastcgi-php.conf ] && echo '1' || echo '0')"
"#;
    
    emit("Detecting system configuration...");
    emit(&format!("Command: {}", detect_cmd.trim()));
    let (detect_out, detect_err, _) = crate::ssh::session_exec_with_output(session, detect_cmd, 10)
        .await?;
    
    if !detect_out.trim().is_empty() {
        emit("STDOUT:");
        for line in detect_out.lines() {
            emit(line);
        }
    }
    if !detect_err.trim().is_empty() {
        emit(&format!("STDERR: {}", detect_err.trim()));
    }

    let get = |key: &str| -> String {
        detect_out
            .lines()
            .find(|l| l.starts_with(key))
            .map(|l| l.split('=').nth(1).unwrap_or("").trim().to_string())
            .unwrap_or_default()
    };

    let family = get("FAMILY");
    let os_id = get("ID");
    let is_debian = family.contains("debian") || os_id == "ubuntu" || os_id == "debian";
    let nginx_user = get("NGINX_USER");
    let nginx_user = if nginx_user.is_empty() { if is_debian { "www-data".to_string() } else { "nginx".to_string() } } else { nginx_user };
    let vhost_dir = get("VHOST_DIR");
    let uses_sites = vhost_dir == "sites-available";
    let php_sock = get("PHP_SOCK");
    let has_fcgi_snippet = get("HAS_FCGI_SNIPPET") == "1";

    let config_path = if uses_sites {
        format!("/etc/nginx/sites-available/{}", domain)
    } else {
        format!("/etc/nginx/conf.d/{}.conf", domain)
    };

    // Compute effective nginx root: web_root + running_dir
    let running_dir_clean = running_dir.trim().trim_start_matches('/');
    let effective_root = if running_dir_clean.is_empty() {
        safe_root.clone()
    } else {
        format!("{}/{}", safe_root, running_dir_clean)
    };

    // Build PHP socket path — always derive from requested version, use detected sock as fallback only
    let php_sock = if php_version.is_empty() {
        String::new()
    } else if is_debian {
        format!("/run/php/php{}-fpm.sock", php_version)
    } else {
        // RHEL/CentOS: version-specific socket, or fallback to generic
        let versioned = format!("/run/php-fpm/www-{}.sock", php_version);
        if !php_sock.is_empty() && php_sock.contains(php_version) {
            php_sock
        } else {
            // ponytail: try common RHEL socket paths for the requested version
            versioned
        }
    };

    let has_php = !php_sock.is_empty();

    // Build nginx config — handle both Debian (snippets) and RHEL/CentOS (inline)
    let open_basedir_line = if open_basedir && has_php {
        format!("\n        fastcgi_param PHP_ADMIN_VALUE \"open_basedir={}:/tmp/\";", safe_root)
    } else {
        String::new()
    };
    let fastcgi_block = if has_php {
        if has_fcgi_snippet {
            format!(r#"
    location ~ \.php$ {{
        include snippets/fastcgi-php.conf;
        fastcgi_pass unix:{sock};{oba}
    }}
"#, sock = php_sock, oba = open_basedir_line)
        } else {
            format!(r#"
    location ~ \.php$ {{
        fastcgi_split_path_info ^(.+\.php)(/.+)$;
        fastcgi_pass unix:{sock};
        fastcgi_index index.php;
        include fastcgi_params;
        fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;
        fastcgi_param PATH_INFO $fastcgi_path_info;{oba}
    }}
"#, sock = php_sock, oba = open_basedir_line)
        }
    } else {
        String::new()
    };

    let try_files = if has_php {
        "try_files $uri $uri/ /index.php?$query_string;"
    } else {
        "try_files $uri $uri/ =404;"
    };

    let nginx_conf = format!(
        r#"server {{
    listen 80;
    listen [::]:80;
    server_name {domain};
    root {root};
    index index.php index.html index.htm;

    location / {{
        {try_files}
    }}
{fastcgi}
    location ~ /\.ht {{
        deny all;
    }}

    access_log /var/log/nginx/{domain}.access.log;
    error_log /var/log/nginx/{domain}.error.log;
# __RUNNING_DIR:{running_dir}
}}
"#,
        domain = safe_domain,
        root = effective_root,
        fastcgi = fastcgi_block,
        running_dir = running_dir.trim(),
        try_files = try_files,
    );

    // Create effective root directory (web_root + running_dir)
    emit(&format!("Creating web root: {}", root));
    let mkdir_cmd = format!("mkdir -p '{}'", effective_root);
    emit(&format!("Command: {}", mkdir_cmd));
    let (mkdir_out, mkdir_err, _) = crate::ssh::session_exec_with_output(session, &mkdir_cmd, 10)
        .await?;
    if !mkdir_out.trim().is_empty() {
        emit(&format!("STDOUT: {}", mkdir_out.trim()));
    }
    if !mkdir_err.trim().is_empty() {
        emit(&format!("STDERR: {}", mkdir_err.trim()));
    }

    // Set ownership
    emit("Setting file permissions...");
    let chown_cmd = format!("chown -R {}:'{}' '{}' 2>/dev/null || true", nginx_user, nginx_user, safe_root);
    emit(&format!("Command: {}", chown_cmd));
    let (chown_out, chown_err, _) = crate::ssh::session_exec_with_output(session, &chown_cmd, 10)
        .await?;
    if !chown_out.trim().is_empty() {
        emit(&format!("STDOUT: {}", chown_out.trim()));
    }
    if !chown_err.trim().is_empty() {
        emit(&format!("STDERR: {}", chown_err.trim()));
    }

    // Create a default welcome page
    if !php_version.is_empty() {
        let index_content = r#"<?php
$domain = $_SERVER['HTTP_HOST'] ?? 'your site';
?>
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Welcome - <?= htmlspecialchars($domain) ?></title>
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: linear-gradient(135deg, #0d1117 0%, #161b22 50%, #1a2332 100%);
            color: #c9d1d9;
            min-height: 100vh;
            display: flex;
            align-items: center;
            justify-content: center;
        }
        .container {
            text-align: center;
            padding: 20px 40px;
            max-width: 600px;
        }
        h1 {
            font-size: 2.2em;
            white-space: nowrap;
            margin-bottom: 12px;
        }
        h1.success-title {
            color: #3fb950;
        }
        .subtitle {
            font-size: 1.1em;
            color: #8b949e;
            margin-bottom: 40px;
        }
        .info {
            background: rgba(88, 166, 255, 0.08);
            border: 1px solid rgba(88, 166, 255, 0.2);
            border-radius: 12px;
            padding: 24px;
            margin-bottom: 30px;
        }
        .info-row {
            display: flex;
            justify-content: space-between;
            padding: 8px 0;
            border-bottom: 1px solid rgba(255,255,255,0.05);
        }
        .info-row:last-child { border-bottom: none; }
        .info-label { color: #8b949e; }
        .info-value { color: #58a6ff; font-weight: 600; }
        .features {
            display: flex;
            gap: 12px;
            justify-content: center;
            flex-wrap: wrap;
        }
        .features span {
            background: rgba(35, 134, 54, 0.15);
            border: 1px solid rgba(35, 134, 54, 0.3);
            color: #3fb950;
            padding: 6px 16px;
            border-radius: 20px;
            font-size: 0.9em;
        }
        .footer {
            margin-top: 40px;
            color: #484f58;
            font-size: 0.85em;
        }
    </style>
</head>
<body>
    <div class="container">
        <h1 class="success-title">Congratulations!</h1>
        <h1>Your website has been created successfully.</h1>
        <p class="subtitle">LeePanel, Your powerful SSH server management companion.</p>
        <div class="features">
            <span>&#10003; Secure Connections</span>
            <span>&#10003; File Management</span>
            <span>&#10003; Server Control</span>
        </div>
        <p class="footer">Powered by LeePanel</p>
    </div>
</body>
</html>
"#;
        crate::ssh::session_write_file(session,
                &format!("{}/index.php", root),
                index_content,
            )
            .await?;
    } else {
        let index_content = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Welcome</title>
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: linear-gradient(135deg, #0d1117 0%, #161b22 50%, #1a2332 100%);
            color: #c9d1d9;
            min-height: 100vh;
            display: flex;
            align-items: center;
            justify-content: center;
        }
        .container {
            text-align: center;
            padding: 20px 40px;
            max-width: 600px;
        }
        h1 {
            font-size: 2.2em;
            white-space: nowrap;
            margin-bottom: 12px;
        }
        h1.success-title {
            color: #3fb950;
        }
        .subtitle {
            font-size: 1.1em;
            color: #8b949e;
            margin-bottom: 40px;
        }
        .features {
            display: flex;
            gap: 12px;
            justify-content: center;
            flex-wrap: wrap;
        }
        .features span {
            background: rgba(35, 134, 54, 0.15);
            border: 1px solid rgba(35, 134, 54, 0.3);
            color: #3fb950;
            padding: 6px 16px;
            border-radius: 20px;
            font-size: 0.9em;
        }
        .footer {
            margin-top: 40px;
            color: #484f58;
            font-size: 0.85em;
        }
    </style>
</head>
<body>
    <div class="container">
        <h1 class="success-title">Congratulations!</h1>
        <h1>Your website has been created successfully.</h1>
        <p class="subtitle">LeePanel, Your powerful SSH server management companion.</p>
        <div class="features">
            <span>&#10003; Secure Connections</span>
            <span>&#10003; File Management</span>
            <span>&#10003; Server Control</span>
        </div>
        <p class="footer">Powered by LeePanel</p>
    </div>
</body>
</html>
"#;
        crate::ssh::session_write_file(session,
                &format!("{}/index.html", root),
                index_content,
            )
            .await?;
    }

    // Write nginx config
    emit("Generating Nginx configuration...");
    crate::ssh::session_write_file(session, &config_path, &nginx_conf)
        .await?;
    emit(&format!("Config written to: {}", config_path));

    // Enable site (symlink if using sites-available)
    if uses_sites {
        emit("Enabling site in Nginx...");
        let symlink_cmd = format!("ln -sf '{}' '/etc/nginx/sites-enabled/{}'", config_path, safe_domain);
        emit(&format!("Command: {}", symlink_cmd));
        let (symlink_out, symlink_err, _) = crate::ssh::session_exec_with_output(session, &symlink_cmd, 5)
            .await?;
        if !symlink_out.trim().is_empty() {
            emit(&format!("STDOUT: {}", symlink_out.trim()));
        }
        if !symlink_err.trim().is_empty() {
            emit(&format!("STDERR: {}", symlink_err.trim()));
        }
    }

    // Test nginx config
    emit("Testing Nginx configuration...");
    let test_cmd = "nginx -t 2>&1";
    emit(&format!("Command: {}", test_cmd));
    let (test_stdout, test_stderr, test_code) = crate::ssh::session_exec_with_output(session, test_cmd, 10)
        .await?;
    // nginx -t outputs everything to stderr; combine both for robust checking
    let test_combined = format!("{} {}", test_stdout, test_stderr);
    if !test_stdout.trim().is_empty() {
        emit(&format!("STDOUT: {}", test_stdout.trim()));
    }
    if !test_stderr.trim().is_empty() {
        emit(&format!("STDERR: {}", test_stderr.trim()));
    }
    let test_ok = test_code == 0 || test_combined.contains("test is successful") || test_combined.contains("syntax is ok");
    if !test_ok {
        return Err(format!("Nginx config test failed: {}", test_combined.trim()));
    }

    // Reload nginx
    emit("Reloading Nginx...");
    let reload_cmd = "systemctl reload nginx";
    emit(&format!("Command: {}", reload_cmd));
    let (reload_out, reload_err, _) = crate::ssh::session_exec_with_output(session, reload_cmd, 10)
        .await?;
    if !reload_out.trim().is_empty() {
        emit(&format!("STDOUT: {}", reload_out.trim()));
    }
    if !reload_err.trim().is_empty() {
        emit(&format!("STDERR: {}", reload_err.trim()));
    }
    emit("✓ Nginx reloaded successfully");

    // Create MySQL database and user if requested
    let mut db_warning = String::new();
    if create_db && !db_name.is_empty() {
        emit(&format!("Creating database: {}...", db_name));
        
        // Check if mysql client is installed on the server
        let (which_out, _, which_code) = crate::ssh::session_exec_with_output(session, "command -v mysql", 5)
            .await?;
        if which_code != 0 || which_out.trim().is_empty() {
            emit("✗ MySQL client is NOT installed on the server");
            emit("  Install it first: apt install mysql-client (Debian/Ubuntu) or yum install mysql (CentOS/RHEL)");
            db_warning = " (database not created: mysql client not found on server)".to_string();
        } else {
            emit(&format!("MySQL client found: {}", which_out.trim()));
        }
        
        if db_warning.is_empty() {
            let safe_db = db_name.replace('`', "");
            let safe_user = db_user.replace('`', "");
            let safe_pw = db_pass.replace('`', "");
            let sql = format!(
                "CREATE DATABASE IF NOT EXISTS `{}`;\n\
                 CREATE USER IF NOT EXISTS '{}'@'localhost' IDENTIFIED BY '{}';\n\
                 GRANT ALL PRIVILEGES ON `{}`.* TO '{}'@'localhost';\n\
                 FLUSH PRIVILEGES;\n",
                safe_db, safe_user, safe_pw, safe_db, safe_user
            );
            
            // Write SQL file via SFTP (more reliable than echo with complex escaping)
            let tmp_sql = "/tmp/db_setup.sql";
            emit(&format!("Writing SQL file to {}...", tmp_sql));
            emit("SQL Content:");
            for line in sql.lines() {
                emit(line);
            }
            
            if let Err(e) = crate::ssh::session_write_file(session, tmp_sql, &sql).await {
                emit(&format!("✗ Failed to write SQL file: {}", e));
                db_warning = format!(" (database not created: failed to write SQL file: {})", e);
            } else {
                emit("✓ SQL file written successfully");
                
                // Execute mysql command separately
                let mysql_cmd = format!("mysql < {}", tmp_sql);
                emit(&format!("Command: {}", mysql_cmd));
                
                let (db_out, db_err, db_code) = crate::ssh::session_exec_with_output(session, &mysql_cmd, 30)
                    .await?;
                
                // Check if database was actually created (verify regardless of exit code)
                let verify_cmd = format!("mysql -e 'SHOW DATABASES' 2>&1 | grep -i '{}'", safe_db);
                let (verify_out, _, _) = crate::ssh::session_exec_with_output(session, &verify_cmd, 10)
                    .await?;
                let db_exists = !verify_out.trim().is_empty();
                
                if db_code != 0 && !db_exists {
                    // Real failure - database was not created
                    let full_output = format!("{} {}", db_out, db_err).trim().to_string();
                    let error_detail = if full_output.is_empty() {
                        "unknown error".to_string()
                    } else {
                        full_output.clone()
                    };
                    db_warning = format!(" (but database creation failed: {})", error_detail.lines().next().unwrap_or("unknown error"));
                    
                    emit("✗ Database creation failed!");
                    emit(&format!("Exit code: {}", db_code));
                    
                    if !db_out.trim().is_empty() {
                        emit("=== STDOUT ===");
                        for line in db_out.lines() {
                            emit(line);
                        }
                    } else {
                        emit("STDOUT: (empty)");
                    }
                    
                    if !db_err.trim().is_empty() {
                        emit("=== STDERR ===");
                        for line in db_err.lines() {
                            emit(line);
                        }
                    } else {
                        emit("STDERR: (empty)");
                    }
                    
                    if db_out.trim().is_empty() && db_err.trim().is_empty() {
                        emit("");
                        emit(" Troubleshooting hints:");
                        emit("- Check if MySQL/MariaDB service is running: systemctl status mysql");
                        emit("- Try connecting manually: mysql -u root -p");
                        emit("- Check MySQL error log: /var/log/mysql/error.log or journalctl -u mysql");
                        emit("- Verify MySQL socket exists: ls -la /var/run/mysqld/mysqld.sock");
                    }
                } else if db_code != 0 && db_exists {
                    // Exit code was non-zero but database exists - likely SSH channel issue
                    emit("✓ Database created successfully (verified)");
                    emit(&format!("Note: Exit code was {} but database exists", db_code));
                    if !db_out.trim().is_empty() {
                        emit("=== Output ===");
                        for line in db_out.lines() {
                            emit(line);
                        }
                    }
                } else {
                    emit("✓ Database created successfully");
                    if !db_out.trim().is_empty() {
                        emit("=== Output ===");
                        for line in db_out.lines() {
                            emit(line);
                        }
                    }
                }
            }
        }
    }

    // Setup SSL with certbot if requested
    if use_ssl {
        emit("Setting up SSL certificate...");
        let ssl_cmd = format!("certbot --nginx -d '{}' --non-interactive --agree-tos --email admin@'{}' 2>&1", safe_domain, safe_domain);
        emit(&format!("Command: {}", ssl_cmd));
        let (ssl_out, ssl_err, ssl_code) = crate::ssh::session_exec_with_output(session, &ssl_cmd, 120)
            .await?;
        
        if !ssl_out.trim().is_empty() {
            emit("=== STDOUT ===");
            for line in ssl_out.lines() {
                emit(line);
            }
        }
        if !ssl_err.trim().is_empty() {
            emit("=== STDERR ===");
            for line in ssl_err.lines() {
                emit(line);
            }
        }
        
        if ssl_code != 0 {
            emit("✗ SSL setup failed!");
            return Ok((
                config_path,
                format!(
                    "Site created successfully but SSL setup failed. Check the logs above for details.\nYou can run certbot manually later."
                ),
            ));
        }
        emit("✓ SSL certificate installed");
    }

    emit(&format!("Site {} created successfully!", domain));
    Ok((config_path, format!("Site {} created successfully at {}{}", domain, root, db_warning)))
}

/// Toggle site enable/disable
pub async fn toggle_site(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    config_path: &str,
    domain: &str,
    enable: bool,
) -> Result<String, String> {
    let safe_domain = domain.replace('\'', "'\\''");
    let safe_path = config_path.replace('\'', "'\\''");

    // Determine which strategy to use based on where the config lives
    if config_path.contains("sites-available") || config_path.contains("sites-enabled") {
        // sites-enabled/sites-available style: manage symlink in sites-enabled,
        // actual config lives in sites-available (or sites-enabled if no symlink was used)
        let link = format!("/etc/nginx/sites-enabled/{}", safe_domain);
        let available_path = if config_path.contains("sites-available") {
            // Strip .disabled suffix if present to get the canonical path
            config_path.trim_end_matches(".disabled").to_string()
        } else {
            // Config is directly in sites-enabled; move to sites-available on disable
            format!("/etc/nginx/sites-available/{}", safe_domain)
        };
        let safe_available = available_path.replace('\'', "'\\''");

        if enable {
            // Ensure config is in sites-available
            if config_path.contains("sites-enabled") && !config_path.contains("sites-available") {
                let src = config_path.trim_end_matches(".disabled");
                let safe_src = src.replace('\'', "'\\''");
                crate::ssh::session_exec_with_output(session, &format!("mv '{}' '{}'", safe_src, safe_available), 5)
                    .await?;
            }
            // Create symlink
            crate::ssh::session_exec_with_output(session, &format!("ln -sf '{}' '{}'", safe_available, link), 5)
                .await?;
        } else {
            // Remove symlink from sites-enabled
            crate::ssh::session_exec_with_output(session, &format!("rm -f '{}'", link), 5)
                .await?;
            // If config is in sites-enabled, move to sites-available
            if config_path.contains("sites-enabled") && !config_path.contains("sites-available") {
                crate::ssh::session_exec_with_output(session, "mkdir -p /etc/nginx/sites-available", 5)
                    .await?;
                crate::ssh::session_exec_with_output(session, &format!("mv '{}' '{}'", safe_path, safe_available), 5)
                    .await?;
            }
        }
    } else {
        // conf.d style: rename between .conf and .conf.disabled
        let enabled_path = config_path.trim_end_matches(".disabled").to_string();
        if enable {
            crate::ssh::session_exec_with_output(session, &format!("mv '{}' '{}'", safe_path, enabled_path.replace('\'', "'\\''")), 5)
                .await?;
        } else {
            crate::ssh::session_exec_with_output(session, &format!("mv '{}' '{}.disabled'", safe_path, safe_path), 5)
                .await?;
        }
    }

    // Test and reload nginx
    let (test_out, test_err, test_code) = crate::ssh::session_exec_with_output(session, "nginx -t 2>&1", 10)
        .await?;
    let test_combined = format!("{} {}", test_out, test_err);
    if test_code != 0 && !test_combined.contains("test is successful") && !test_combined.contains("syntax is ok") {
        // Revert
        let link = format!("/etc/nginx/sites-enabled/{}", safe_domain);
        if config_path.contains("sites-available") || config_path.contains("sites-enabled") {
            let available_path = if config_path.contains("sites-available") {
                config_path.trim_end_matches(".disabled").to_string()
            } else {
                format!("/etc/nginx/sites-available/{}", safe_domain)
            };
            let safe_available = available_path.replace('\'', "'\\''");
            if enable {
                let _ = crate::ssh::session_exec_with_output(session, &format!("rm -f '{}'", link), 5).await;
                if config_path.contains("sites-enabled") && !config_path.contains("sites-available") {
                    let src = config_path.trim_end_matches(".disabled");
                    let safe_src = src.replace('\'', "'\\''");
                    let _ = crate::ssh::session_exec_with_output(session, &format!("mv '{}' '{}'", safe_available, safe_src), 5).await;
                }
            } else {
                if config_path.contains("sites-enabled") && !config_path.contains("sites-available") {
                    let _ = crate::ssh::session_exec_with_output(session, &format!("mv '{}' '{}'", safe_available, safe_path), 5).await;
                }
                let _ = crate::ssh::session_exec_with_output(session, &format!("ln -sf '{}' '{}'", safe_available, link), 5).await;
            }
        }
        return Err(format!("Nginx test failed after toggling site, reverted: {}", test_combined.trim()));
    }

    crate::ssh::session_exec_with_output(session, "systemctl reload nginx", 10)
        .await?;

    let action = if enable { "Started" } else { "Stopped" };
    Ok(format!("{} site {}", action, domain))
}

/// Graceful restart (reload) a site — nginx -t then systemctl reload nginx
#[allow(dead_code)]
pub async fn restart_site(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    domain: &str,
) -> Result<String, String> {
    // Verify config first
    let (test_out, test_err, test_code) = crate::ssh::session_exec_with_output(session, "nginx -t 2>&1", 10)
        .await?;
    let test_combined = format!("{} {}", test_out, test_err);
    if test_code != 0 && !test_combined.contains("test is successful") && !test_combined.contains("syntax is ok") {
        return Err(format!("Nginx config test failed, reload aborted: {}", test_combined.trim()));
    }

    crate::ssh::session_exec_with_output(session, "systemctl reload nginx", 10)
        .await?;

    Ok(format!("Restarted site {}", domain))
}

/// Delete a site
pub async fn delete_site(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    domain: &str,
    remove_files: bool,
) -> Result<String, String> {
    let safe_domain = domain.replace('\'', "'\\''");

    // Remove symlinks and config files
    crate::ssh::session_exec_with_output(session,
            &format!(
                "rm -f '/etc/nginx/sites-enabled/{}' '/etc/nginx/conf.d/{}.conf' 2>/dev/null; rm -f '/etc/nginx/sites-available/{}' 2>/dev/null",
                safe_domain, safe_domain, safe_domain
            ),
            5,
        )
        .await?;

    if remove_files {
        // Find and remove the web root (check both common paths)
        let (stdout, _, _) = crate::ssh::session_exec_with_output(session,
                &format!(
                    "for d in /www/wwwroot/{d} /var/www/{d}; do [ -d \"$d\" ] && echo \"$d\" && break; done",
                    d = safe_domain
                ),
                5,
            )
            .await?;
        let web_root = stdout.trim();
        if !web_root.is_empty() {
            crate::ssh::session_exec_with_output(session,
                    &format!("rm -rf '{}'", web_root.replace('\'', "'\\''")),
                    10,
                )
                .await?;
        }
    }

    // Reload nginx
    let (reload_stdout, reload_stderr, reload_code) = crate::ssh::session_exec_with_output(session, "nginx -t 2>&1 && systemctl reload nginx 2>&1", 10)
        .await?;
    let reload_combined = format!("{} {}", reload_stdout, reload_stderr);
    let reload_ok = reload_code == 0
        || reload_combined.contains("test is successful")
        || reload_combined.contains("syntax is ok");
    if !reload_ok {
        return Err(format!("Nginx reload failed after site deletion: {}", reload_combined.trim()));
    }

    Ok(format!("Site {} deleted successfully", domain))
}

/// Update site with all settings in one call (batch update)
pub async fn update_site_full(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
    old_domain: &str,
    new_domains: &str,           // space-separated
    new_root: &str,
    new_php_version: &str,
    index_files: &str,           // space-separated
    rewrite_rules: &str,
    config_path: &str,
    running_dir: &str,
    open_basedir: bool,
    hotlink_enabled: bool,
    hotlink_extensions: &str,    // comma-separated
    hotlink_allowed_domains: &str, // newline-separated
    hotlink_response: &str,
    hotlink_allow_empty_referer: bool,
    proxy_enabled: bool,
    proxy_path: &str,
    proxy_target: &str,
    proxy_websocket: bool,
    proxy_preserve_host: bool,
) -> Result<String, String> {
    let primary_domain = new_domains.split_whitespace().next().unwrap_or(old_domain);
    
    // Step 1: Read existing config to check for SSL and preserve it
    let (old_conf, _, _) = crate::ssh::session_exec_with_output(session, &format!("cat '{}' 2>/dev/null", config_path.replace('\'', "'\\''")), 5)
        .await?;
    let has_ssl = old_conf.contains("ssl_certificate") || old_conf.contains("listen 443");
    
    // Step 2: Build complete nginx config in memory
    let safe_domains: Vec<String> = new_domains.split_whitespace().map(|d| d.replace('\'', "'\\''")).collect();
    let server_name = safe_domains.join(" ");
    let safe_root = new_root.replace('\'', "'\\''");
    
    let php_sock = if new_php_version.is_empty() {
        String::new()
    } else {
        format!("/run/php/php{}-fpm.sock", new_php_version)
    };
    let has_php = !php_sock.is_empty();
    
    let index_directive = if index_files.trim().is_empty() {
        "index.php index.html index.htm".to_string()
    } else {
        index_files.trim().to_string()
    };
    
    // Compute effective nginx root: web_root + running_dir
    let running_dir_clean = running_dir.trim().trim_start_matches('/');
    let effective_root = if running_dir_clean.is_empty() {
        safe_root.clone()
    } else {
        format!("{}/{}", safe_root, running_dir_clean)
    };
    
    let open_basedir_line = if open_basedir && has_php {
        format!("\n        fastcgi_param PHP_ADMIN_VALUE \"open_basedir={}:/tmp/\";", safe_root)
    } else {
        String::new()
    };
    
    // Build PHP location block
    let php_location = if has_php {
        format!(r#"
    location ~ \.php$ {{
        include snippets/fastcgi-php.conf;
        fastcgi_pass unix:{php_sock};
        fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;
        include fastcgi_params;{oba}
    }}
"#, php_sock = php_sock, oba = open_basedir_line)
    } else {
        String::new()
    };
    
    let try_files = if has_php {
        "try_files $uri $uri/ /index.php?$query_string;"
    } else {
        "try_files $uri $uri/ =404;"
    };
    
    // Build base config
    let mut nginx_conf = format!(
        r#"server {{
    listen 80;
    listen [::]:80;
    server_name {server_name};
    root {root};
    index {index_directive};

{location_root}{rewrite_section}{php_location}
    location ~ /\.ht {{
        deny all;
    }}

    access_log /var/log/nginx/{domain}.access.log;
    error_log /var/log/nginx/{domain}.error.log;
# __RUNNING_DIR:{running_dir}
"#,
        server_name = server_name,
        root = effective_root,
        index_directive = index_directive,
        domain = safe_domains.first().map(|s| s.as_str()).unwrap_or(primary_domain),
        running_dir = running_dir.trim(),
        location_root = if proxy_enabled && proxy_path.trim() == "/" {
            // When reverse proxy covers root path, skip default location / block to avoid duplicate
            String::new()
        } else {
            format!(
                r#"    location / {{
        {}
    }}

"#,
                try_files
            )
        },
        php_location = php_location,
        rewrite_section = if rewrite_rules.trim().is_empty() {
            String::new()
        } else {
            let trimmed = rewrite_rules.trim();
            // Check if user input contains a complete location block (e.g., "location / { ... }")
            // If so, extract the inner content to avoid duplicate location blocks
            if trimmed.starts_with("location ") && trimmed.contains('{') && trimmed.contains('}') {
                // Extract content between first '{' and last '}'
                if let Some(start) = trimmed.find('{') {
                    if let Some(end) = trimmed.rfind('}') {
                        let inner = trimmed[start+1..end].trim();
                        // Format as indented instructions without location wrapper
                        format!("    # Rewrite rules\n    {}\n\n", inner.replace('\n', "\n    "))
                    } else {
                        format!("    # Rewrite rules\n    {}\n\n", trimmed.replace('\n', "\n    "))
                    }
                } else {
                    format!("    # Rewrite rules\n    {}\n\n", trimmed.replace('\n', "\n    "))
                }
            } else {
                // User provided just instructions, insert as-is with indentation
                format!("    # Rewrite rules\n    {}\n\n", trimmed.replace('\n', "\n    "))
            }
        },
    );
    
    // Preserve SSL block if it existed
    if has_ssl {
        let ssl_lines: Vec<&str> = old_conf.lines()
            .filter(|l| l.contains("ssl_") || l.contains("listen 443") || l.contains("listen [::]:443"))
            .collect();
        if !ssl_lines.is_empty() {
            nginx_conf.push_str("\n    # SSL Configuration (preserved)\n");
            for line in ssl_lines {
                nginx_conf.push_str("    ");
                nginx_conf.push_str(line.trim());
                nginx_conf.push('\n');
            }
        }
    }
    
    // Add Hotlink Protection block
    if hotlink_enabled {
        let ext_list: Vec<&str> = hotlink_extensions.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
        let ext_regex = if ext_list.is_empty() {
            "(jpg|jpeg|gif|png|js|css)".to_string()
        } else {
            format!("({})", ext_list.join("|"))
        };
        
        let mut referers = Vec::new();
        if hotlink_allow_empty_referer {
            referers.push("none".to_string());
        }
        referers.push("blocked".to_string());
        referers.push("server_names".to_string());
        for d in hotlink_allowed_domains.lines() {
            let d = d.trim();
            if !d.is_empty() {
                if d.starts_with("*.") {
                    referers.push(d.to_string());
                } else {
                    referers.push(format!("*.{}", d));
                }
            }
        }
        let valid_referers = referers.join(" ");
        
        let return_directive = if hotlink_response.trim().parse::<u16>().is_ok() {
            format!("return {}", hotlink_response.trim())
        } else {
            format!("rewrite ^ {} last", hotlink_response.trim())
        };
        
        nginx_conf.push_str(&format!(r#"
    # Hotlink Protection Start
    location ~* \.{}$ {{
        valid_referers {};
        if ($invalid_referer) {{
            {};
        }}
    }}
    # Hotlink Protection End
"#, ext_regex, valid_referers, return_directive));
    }
    
    // Add Reverse Proxy block
    if proxy_enabled {
        let proxy_path_clean = if proxy_path.starts_with('/') {
            proxy_path.to_string()
        } else {
            format!("/{}", proxy_path)
        };
        let proxy_target_clean = proxy_target.trim().split_whitespace().next().unwrap_or(proxy_target);
        
        let mut headers = vec![
            "proxy_set_header Host $host;".to_string(),
            "proxy_set_header X-Real-IP $remote_addr;".to_string(),
            "proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;".to_string(),
            "proxy_set_header X-Forwarded-Proto $scheme;".to_string(),
        ];
        
        if !proxy_preserve_host {
            headers[0] = format!("proxy_set_header Host {};", proxy_target_clean.replace("http://", "").replace("https://", "").trim_end_matches('/'));
        }
        
        if proxy_websocket {
            headers.push("proxy_http_version 1.1;".to_string());
            headers.push("proxy_set_header Upgrade $http_upgrade;".to_string());
            headers.push("proxy_set_header Connection upgrade;".to_string());
        }
        
        nginx_conf.push_str(&format!(r#"
    # Reverse Proxy Start
    location {} {{
        proxy_pass {};
{}
    }}
    # Reverse Proxy End
"#, proxy_path_clean, proxy_target_clean, headers.iter().map(|h| format!("        {}", h)).collect::<Vec<_>>().join("\n")));
    }
    
    nginx_conf.push_str("}\n");
    
    // Step 3: Determine new config path if primary domain changed
    let domain_changed = old_domain != primary_domain;
    let new_config_path = if domain_changed {
        if config_path.contains("sites-available") {
            format!("/etc/nginx/sites-available/{}", primary_domain)
        } else if config_path.contains("conf.d") {
            format!("/etc/nginx/conf.d/{}.conf", primary_domain)
        } else {
            config_path.to_string()
        }
    } else {
        config_path.to_string()
    };
    
    // Step 4: Create effective root directory
    crate::ssh::session_exec_with_output(session,
            &format!("mkdir -p '{}' && chown -R www-data:www-data '{}' 2>/dev/null || true", effective_root, effective_root),
            10,
        )
        .await?;
    
    // Step 5: Write complete config in one operation
    crate::ssh::session_write_file(session, &new_config_path, &nginx_conf)
        .await?;
    
    // Step 6: Handle domain change: symlink + cleanup
    if domain_changed {
        let safe_old_domain = old_domain.replace('\'', "'\\''");
        if new_config_path.contains("sites-available") {
            crate::ssh::session_exec_with_output(session,
                    &format!(
                        "rm -f '/etc/nginx/sites-enabled/{}'; ln -sf '{}' '/etc/nginx/sites-enabled/{}'",
                        safe_old_domain, new_config_path, safe_domains.first().map(|s| s.as_str()).unwrap_or(primary_domain)
                    ),
                    5,
                )
                .await?;
        }
        if new_config_path != config_path {
            crate::ssh::session_exec_with_output(session,
                    &format!("rm -f '{}'", config_path.replace('\'', "'\\''")),
                    5,
                )
                .await?;
        }
    }
    
    // Step 7: Test + reload nginx
    test_and_reload_nginx(session, cache, session_id).await?;
    
    Ok(format!("Site {} updated successfully", primary_domain))
}

/// Update an existing site's configuration
pub async fn update_site(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
    old_domain: &str,
    new_domains: &str,
    new_root: &str,
    new_php_version: &str,
    index_files: &str,
    rewrite_rules: &str,
    config_path: &str,
    running_dir: &str,
    open_basedir: bool,
) -> Result<String, String> {
    // new_domains is space-separated list; first is primary
    let primary_domain = new_domains.split_whitespace().next().unwrap_or(old_domain);
    let safe_domains: Vec<String> = new_domains.split_whitespace().map(|d| d.replace('\'', "'\\''")).collect();
    let server_name = safe_domains.join(" ");
    let safe_root = new_root.replace('\'', "'\\''");

    let php_sock = if new_php_version.is_empty() {
        String::new()
    } else {
        format!("/run/php/php{}-fpm.sock", new_php_version)
    };
    let has_php = !php_sock.is_empty();

    // Read old config to check for SSL
    let (old_conf, _, _) = crate::ssh::session_exec_with_output(session, &format!("cat '{}' 2>/dev/null", config_path.replace('\'', "'\\''")), 5)
        .await?;
    let has_ssl = old_conf.contains("ssl_certificate") || old_conf.contains("listen 443");

    let index_directive = if index_files.trim().is_empty() {
        "index.php index.html index.htm".to_string()
    } else {
        index_files.trim().to_string()
    };

    // Compute effective nginx root: web_root + running_dir
    let running_dir_clean = running_dir.trim().trim_start_matches('/');
    let effective_root = if running_dir_clean.is_empty() {
        safe_root.clone()
    } else {
        format!("{}/{}", safe_root, running_dir_clean)
    };

    let open_basedir_line = if open_basedir && has_php {
        format!("\n        fastcgi_param PHP_ADMIN_VALUE \"open_basedir={}:/tmp/\";", safe_root)
    } else {
        String::new()
    };

    // Build PHP location block (only if PHP version selected)
    let php_location = if has_php {
        format!(r#"
    location ~ \.php$ {{
        include snippets/fastcgi-php.conf;
        fastcgi_pass unix:{php_sock};
        fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;
        include fastcgi_params;{oba}
    }}
"#, php_sock = php_sock, oba = open_basedir_line)
    } else {
        String::new()
    };

    let try_files = if has_php {
        "try_files $uri $uri/ /index.php?$query_string;"
    } else {
        "try_files $uri $uri/ =404;"
    };

    let mut nginx_conf = format!(
        r#"server {{
    listen 80;
    listen [::]:80;
    server_name {server_name};
    root {root};
    index {index_directive};

    location / {{
        {try_files}
    }}

{rewrite_section}{php_location}
    location ~ /\.ht {{
        deny all;
    }}

    access_log /var/log/nginx/{domain}.access.log;
    error_log /var/log/nginx/{domain}.error.log;
# __RUNNING_DIR:{running_dir}
"#,
        server_name = server_name,
        root = effective_root,
        index_directive = index_directive,
        domain = safe_domains.first().map(|s| s.as_str()).unwrap_or(primary_domain),
        running_dir = running_dir.trim(),
        try_files = try_files,
        php_location = php_location,
        rewrite_section = if rewrite_rules.trim().is_empty() {
            String::new()
        } else {
            let trimmed = rewrite_rules.trim();
            // Check if user input contains a complete location block (e.g., "location / { ... }")
            // If so, extract the inner content to avoid duplicate location blocks
            if trimmed.starts_with("location ") && trimmed.contains('{') && trimmed.contains('}') {
                // Extract content between first '{' and last '}'
                if let Some(start) = trimmed.find('{') {
                    if let Some(end) = trimmed.rfind('}') {
                        let inner = trimmed[start+1..end].trim();
                        // Format as indented instructions without location wrapper
                        format!("    # Rewrite rules\n    {}\n\n", inner.replace('\n', "\n    "))
                    } else {
                        format!("    # Rewrite rules\n    {}\n\n", trimmed.replace('\n', "\n    "))
                    }
                } else {
                    format!("    # Rewrite rules\n    {}\n\n", trimmed.replace('\n', "\n    "))
                }
            } else {
                // User provided just instructions, insert as-is with indentation
                format!("    # Rewrite rules\n    {}\n\n", trimmed.replace('\n', "\n    "))
            }
        },
    );

    // Preserve SSL block if it existed
    if has_ssl {
        let ssl_lines: Vec<&str> = old_conf.lines()
            .filter(|l| l.contains("ssl_") || l.contains("listen 443") || l.contains("listen [::]:443"))
            .collect();
        if !ssl_lines.is_empty() {
            nginx_conf.push_str("\n    # SSL Configuration (preserved)\n");
            for line in ssl_lines {
                nginx_conf.push_str("    ");
                nginx_conf.push_str(line.trim());
                nginx_conf.push('\n');
            }
        }
    }
    nginx_conf.push_str("}\n");

    // Determine new config path if primary domain changed
    let domain_changed = old_domain != primary_domain;
    let new_config_path = if domain_changed {
        if config_path.contains("sites-available") {
            format!("/etc/nginx/sites-available/{}", primary_domain)
        } else if config_path.contains("conf.d") {
            format!("/etc/nginx/conf.d/{}.conf", primary_domain)
        } else {
            config_path.to_string()
        }
    } else {
        config_path.to_string()
    };

    // Create effective root directory (web_root + running_dir)
    crate::ssh::session_exec_with_output(session,
            &format!("mkdir -p '{}' && chown -R www-data:www-data '{}' 2>/dev/null || true", effective_root, effective_root),
            10,
        )
        .await?;

    // Write config
    crate::ssh::session_write_file(session, &new_config_path, &nginx_conf)
        .await?;

    // Handle domain change: symlink + cleanup
    if domain_changed {
        let safe_old_domain = old_domain.replace('\'', "'\\''");
        if new_config_path.contains("sites-available") {
            crate::ssh::session_exec_with_output(session,
                    &format!(
                        "rm -f '/etc/nginx/sites-enabled/{}'; ln -sf '{}' '/etc/nginx/sites-enabled/{}'",
                        safe_old_domain, new_config_path, safe_domains.first().map(|s| s.as_str()).unwrap_or(primary_domain)
                    ),
                    5,
                )
                .await?;
        }
        if new_config_path != config_path {
            crate::ssh::session_exec_with_output(session,
                    &format!("rm -f '{}'", config_path.replace('\'', "'\\''")),
                    5,
                )
                .await?;
        }
    }

    // Test + reload nginx
    test_and_reload_nginx(session, cache, session_id).await?;

    Ok(format!("Site {} updated successfully", primary_domain))
}

/// Save raw nginx config for a site, test and reload
pub async fn save_site_config(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
    config_path: &str,
    config_content: &str,
) -> Result<String, String> {
    crate::ssh::session_write_file(session, config_path, config_content)
        .await?;
    test_and_reload_nginx(session, cache, session_id).await?;
    Ok("Config saved and nginx reloaded".to_string())
}

/// Set hotlink protection for a site
pub async fn set_hotlink_protection(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
    config_path: &str,
    enabled: bool,
    extensions: &str,
    allowed_domains: &str,
    response_code: &str,
    allow_empty_referer: bool,
) -> Result<String, String> {
    // Read current config
    let (config, _, _) = crate::ssh::session_exec_with_output(session, &format!("cat '{}'", config_path.replace('\'', "'\\''")), 5)
        .await?;

    // Remove existing hotlink block (between markers)
    let mut lines: Vec<String> = config.lines().map(|l| l.to_string()).collect();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].contains("# Hotlink Protection Start") {
            let start = i;
            while i < lines.len() && !lines[i].contains("# Hotlink Protection End") {
                i += 1;
            }
            if i < lines.len() { i += 1; } // skip the End marker
            lines.drain(start..i);
            break;
        }
        i += 1;
    }

    if enabled {
        // Build extensions regex: jpg,jpeg,png -> (jpg|jpeg|png)
        let ext_list: Vec<&str> = extensions.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
        let ext_regex = if ext_list.is_empty() {
            "(jpg|jpeg|gif|png|js|css)".to_string()
        } else {
            format!("({})", ext_list.join("|"))
        };

        // Build valid_referers
        let mut referers = Vec::new();
        if allow_empty_referer {
            referers.push("none".to_string());
        }
        referers.push("blocked".to_string());
        referers.push("server_names".to_string());
        for d in allowed_domains.lines() {
            let d = d.trim();
            if !d.is_empty() {
                // Only add wildcard version (*.domain) which covers both subdomains and main domain
                // Avoid adding both *.domain and domain to prevent nginx "conflicting parameter" error
                if d.starts_with("*.") {
                    referers.push(d.to_string());
                } else {
                    referers.push(format!("*.{}", d));
                }
            }
        }
        let valid_referers = referers.join(" ");

        // Response: 403, 404, or a path
        let return_directive = if response_code.trim().parse::<u16>().is_ok() {
            format!("return {}", response_code.trim())
        } else {
            format!("return 403 \"{}\" ", response_code.trim())
        };

        let hotlink_block = format!(
r#"    # Hotlink Protection Start
    location ~* \.{ext}$ {{
        valid_referers {referers};
        if ($invalid_referer) {{
            {ret};
        }}
    }}
    # Hotlink Protection End"#,
            ext = ext_regex,
            referers = valid_referers,
            ret = return_directive,
        );

        // Insert before the last closing brace
        // Find the last `}` in the config
        let mut insert_idx = lines.len();
        for j in (0..lines.len()).rev() {
            if lines[j].trim() == "}" {
                insert_idx = j;
                break;
            }
        }
        lines.insert(insert_idx, hotlink_block);
    }

    let new_config = lines.join("\n");
    crate::ssh::session_write_file(session, config_path, &new_config).await?;
    test_and_reload_nginx(session, cache, session_id).await?;

    Ok(if enabled { "Hotlink protection enabled".to_string() } else { "Hotlink protection disabled".to_string() })
}

/// Set or remove reverse proxy configuration for a site
pub async fn set_reverse_proxy(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
    config_path: &str,
    enabled: bool,
    proxy_path: &str,
    proxy_target: &str,
    websocket: bool,
    preserve_host: bool,
) -> Result<String, String> {
    // Clean up corrupted proxy blocks in other sites BEFORE we do anything
    cleanup_all_proxy_blocks(session, cache, session_id, config_path).await;

    // Read current config
    let (config, _, _) = crate::ssh::session_exec_with_output(session, &format!("cat '{}'", config_path.replace('\'', "'\\''" )), 5)
        .await?;

    // Remove existing reverse proxy block (between markers)
    let mut lines: Vec<String> = config.lines().map(|l| l.to_string()).collect();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].contains("# Reverse Proxy Start") {
            let start = i;
            while i < lines.len() && !lines[i].contains("# Reverse Proxy End") {
                i += 1;
            }
            if i < lines.len() { i += 1; } // skip the End marker
            lines.drain(start..i);
            break;
        }
        i += 1;
    }

    if enabled {
        // Validate proxy_target: remove whitespace and ensure it's a clean URL
        let proxy_target_clean = proxy_target.trim().split_whitespace().next().unwrap_or(proxy_target);

        let proxy_path_clean = if proxy_path.starts_with('/') {
            proxy_path.to_string()
        } else {
            format!("/{}", proxy_path)
        };

        let mut headers = vec![
            "proxy_set_header Host $host;".to_string(),
            "proxy_set_header X-Real-IP $remote_addr;".to_string(),
            "proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;".to_string(),
            "proxy_set_header X-Forwarded-Proto $scheme;".to_string(),
        ];

        if !preserve_host {
            headers[0] = format!("proxy_set_header Host {};", proxy_target_clean.replace("http://", "").replace("https://", "").trim_end_matches('/'));
        }

        if websocket {
            headers.push("proxy_http_version 1.1;".to_string());
            headers.push("proxy_set_header Upgrade $http_upgrade;".to_string());
            headers.push("proxy_set_header Connection upgrade;".to_string());
        }

        let proxy_block = format!(
"    # Reverse Proxy Start
    location {} {{
        proxy_pass {};
{}
        proxy_connect_timeout 60s;
        proxy_read_timeout 120s;
        proxy_send_timeout 60s;
    }}
    # Reverse Proxy End",
            proxy_path_clean,
            proxy_target_clean,
            headers.iter().map(|h| format!("        {}", h)).collect::<Vec<_>>().join("\n"),
        );

        // Remove existing location block that conflicts with the proxy path
        // e.g., remove `location / { try_files ... }` when adding proxy for /
        let loc_pattern = format!("location {}", proxy_path_clean);
        let mut j = 0;
        while j < lines.len() {
            let trimmed = lines[j].trim();
            if trimmed.starts_with(&loc_pattern) && (trimmed.ends_with('{') || trimmed == &loc_pattern) {
                let block_start = j;
                // Find matching closing brace (track nesting)
                let mut depth: isize = 0;
                while j < lines.len() {
                    if lines[j].contains('{') { depth += lines[j].matches('{').count() as isize; }
                    if lines[j].contains('}') { depth -= lines[j].matches('}').count() as isize; }
                    j += 1;
                    if depth <= 0 { break; }
                }
                lines.drain(block_start..j);
                break;
            }
            j += 1;
        }

        // Insert before the last closing brace
        let mut insert_idx = lines.len();
        for j in (0..lines.len()).rev() {
            if lines[j].trim() == "}" {
                insert_idx = j;
                break;
            }
        }
        lines.insert(insert_idx, proxy_block);
    }

    let new_config = lines.join("\n");
    crate::ssh::session_write_file(session, config_path, &new_config).await?;
    test_and_reload_nginx(session, cache, session_id).await?;

    Ok(if enabled { "Reverse proxy enabled".to_string() } else { "Reverse proxy disabled".to_string() })
}

/// Remove ALL reverse proxy location blocks from all site configs in /etc/nginx/sites-enabled/
/// This handles both marked blocks (with # Reverse Proxy Start/End) and unmarked/orphaned proxy locations
async fn cleanup_all_proxy_blocks(session: &SshSession, _cache: &SshCache, _session_id: &str, skip_path: &str) {
    let (files_out, _, _) = match crate::ssh::session_exec_with_output(session, "ls -1 /etc/nginx/sites-enabled/ 2>/dev/null", 5)
        .await
    {
        Ok(r) => r,
        Err(_) => return,
    };

    for fname in files_out.split_whitespace() {
        let fpath = format!("/etc/nginx/sites-enabled/{}", fname);
        // Skip the config we're currently modifying (match by full path or filename)
        let skip_fname = skip_path.rsplit('/').next().unwrap_or("");
        if fpath == skip_path || fname == skip_fname
            || fpath.replace("sites-enabled", "sites-available") == skip_path
            || fpath.replace("sites-available", "sites-enabled") == skip_path
        { continue; }

        let (content, _, _) = match crate::ssh::session_exec_with_output(session, &format!("cat '{}'", fpath), 5)
            .await
        {
            Ok(r) => r,
            Err(_) => continue,
        };

        if !content.contains("proxy_pass") {
            continue;
        }

        let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
        let mut changed = false;
        let mut i = 0;
        while i < lines.len() {
            let trimmed = lines[i].trim();
            // Remove marked proxy blocks
            if trimmed.contains("# Reverse Proxy Start") {
                let start = i;
                while i < lines.len() && !lines[i].contains("# Reverse Proxy End") {
                    i += 1;
                }
                if i < lines.len() { i += 1; }
                lines.drain(start..i);
                changed = true;
                continue;
            }
            // Remove unmarked location blocks that contain proxy_pass
            if trimmed.starts_with("location") && trimmed.ends_with('{') {
                let block_start = i;
                let mut depth: isize = 0;
                let mut has_proxy_pass = false;
                let mut j = i;
                while j < lines.len() {
                    if lines[j].contains('{') { depth += lines[j].matches('{').count() as isize; }
                    if lines[j].contains('}') { depth -= lines[j].matches('}').count() as isize; }
                    if lines[j].contains("proxy_pass") { has_proxy_pass = true; }
                    j += 1;
                    if depth <= 0 { break; }
                }
                if has_proxy_pass {
                    lines.drain(block_start..j);
                    changed = true;
                    continue;
                }
            }
            i += 1;
        }

        if changed {
            let cleaned = lines.join("\n");
            let _ = crate::ssh::session_write_file(session, &fpath, &cleaned).await;
        }
    }
}

/// Helper: test nginx config and reload
async fn test_and_reload_nginx(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
) -> Result<(), String> {
    // Test config first
    let (test_stdout, test_stderr, test_code) = crate::ssh::session_exec_with_output(session, "nginx -t 2>&1", 10)
        .await?;
    let test_combined = format!("{} {}", test_stdout, test_stderr).trim().to_string();
    if test_code != 0 && !test_combined.contains("test is successful") && !test_combined.contains("syntax is ok") {
        return Err(format!("Nginx config test failed: {}", test_combined));
    }

    // Try systemctl reload first
    let (sys_stdout, sys_stderr, sys_code) = crate::ssh::session_exec_with_output(session, "systemctl reload nginx 2>&1", 10)
        .await?;
    let sys_combined = format!("{} {}", sys_stdout, sys_stderr).trim().to_string();
    if sys_code == 0 || sys_combined.is_empty() {
        return Ok(()); // Success or silent success
    }

    // Fallback: try nginx -s reload
    let (ns_stdout, ns_stderr, ns_code) = crate::ssh::session_exec_with_output(session, "nginx -s reload 2>&1", 10)
        .await?;
    let ns_combined = format!("{} {}", ns_stdout, ns_stderr).trim().to_string();
    if ns_code == 0 || ns_combined.is_empty() {
        return Ok(());
    }

    // Both failed — check nginx error log for real reason
    let (log_out, _, _) = crate::ssh::session_exec_with_output(session, "tail -5 /var/log/nginx/error.log 2>/dev/null || journalctl -u nginx --no-pager -n 5 2>/dev/null || echo 'No error log accessible'", 5)
        .await?;
    let log_info = log_out.trim();

    Err(format!("Nginx reload failed. systemctl output: '{}', nginx -s output: '{}'. Recent errors: {}",
        sys_combined, ns_combined, log_info))
}

/// Setup SSL certificate for a site using certbot (streaming output via events)
pub async fn setup_ssl(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
    domain: &str,
    app_handle: &AppHandle,
) -> Result<String, String> {
    let safe_domain = domain.replace('\'', "'\\''");

    let emit = |line: &str, status: &str| {
        let _ = app_handle.emit("ssl-install-progress", serde_json::json!({
            "sessionId": session_id,
            "domain": domain,
            "line": line,
            "status": status,
        }));
    };

    // Check if certbot and nginx plugin are installed
    emit("Checking certbot...", "installing");
    let (certbot_out, _, certbot_code) = crate::ssh::session_exec_with_output(session, "command -v certbot 2>/dev/null", 5)
        .await?;
    let certbot_installed = certbot_code == 0 && !certbot_out.trim().is_empty();

    // Check nginx plugin: certbot plugins 2>/dev/null | grep -q nginx
    let (plugin_out, _, plugin_code) = crate::ssh::session_exec_with_output(session, "certbot plugins 2>/dev/null | grep -q nginx && echo OK", 10)
        .await?;
    let nginx_plugin_installed = plugin_code == 0 && plugin_out.contains("OK");

    if !certbot_installed || !nginx_plugin_installed {
        if !certbot_installed {
            emit("Installing certbot...", "installing");
        } else {
            emit("Installing certbot-nginx plugin...", "installing");
        }
        let os = detect_os(session, cache, session_id).await?;
        let install_cmd = if os.family == "debian" {
            "apt-get install -y certbot python3-certbot-nginx"
        } else {
            "yum install -y --nogpgcheck --assumeyes certbot python3-certbot-nginx || dnf install -y --nogpgcheck --assumeyes certbot python3-certbot-nginx"
        };
        // Stream certbot install output
        let mut install_channel = crate::ssh::session_open_channel(session).await?;
        install_channel.exec(true, install_cmd).await
            .map_err(|e| format!("Failed to install certbot: {}", e))?;
        let install_deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(180);
        loop {
            tokio::select! {
                msg = install_channel.wait() => {
                    match msg {
                        Some(russh::ChannelMsg::Data { data }) | Some(russh::ChannelMsg::ExtendedData { data, .. }) => {
                            let text = String::from_utf8_lossy(&data);
                            for line in text.lines() {
                                if !line.trim().is_empty() { emit(line, "installing"); }
                            }
                        }
                        Some(russh::ChannelMsg::ExitStatus { .. }) | Some(russh::ChannelMsg::Eof) | Some(russh::ChannelMsg::Close) | None => break,
                        _ => {}
                    }
                }
                _ = tokio::time::sleep_until(install_deadline) => {
                    return Err("Certbot installation timed out".to_string());
                }
            }
        }

        // ponytail: if package manager failed to provide nginx plugin, try pip fallback
        let (pip_check, _, pip_code) = crate::ssh::session_exec_with_output(session, "certbot plugins 2>/dev/null | grep -q nginx && echo OK", 10)
            .await?;
        if pip_code != 0 || !pip_check.contains("OK") {
            emit("Package install didn't provide nginx plugin, trying pip...", "installing");
            let pip_cmd = "pip3 install certbot-nginx 2>&1 || pip install certbot-nginx 2>&1";
            let (pip_out, _, _) = crate::ssh::session_exec_with_output(session, pip_cmd, 120)
                .await?;
            for line in pip_out.lines() {
                if !line.trim().is_empty() { emit(line, "installing"); }
            }
        }
    }

    // Detect BT Panel nginx path: certbot expects /etc/nginx/nginx.conf by default
    // BT Panel stores config at /www/server/nginx/conf/nginx.conf
    let (nginx_root_check, _, _) = crate::ssh::session_exec_with_output(session,
            "if [ ! -f /etc/nginx/nginx.conf ] && [ -f /www/server/nginx/conf/nginx.conf ]; then echo /www/server/nginx/conf; fi",
            5).await?;
    let nginx_server_root = nginx_root_check.trim().to_string();
    let root_flag = if !nginx_server_root.is_empty() {
        emit(&format!("BT Panel nginx detected, server root: {}", nginx_server_root), "installing");
        // Create /etc/nginx symlink so certbot's internal path checks work
        let _ = crate::ssh::session_exec_with_output(session,
            "[ ! -e /etc/nginx ] && ln -sf /www/server/nginx/conf /etc/nginx || true",
            5).await?;
        format!("--nginx-server-root '{}'", nginx_server_root)
    } else {
        String::new()
    };

    // Run certbot with streaming output
    let cmd = format!(
        "certbot --nginx {} -d '{}' --non-interactive --agree-tos --register-unsafely-without-email 2>&1",
        root_flag, safe_domain
    );
    emit(&format!("Running: {}", cmd), "installing");

    let mut channel = crate::ssh::session_open_channel(session).await?;
    channel.exec(true, cmd.as_str()).await
        .map_err(|e| format!("Failed to start certbot: {}", e))?;

    let mut full_output = String::new();
    let mut exit_code: i32 = -1;
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(120);

    loop {
        tokio::select! {
            msg = channel.wait() => {
                match msg {
                    Some(russh::ChannelMsg::Data { data }) => {
                        let text = String::from_utf8_lossy(&data);
                        full_output.push_str(&text);
                        for line in text.lines() {
                            if !line.trim().is_empty() { emit(line, "installing"); }
                        }
                    }
                    Some(russh::ChannelMsg::ExtendedData { data, ext }) => {
                        if ext == 1 {
                            let text = String::from_utf8_lossy(&data);
                            full_output.push_str(&text);
                            for line in text.lines() {
                                if !line.trim().is_empty() { emit(line, "installing"); }
                            }
                        }
                    }
                    Some(russh::ChannelMsg::ExitStatus { exit_status }) => {
                        exit_code = exit_status as i32;
                    }
                    Some(russh::ChannelMsg::Eof) | Some(russh::ChannelMsg::Close) | None => break,
                    _ => {}
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                return Err("SSL setup timed out (2 minutes)".to_string());
            }
        }
    }

    // ponytail: russh may deliver ExitStatus after Eof/Close, so exit_code stays -1.
    // Fall back to checking certbot's own success marker in output.
    let script_succeeded = full_output.contains("Successfully deployed certificate")
        || full_output.contains("Certificate is saved at");

    if exit_code == 0 || script_succeeded {
        emit(&format!("SSL certificate installed for {}", domain), "done");
        
        // Verify SSL config was added to nginx vhost
        emit("Verifying SSL configuration...", "installing");
        
        // Check if the specific domain's config file contains ssl_certificate
        let check_ssl_cmd = format!(
            r#"if [ -f '/etc/nginx/sites-enabled/{domain}' ] && grep -q 'ssl_certificate' '/etc/nginx/sites-enabled/{domain}' 2>/dev/null; then echo 'FOUND:/etc/nginx/sites-enabled/{domain}'; elif [ -f '/etc/nginx/conf.d/{domain}.conf' ] && grep -q 'ssl_certificate' '/etc/nginx/conf.d/{domain}.conf' 2>/dev/null; then echo 'FOUND:/etc/nginx/conf.d/{domain}.conf'; elif [ -f '/www/server/panel/vhost/nginx/{domain}.conf' ] && grep -q 'ssl_certificate' '/www/server/panel/vhost/nginx/{domain}.conf' 2>/dev/null; then echo 'FOUND:/www/server/panel/vhost/nginx/{domain}.conf'; elif [ -f '/www/server/nginx/conf/vhost/{domain}.conf' ] && grep -q 'ssl_certificate' '/www/server/nginx/conf/vhost/{domain}.conf' 2>/dev/null; then echo 'FOUND:/www/server/nginx/conf/vhost/{domain}.conf'; else echo 'NOT_FOUND'; fi"#,
            domain = safe_domain
        );
        let (verify_out, _, _) = crate::ssh::session_exec_with_output(session, &check_ssl_cmd, 5)
            .await?;
        
        if verify_out.trim().starts_with("FOUND:") {
            let config_path = verify_out.trim().strip_prefix("FOUND:").unwrap_or("");
            emit(&format!("✓ SSL config verified in: {}", config_path), "done");
            
            // Reload nginx: run test and reload SEPARATELY
            // SSH exit codes are unreliable (-1 means channel didn't receive ExitStatus)
            emit("Reloading Nginx to apply SSL config...", "installing");
            let (test_out, test_err, _test_code) = crate::ssh::session_exec_with_output(session, "nginx -t 2>&1", 5)
                .await?;
            let test_combined = format!("{}{}", test_out, test_err);
            let test_ok = test_combined.contains("syntax is ok") && test_combined.contains("test is successful");
            
            if !test_ok {
                emit(&format!("Nginx config test failed: {}", test_combined.trim()), "error");
            } else {
                let (reload_out, reload_err, _reload_code) = crate::ssh::session_exec_with_output(session, "systemctl reload nginx 2>&1", 10)
                    .await?;
                let reload_combined = format!("{}{}", reload_out, reload_err);
                if reload_combined.to_lowercase().contains("error") || reload_combined.to_lowercase().contains("fail") {
                    emit(&format!("Nginx reload warning: {}", reload_combined.trim()), "error");
                } else {
                    emit("✓ Nginx reloaded successfully", "done");
                }
            }
        } else {
            emit("⚠ Warning: SSL directives not found in expected vhost files. Checking all configs...", "error");
            // Check all enabled configs
            let check_all_cmd = r#"for f in /etc/nginx/sites-enabled/* /etc/nginx/conf.d/*.conf /www/server/panel/vhost/nginx/*.conf /www/server/nginx/conf/vhost/*.conf; do [ -f "$f" ] && grep -q 'ssl_certificate' "$f" 2>/dev/null && echo "SSL found in: $f"; done 2>/dev/null || echo 'No SSL configs found'"#;
            let (all_out, _, _) = crate::ssh::session_exec_with_output(session, check_all_cmd, 5)
                .await?;
            if !all_out.trim().is_empty() && all_out.trim() != "No SSL configs found" {
                emit(&format!("Found SSL in other configs: {}", all_out.trim()), "installing");
            } else {
                emit(" No SSL configuration detected in any nginx vhost file", "error");
            }
        }
        
        Ok(format!("SSL certificate installed for {}", domain))
    } else {
        let err_msg = format!("SSL setup failed (exit code {})", exit_code);
        emit(&err_msg, "error");
        Err(format!("{}:\n{}", err_msg, full_output.trim()))
    }
}

// ===== System Monitor =====

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MonitorData {
    pub cpu_percent: u32,
    pub mem_total_mb: u64,
    pub mem_used_mb: u64,
    pub swap_total_mb: u64,
    pub swap_used_mb: u64,
    pub load_avg: String,
    pub net_rx: String,
    pub net_tx: String,
    pub disk_read: String,
    pub disk_write: String,
    pub top_processes: Vec<ProcessInfo>,
    pub uptime: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProcessInfo {
    pub pid: String,
    pub user: String,
    pub cpu: String,
    pub mem: String,
    pub command: String,
}

/// Get real-time monitoring data
pub async fn get_monitor_data(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
) -> Result<MonitorData, String> {
    let (stdout, _, _) = crate::ssh::session_exec_with_output(session,
            r#"
# CPU usage (1-second sample)
CPU_IDLE=$(top -bn1 | grep 'Cpu(s)' | awk '{print $8}' | tr -d '%,id,' 2>/dev/null)
if [ -z "$CPU_IDLE" ]; then
  CPU_IDLE=$(mpstat 1 1 2>/dev/null | tail -1 | awk '{print $NF}')
fi
CPU_USED=$(echo "$CPU_IDLE" | awk '{printf "%d", 100 - $1}')
echo "CPU=$CPU_USED"

# Memory
free -m | awk '/^Mem:/ {print "MEM_TOTAL=" $2; print "MEM_USED=" $3}'
free -m | awk '/^Swap:/ {print "SWAP_TOTAL=" $2; print "SWAP_USED=" $3}'

# Load
echo "LOAD=$(cat /proc/loadavg | awk '{print $1, $2, $3}')"

# Uptime
echo "UPTIME=$(uptime -p 2>/dev/null || uptime | sed 's/.*up /up /' | sed 's/,* *[0-9]* user.*//')"

# Network (from /proc/net/dev)
echo "---NET---"
cat /proc/net/dev | grep -v 'lo:' | tail -n +3 | awk '{print $1, $2, $10}'

# Disk I/O (from /proc/diskstats)
echo "---DISK---"
cat /proc/diskstats | grep -E '^(sd[a-z]|vd[a-z]|nvme[0-9]n[0-9]) ' | head -4

# Top processes
echo "---PROC---"
ps aux --sort=-%cpu | head -11 | tail -10
"#,
            15,
        )
        .await?;

    let mut data = MonitorData {
        cpu_percent: 0,
        mem_total_mb: 0,
        mem_used_mb: 0,
        swap_total_mb: 0,
        swap_used_mb: 0,
        load_avg: String::new(),
        net_rx: "0 B".to_string(),
        net_tx: "0 B".to_string(),
        disk_read: "0 B".to_string(),
        disk_write: "0 B".to_string(),
        top_processes: Vec::new(),
        uptime: String::new(),
    };

    let mut section = "";
    let mut total_net_rx: u64 = 0;
    let mut total_net_tx: u64 = 0;

    for line in stdout.lines() {
        if line.starts_with("---NET---") {
            section = "net";
            continue;
        }
        if line.starts_with("---DISK---") {
            section = "disk";
            continue;
        }
        if line.starts_with("---PROC---") {
            section = "proc";
            continue;
        }

        match section {
            "net" => {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 {
                    total_net_rx += parts[1].parse::<u64>().unwrap_or(0);
                    total_net_tx += parts[2].parse::<u64>().unwrap_or(0);
                }
            }
            "disk" => {
                // Simplified: just sum up sectors read/written
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 10 {
                    let r_sectors = parts[5].parse::<u64>().unwrap_or(0);
                    let w_sectors = parts[9].parse::<u64>().unwrap_or(0);
                    let r_mb = r_sectors * 512 / 1024 / 1024;
                    let w_mb = w_sectors * 512 / 1024 / 1024;
                    data.disk_read = format!("{} MB", r_mb);
                    data.disk_write = format!("{} MB", w_mb);
                }
            }
            "proc" => {
                // ps aux output: USER PID %CPU %MEM VSZ RSS TTY STAT START TIME COMMAND
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 11 {
                    data.top_processes.push(ProcessInfo {
                        pid: parts[1].to_string(),
                        user: parts[0].to_string(),
                        cpu: parts[2].to_string(),
                        mem: parts[3].to_string(),
                        command: parts[10..].join(" "),
                    });
                }
            }
            _ => {
                if let Some((key, val)) = line.split_once('=') {
                    let val = val.trim();
                    match key.trim() {
                        "CPU" => data.cpu_percent = val.parse().unwrap_or(0),
                        "MEM_TOTAL" => data.mem_total_mb = val.parse().unwrap_or(0),
                        "MEM_USED" => data.mem_used_mb = val.parse().unwrap_or(0),
                        "SWAP_TOTAL" => data.swap_total_mb = val.parse().unwrap_or(0),
                        "SWAP_USED" => data.swap_used_mb = val.parse().unwrap_or(0),
                        "LOAD" => data.load_avg = val.to_string(),
                        "UPTIME" => data.uptime = val.replace("up ", ""),
                        _ => {}
                    }
                }
            }
        }
    }

    data.net_rx = format_bytes(total_net_rx);
    data.net_tx = format_bytes(total_net_tx);

    Ok(data)
}

fn format_bytes(bytes: u64) -> String {
    if bytes > 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / 1024.0 / 1024.0 / 1024.0)
    } else if bytes > 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / 1024.0 / 1024.0)
    } else if bytes > 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

// ===== Firewall Management =====

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FirewallRule {
    pub id: String,          // unique identifier for the rule
    pub port: String,        // e.g. "80", "8080-8090"
    pub protocol: String,    // "tcp", "udp", "both"
    pub action: String,      // "allow", "deny", "reject"
    pub source: String,      // "Anywhere", specific IP, etc.
    pub raw: String,         // original rule line for display
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FirewallInfo {
    pub firewall_type: String, // "ufw", "firewalld", "iptables", "none"
    pub enabled: bool,
    pub rules: Vec<FirewallRule>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FirewallToggleResult {
    pub enabled: bool,
    pub ssh_port_auto_opened: bool,
    pub ssh_port: u16,
}

pub async fn get_firewall_rules(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
) -> Result<FirewallInfo, String> {
    // ponytail: cache firewall rules for 60s (changes only on add/remove)
    if let Some(cached) = cache.get(session_id, "firewall", 60) {
        if let Ok(info) = serde_json::from_str::<FirewallInfo>(&cached) {
            return Ok(info);
        }
    }
    // ponytail: single SSH round-trip combining firewall detection + query (was 2 calls)
    let (stdout, _, _) = crate::ssh::session_exec_with_output(session,
            r#"
if command -v ufw >/dev/null 2>&1; then
  echo "FW_TYPE=ufw"
  ufw status 2>/dev/null || echo "UFW_ERROR"
elif command -v firewall-cmd >/dev/null 2>&1; then
  echo "FW_TYPE=firewalld"
  firewall-cmd --state 2>/dev/null
  firewall-cmd --list-ports 2>/dev/null
  echo "---"
  firewall-cmd --list-rich-rules 2>/dev/null
elif command -v iptables >/dev/null 2>&1; then
  echo "FW_TYPE=iptables"
  iptables -L -n --line-numbers 2>/dev/null
else
  echo "FW_TYPE=none"
fi
"#,
            15,
        )
        .await?;

    let fw_type = stdout
        .lines()
        .find(|l| l.starts_with("FW_TYPE="))
        .map(|l| l.strip_prefix("FW_TYPE=").unwrap_or("none").to_string())
        .unwrap_or_else(|| "none".to_string());

    let result = match fw_type.as_str() {
        "ufw" => parse_ufw_output(&stdout),
        "firewalld" => parse_firewalld_output(&stdout),
        "iptables" => parse_iptables_output(&stdout),
        _ => Ok(FirewallInfo {
            firewall_type: "none".to_string(),
            enabled: false,
            rules: vec![],
        }),
    };
    // ponytail: cache firewall rules
    if let Ok(ref info) = result {
        if let Ok(json) = serde_json::to_string(info) {
            cache.put(session_id, "firewall", json);
        }
    }
    result
}

fn parse_ufw_output(stdout: &str) -> Result<FirewallInfo, String> {
    if stdout.contains("UFW_ERROR") || stdout.contains("not found") {
        return Ok(FirewallInfo { firewall_type: "none".to_string(), enabled: false, rules: vec![] });
    }

    let enabled = stdout.contains("Status: active");
    let mut rules = Vec::new();
    let mut id_counter = 0;
    let mut past_separator = false;

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("FW_TYPE=") { continue; }
        if line.starts_with("Status:") { continue; }
        if line.starts_with("--") && line.contains("--") && line.len() > 10 {
            past_separator = true;
            continue;
        }
        if !past_separator { continue; }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 {
            let port_proto = parts[0];
            let action = parts[1];
            let source = parts[2..].join(" ");
            if action == "PROFILES:" || port_proto == "New" { continue; }
            let (port, protocol) = if let Some((p, proto)) = port_proto.split_once('/') {
                (p.to_string(), proto.to_string())
            } else {
                (port_proto.to_string(), "any".to_string())
            };
            id_counter += 1;
            rules.push(FirewallRule {
                id: id_counter.to_string(), port, protocol,
                action: action.to_lowercase(), source, raw: line.to_string(),
            });
        }
    }

    Ok(FirewallInfo { firewall_type: "ufw".to_string(), enabled, rules })
}

fn parse_firewalld_output(stdout: &str) -> Result<FirewallInfo, String> {
    let enabled = stdout.contains("running");
    let mut rules = Vec::new();
    let mut id_counter = 0;

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() || line == "running" || line == "not running" || line == "---" || line.starts_with("FW_TYPE=") { continue; }
        for entry in line.split_whitespace() {
            if entry.contains('/') {
                let (port, protocol) = entry.split_once('/').unwrap_or((entry, "tcp"));
                id_counter += 1;
                rules.push(FirewallRule {
                    id: id_counter.to_string(), port: port.to_string(),
                    protocol: protocol.to_string(), action: "allow".to_string(),
                    source: "Anywhere".to_string(), raw: entry.to_string(),
                });
            }
        }
        if line.starts_with("rule") {
            id_counter += 1;
            rules.push(FirewallRule {
                id: id_counter.to_string(), port: "-".to_string(),
                protocol: "-".to_string(), action: "allow".to_string(),
                source: "Anywhere".to_string(), raw: line.to_string(),
            });
        }
    }

    Ok(FirewallInfo { firewall_type: "firewalld".to_string(), enabled, rules })
}

fn parse_iptables_output(stdout: &str) -> Result<FirewallInfo, String> {
    let enabled = !stdout.contains("command not found") && !stdout.is_empty();
    let mut rules = Vec::new();
    let mut id_counter = 0;
    let mut current_chain = String::new();

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("FW_TYPE=") { continue; }
        if line.starts_with("Chain ") {
            current_chain = line.split_whitespace().nth(1).unwrap_or("").to_string();
            continue;
        }
        if line.starts_with("num") || line.starts_with("target") { continue; }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 6 {
            let target = parts[1];
            let protocol = parts[2];
            let source = parts[4];
            let dest = parts[5];
            let extra = parts[6..].join(" ");
            let port = if let Some(pos) = extra.find("dpt:") {
                extra[pos + 4..].split_whitespace().next().unwrap_or("-").to_string()
            } else if let Some(pos) = extra.find("dpts:") {
                extra[pos + 5..].split_whitespace().next().unwrap_or("-").to_string()
            } else {
                "-".to_string()
            };

            if target == "ACCEPT" || target == "DROP" || target == "REJECT" {
                id_counter += 1;
                let action = match target {
                    "ACCEPT" => "allow",
                    "DROP" => "deny",
                    "REJECT" => "reject",
                    _ => target,
                };
                rules.push(FirewallRule {
                    id: id_counter.to_string(),
                    port,
                    protocol: protocol.to_lowercase(),
                    action: action.to_string(),
                    source: if source == "0.0.0.0/0" { "Anywhere".to_string() } else { source.to_string() },
                    raw: format!("[{}] {} {} {} -> {} {}", current_chain, target, protocol, source, dest, extra),
                });
            }
        }
    }

    Ok(FirewallInfo { firewall_type: "iptables".to_string(), enabled: enabled || !rules.is_empty(), rules })
}

pub async fn add_firewall_rule(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    port: &str,
    protocol: &str,
    action: &str,
) -> Result<String, String> {
    // Detect firewall type
    let (stdout, _, _) = crate::ssh::session_exec_with_output(session, "command -v ufw && echo HAS_UFW; command -v firewall-cmd && echo HAS_FIREWALLD", 10)
        .await?;

    let cmd = if stdout.contains("HAS_UFW") {
        let proto = if protocol == "both" || protocol == "any" { "" } else { protocol };
        let action_ufw = if action == "allow" { "allow" } else { "deny" };
        if proto.is_empty() {
            format!("ufw {} {}", action_ufw, port)
        } else {
            format!("ufw {} {}/{}", action_ufw, port, proto)
        }
    } else if stdout.contains("HAS_FIREWALLD") {
        let proto = if protocol == "both" || protocol == "any" { "tcp" } else { protocol };
        format!("firewall-cmd --permanent --add-port={}/{}", port, proto)
    } else {
        let target = match action {
            "allow" => "ACCEPT",
            "deny" => "DROP",
            _ => "REJECT",
        };
        let proto = if protocol == "both" || protocol == "any" { "tcp" } else { protocol };
        format!("iptables -I INPUT -p {} --dport {} -j {}", proto, port, target)
    };

    let (stdout, stderr, code) = crate::ssh::session_exec_with_output(session, &cmd, 15).await?;
    // ponytail: ufw/firewalld may return non-zero exit on warnings; check for actual error text
    let combined = format!("{} {}", stdout, stderr);
    let has_real_error = combined.contains("ERROR") || combined.contains("denied")
        || combined.contains("failed") || combined.contains("iptables: ");
    if code != 0 && has_real_error {
        return Err(format!("Failed: {}", combined.trim()));
    }

    // Reload if firewalld
    if stdout.contains("HAS_FIREWALLD") || cmd.starts_with("firewall-cmd") {
        let _ = crate::ssh::session_exec_with_output(session, "firewall-cmd --reload", 15).await;
    }

    Ok(format!("Added rule: {}/{} ({})", port, protocol, action))
}

pub async fn remove_firewall_rule(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    port: &str,
    protocol: &str,
    action: &str,
) -> Result<String, String> {
    let (stdout, _, _) = crate::ssh::session_exec_with_output(session, "command -v ufw && echo HAS_UFW; command -v firewall-cmd && echo HAS_FIREWALLD", 10)
        .await?;

    let cmd = if stdout.contains("HAS_UFW") {
        let proto = if protocol == "both" || protocol == "any" { "" } else { protocol };
        let action_ufw = if action == "allow" { "allow" } else { "deny" };
        if proto.is_empty() {
            format!("ufw delete {} {}", action_ufw, port)
        } else {
            format!("ufw delete {} {}/{}", action_ufw, port, proto)
        }
    } else if stdout.contains("HAS_FIREWALLD") {
        let proto = if protocol == "both" || protocol == "any" { "tcp" } else { protocol };
        format!("firewall-cmd --permanent --remove-port={}/{}", port, proto)
    } else {
        let target = match action {
            "allow" => "ACCEPT",
            "deny" => "DROP",
            _ => "REJECT",
        };
        let proto = if protocol == "both" || protocol == "any" { "tcp" } else { protocol };
        format!("iptables -D INPUT -p {} --dport {} -j {}", proto, port, target)
    };

    let (stdout_out, stderr, code) = crate::ssh::session_exec_with_output(session, &cmd, 15).await?;
    let combined = format!("{} {}", stdout_out, stderr);
    let has_real_error = combined.contains("ERROR") || combined.contains("denied")
        || combined.contains("failed") || combined.contains("iptables: ");
    if code != 0 && has_real_error {
        return Err(format!("Failed: {}", combined.trim()));
    }

    // Reload if firewalld
    if cmd.starts_with("firewall-cmd") {
        let _ = crate::ssh::session_exec_with_output(session, "firewall-cmd --reload", 15).await;
    }

    Ok(format!("Removed rule: {}/{} ({})", port, protocol, action))
}

pub async fn toggle_firewall(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    enable: bool,
) -> Result<FirewallToggleResult, String> {
    let (detect, _, _) = crate::ssh::session_exec_with_output(session, "command -v ufw && echo HAS_UFW; command -v firewall-cmd && echo HAS_FIREWALLD", 10)
        .await?;

    let ssh_port = Some(session.connect_info.clone()).map(|i| i.port).unwrap_or(22);
    let mut ssh_port_auto_opened = false;

    let action = if enable { "enable" } else { "disable" };

    // firewalld: must start the service BEFORE adding rules (firewall-cmd fails if not running)
    if enable && detect.contains("HAS_FIREWALLD") && !detect.contains("HAS_UFW") {
        let _ = crate::ssh::session_exec_with_output(session, "systemctl start firewalld", 15)
            .await;
    }

    // Safety: when enabling, pre-allow the SSH port to prevent lockout
    if enable {
        if detect.contains("HAS_UFW") {
            let (out, err, code) = crate::ssh::session_exec_with_output(session, &format!("ufw allow {}/tcp", ssh_port), 15)
                .await
                .unwrap_or_else(|_| (String::new(), String::new(), 1));
            if code == 0 && !format!("{} {}", out, err).contains("ERROR") {
                ssh_port_auto_opened = true;
            }
        } else if detect.contains("HAS_FIREWALLD") {
            let (out, err, code) = crate::ssh::session_exec_with_output(session,
                    &format!("firewall-cmd --permanent --add-port={}/tcp", ssh_port),
                    15,
                )
                .await
                .unwrap_or_else(|_| (String::new(), String::new(), 1));
            if code == 0 && !format!("{} {}", out, err).contains("Error") {
                ssh_port_auto_opened = true;
            }
        }
    }

    let (stdout, stderr, code) = if detect.contains("HAS_UFW") {
        let cmd = if enable {
            "echo 'y' | ufw --force enable"
        } else {
            "ufw --force disable"
        };
        crate::ssh::session_exec_with_output(session, cmd, 15).await?
    } else if detect.contains("HAS_FIREWALLD") {
        let cmd = if enable {
            // firewalld was already started above; just enable for boot persistence
            "systemctl enable firewalld"
        } else {
            "systemctl stop firewalld && systemctl disable firewalld"
        };
        crate::ssh::session_exec_with_output(session, cmd, 15).await?
    } else {
        return Err("No supported firewall found".to_string());
    };

    // firewalld: reload to apply the pre-added SSH port rule
    if enable && ssh_port_auto_opened && detect.contains("HAS_FIREWALLD") {
        let _ = crate::ssh::session_exec_with_output(session, "firewall-cmd --reload", 15).await;
    }

    let combined = format!("{} {}", stdout, stderr);
    if code != 0 && (combined.contains("ERROR") || combined.contains("failed")) {
        return Err(format!("Failed to {} firewall: {}", action, combined.trim()));
    }

    Ok(FirewallToggleResult {
        enabled: enable,
        ssh_port_auto_opened,
        ssh_port,
    })
}

// ===== Software Repository =====

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SoftwareInfo {
    pub name: String,
    pub display_name: String,
    pub category: String,
    pub installed: bool,
    pub version: String,
    pub service_name: String,
    pub running: bool,
}

/// Get list of available software and their install status
pub async fn get_software_list(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
) -> Result<Vec<SoftwareInfo>, String> {
    // ponytail: cache software list for connection lifetime (changes only on install/uninstall)
    if let Some(cached) = cache.get(session_id, "software_list", 0) {
        if let Ok(list) = serde_json::from_str::<Vec<SoftwareInfo>>(&cached) {
            return Ok(list);
        }
    }
    // ponytail: single SSH call to check all software status
    let cmd = r#"
# Check Nginx (standard + BT Panel)
if command -v nginx &>/dev/null || [ -x /www/server/nginx/sbin/nginx ]; then
  echo "NGINX_INSTALLED=1"
  echo "NGINX_VERSION=$(nginx -v 2>&1 || /www/server/nginx/sbin/nginx -v 2>&1 | grep -oP '[\d.]+' || echo '')"
  echo "NGINX_RUNNING=$(systemctl is-active nginx 2>/dev/null || echo inactive)"
else
  echo "NGINX_INSTALLED=0"
fi

# Check Apache versions (standard + BT Panel)
# Detect actual installed version by querying the binary
_apache_installed=0
_apache_version=""
_apache_running="inactive"
_apache_service="apache2"

# Check standard Apache
if command -v apache2 &>/dev/null || [ -x /usr/sbin/apache2 ]; then
  _apache_installed=1
  _apache_version=$(apache2 -v 2>/dev/null | grep -oP 'Apache/[\d.]+' | head -1 | sed 's/Apache\///' || echo '')
  if systemctl is-active apache2 &>/dev/null; then
    _apache_running="active"
  fi
# Check BT Panel Apache
elif [ -x /www/server/apache/bin/httpd ]; then
  _apache_installed=1
  _apache_version=$(/www/server/apache/bin/httpd -v 2>/dev/null | grep -oP '[\d]+\.[\d]+\.[\d]+' | head -1 || echo '')
  # BT Panel may use different service name
  for _svc in apache httpd Baota-Apache; do
    if systemctl is-active "$_svc" &>/dev/null; then
      _apache_running="active"
      _apache_service="$_svc"
      break
    fi
  done
fi

# Output based on detected major version (2.2 or 2.4)
if [ $_apache_installed -eq 1 ]; then
  # Determine major version from full version string
  if [[ "$_apache_version" == 2.4* ]]; then
    echo "APACHE_2_4_INSTALLED=1"
    echo "APACHE_2_4_VERSION=$_apache_version"
    echo "APACHE_2_4_RUNNING=$_apache_running"
    echo "APACHE_2_4_SERVICE=$_apache_service"
    echo "APACHE_2_2_INSTALLED=0"
  elif [[ "$_apache_version" == 2.2* ]]; then
    echo "APACHE_2_2_INSTALLED=1"
    echo "APACHE_2_2_VERSION=$_apache_version"
    echo "APACHE_2_2_RUNNING=$_apache_running"
    echo "APACHE_2_2_SERVICE=$_apache_service"
    echo "APACHE_2_4_INSTALLED=0"
  else
    # Default to 2.4 if version detection fails
    echo "APACHE_2_4_INSTALLED=1"
    echo "APACHE_2_4_VERSION=${_apache_version:-2.4.x}"
    echo "APACHE_2_4_RUNNING=$_apache_running"
    echo "APACHE_2_4_SERVICE=$_apache_service"
    echo "APACHE_2_2_INSTALLED=0"
  fi
else
  echo "APACHE_2_4_INSTALLED=0"
  echo "APACHE_2_2_INSTALLED=0"
fi

# Legacy single Apache detection (fallback)
if command -v apache2 &>/dev/null || command -v httpd &>/dev/null || [ -x /www/server/apache/bin/httpd ]; then
  echo "APACHE_INSTALLED=1"
  echo "APACHE_VERSION=$(apache2 -v 2>/dev/null || httpd -v 2>/dev/null || /www/server/apache/bin/httpd -v 2>/dev/null | grep -oP '[\d]+\.[\d]+\.[\d]+' | head -1 || echo '')"
  APACHE_SVC=$(systemctl list-units --type=service 2>/dev/null | grep -E 'apache|httpd' | awk '{print $1}' | head -1 | sed 's/.service//')
  echo "APACHE_SERVICE=$APACHE_SVC"
  if [ -n "$APACHE_SVC" ] && systemctl is-active "$APACHE_SVC" &>/dev/null; then
    echo "APACHE_RUNNING=active"
  else
    echo "APACHE_RUNNING=inactive"
  fi
else
  echo "APACHE_INSTALLED=0"
fi

# Check MySQL/MariaDB (standard + BT Panel)
# Check for MySQL/MariaDB server specifically (not just client)
# Use server binary + package state as primary checks; systemctl as fallback
if command -v mysqld &>/dev/null || command -v mariadbd &>/dev/null || [ -x /www/server/mysql/bin/mysqld ] || dpkg -l mysql-server mysql-community-server mariadb-server 2>/dev/null | grep -q '^ii' || rpm -q mysql-community-server MariaDB-server 2>/dev/null | grep -q '^mysql\|^MariaDB'; then
  echo "MYSQL_INSTALLED=1"
  echo "MYSQL_VERSION=$(mysqld --version 2>/dev/null || mariadbd --version 2>/dev/null || /www/server/mysql/bin/mysql --version 2>/dev/null | head -1 || mysql --version 2>/dev/null | head -1 || echo '')"
  if systemctl is-active mysql &>/dev/null; then
    echo "MYSQL_RUNNING=active"
    echo "MYSQL_SERVICE=mysql"
  elif systemctl is-active mysqld &>/dev/null; then
    echo "MYSQL_RUNNING=active"
    echo "MYSQL_SERVICE=mysqld"
  elif systemctl is-active mariadb &>/dev/null; then
    echo "MYSQL_RUNNING=active"
    echo "MYSQL_SERVICE=mariadb"
  else
    echo "MYSQL_RUNNING=inactive"
    echo "MYSQL_SERVICE=$(systemctl list-units --type=service 2>/dev/null | grep -E 'mysql|maria' | awk '{print $1}' | head -1 | sed 's/.service//')"
  fi
else
  echo "MYSQL_INSTALLED=0"
fi

# Check PHP versions — dynamic scan for any installed PHP-FPM
# ponytail: no hardcoded version list — detect whatever is on the system
for _svc in $(systemctl list-unit-files --type=service 2>/dev/null | grep -oE 'php[0-9]+\.[0-9]+-fpm' | sed 's/.service$//' | sort -uV); do
  phpver=$(echo "$_svc" | sed -E 's/^php([0-9]+\.[0-9]+)-fpm$/\1/')
  _bin="/usr/sbin/php-fpm-${phpver}"
  _btbin="/www/server/php/${phpver}/sbin/php-fpm"
  if systemctl is-enabled "$_svc" &>/dev/null || [ -x "$_bin" ] || [ -x "$_btbin" ]; then
    echo "PHP_DETECT_VERSION=${phpver}"
    if [ -x "$_bin" ]; then
      _fullver=$("$_bin" -v 2>/dev/null | head -1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' || echo "${phpver}.x")
    elif [ -x "$_btbin" ]; then
      _fullver=$("$_btbin" -v 2>/dev/null | head -1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' || echo "${phpver}.x")
    else
      _fullver="${phpver}.x"
    fi
    echo "PHP_DETECT_FULLVER=${_fullver}"
    echo "PHP_DETECT_SERVICE=${_svc}"
    if systemctl is-active "$_svc" &>/dev/null; then
      echo "PHP_DETECT_RUNNING=active"
    else
      echo "PHP_DETECT_RUNNING=inactive"
    fi
  fi
done
# BT Panel: scan for PHP versions not caught by systemd
if [ -d /www/server/php ]; then
  for _btdir in /www/server/php/*/; do
    [ -d "$_btdir" ] || continue
    phpver=$(basename "$_btdir")
    echo "$phpver" | grep -qE '^[0-9]+\.[0-9]+$' || continue
    _btbin="/www/server/php/${phpver}/sbin/php-fpm"
    [ -x "$_btbin" ] || continue
    # Skip if already detected by systemd
    _svc="php${phpver}-fpm"
    if ! systemctl list-unit-files --type=service 2>/dev/null | grep -q "${_svc}"; then
      echo "PHP_DETECT_VERSION=${phpver}"
      _fullver=$("$_btbin" -v 2>/dev/null | head -1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' || echo "${phpver}.x")
      echo "PHP_DETECT_FULLVER=${_fullver}"
      echo "PHP_DETECT_SERVICE=$_svc"
      echo "PHP_DETECT_RUNNING=inactive"
    fi
  done
fi

# Generic PHP detection (for always-visible install card)
if command -v php &>/dev/null; then
  echo "PHP_GENERIC_INSTALLED=1"
  echo "PHP_GENERIC_VERSION=$(php -v 2>/dev/null | head -1 | grep -oP '[\d]+\.[\d]+\.[\d]+' || echo '')"
  PHP_GENERIC_SVC=$(systemctl list-units --type=service 2>/dev/null | grep -E 'php[0-9.]*-fpm' | awk '{print $1}' | head -1 | sed 's/.service//')
  echo "PHP_GENERIC_SERVICE=$PHP_GENERIC_SVC"
  if [ -n "$PHP_GENERIC_SVC" ] && systemctl is-active "$PHP_GENERIC_SVC" &>/dev/null; then
    echo "PHP_GENERIC_RUNNING=active"
  else
    echo "PHP_GENERIC_RUNNING=inactive"
  fi
else
  echo "PHP_GENERIC_INSTALLED=0"
fi

# Check Redis
if command -v redis-server &>/dev/null; then
  echo "REDIS_INSTALLED=1"
  echo "REDIS_VERSION=$(redis-server --version 2>/dev/null | grep -oP 'v=[\d.]+' | cut -d= -f2 || echo '')"
  echo "REDIS_RUNNING=$(systemctl is-active redis 2>/dev/null || systemctl is-active redis-server 2>/dev/null || echo inactive)"
else
  echo "REDIS_INSTALLED=0"
fi

# Check Memcached
if command -v memcached &>/dev/null; then
  echo "MEMCACHED_INSTALLED=1"
  echo "MEMCACHED_VERSION=$(memcached -h 2>/dev/null | head -1 | grep -oP '[\d.]+' || echo '')"
  echo "MEMCACHED_RUNNING=$(systemctl is-active memcached 2>/dev/null || echo inactive)"
else
  echo "MEMCACHED_INSTALLED=0"
fi

# Check Node.js
if command -v node &>/dev/null; then
  echo "NODEJS_INSTALLED=1"
  echo "NODEJS_VERSION=$(node -v 2>/dev/null | sed 's/^v//' || echo '')"
  echo "NODEJS_RUNNING=n/a"
else
  echo "NODEJS_INSTALLED=0"
fi

# Check zip
if command -v zip &>/dev/null; then
  echo "ZIP_INSTALLED=1"
  echo "ZIP_VERSION=$(zip -v 2>/dev/null | head -1 | grep -oP '[\d.]+' || echo '')"
else
  echo "ZIP_INSTALLED=0"
fi

# Check unzip
if command -v unzip &>/dev/null; then
  echo "UNZIP_INSTALLED=1"
  echo "UNZIP_VERSION=$(unzip -v 2>/dev/null | head -1 | grep -oP '[\d.]+' || echo '')"
else
  echo "UNZIP_INSTALLED=0"
fi

# Check Docker
if command -v docker &>/dev/null; then
  echo "DOCKER_INSTALLED=1"
  echo "DOCKER_VERSION=$(docker -v 2>/dev/null | grep -oP '[\d]+\.[\d]+\.[\d]+' | head -1 || echo '')"
  echo "DOCKER_RUNNING=$(systemctl is-active docker 2>/dev/null || echo inactive)"
else
  echo "DOCKER_INSTALLED=0"
fi

# Check PostgreSQL
# Check for PostgreSQL server specifically (not just psql client)
# Use server binary + package state as primary checks; systemctl as fallback
if command -v postgres &>/dev/null || [ -x /usr/lib/postgresql/*/bin/postgres ] || dpkg -l postgresql 2>/dev/null | grep -q '^ii' || rpm -q postgresql-server 2>/dev/null | grep -q '^postgresql'; then
  echo "PGSQL_INSTALLED=1"
  echo "PGSQL_VERSION=$(psql -V 2>/dev/null | grep -oP '[\d]+\.[\d]+' | head -1 || echo '')"
  echo "PGSQL_RUNNING=$(systemctl is-active postgresql 2>/dev/null || echo inactive)"
else
  echo "PGSQL_INSTALLED=0"
fi
"#;

    let (stdout, stderr, _) = crate::ssh::session_exec_with_output(session, cmd, 20).await?;
    let combined = format!("{}{}", stdout, stderr);

    let get = |key: &str| -> String {
        combined
            .lines()
            .find(|l| l.starts_with(key))
            .map(|l| l.split('=').nth(1).unwrap_or("").trim().to_string())
            .unwrap_or_default()
    };

    let mut list = Vec::new();

    // Nginx
    list.push(SoftwareInfo {
        name: "nginx".to_string(),
        display_name: "Nginx".to_string(),
        category: "web".to_string(),
        installed: get("NGINX_INSTALLED") == "1",
        version: get("NGINX_VERSION"),
        service_name: "nginx".to_string(),
        running: get("NGINX_RUNNING") == "active",
    });

    // Apache - detect all installed versions
    let apache_versions = ["2.2", "2.4"];
    for apachever in &apache_versions {
        let key = format!("APACHE_{}_INSTALLED", apachever.replace('.', "_"));
        if get(&key) == "1" {
            let ver_key = format!("APACHE_{}_VERSION", apachever.replace('.', "_"));
            let run_key = format!("APACHE_{}_RUNNING", apachever.replace('.', "_"));
            let svc_key = format!("APACHE_{}_SERVICE", apachever.replace('.', "_"));
            list.push(SoftwareInfo {
                name: format!("apache{}", apachever),
                display_name: format!("Apache {}", apachever),
                category: "web".to_string(),
                installed: true,
                version: get(&ver_key),
                service_name: get(&svc_key),
                running: get(&run_key) == "active",
            });
        }
    }
    // Fallback: if no versioned Apache found but legacy APACHE_INSTALLED=1, add generic entry
    if list.iter().all(|s| !s.name.starts_with("apache")) && get("APACHE_INSTALLED") == "1" {
        list.push(SoftwareInfo {
            name: "apache".to_string(),
            display_name: "Apache".to_string(),
            category: "web".to_string(),
            installed: true,
            version: get("APACHE_VERSION"),
            service_name: get("APACHE_SERVICE"),
            running: get("APACHE_RUNNING") == "active",
        });
    }

    // MySQL/MariaDB
    list.push(SoftwareInfo {
        name: "mysql".to_string(),
        display_name: "MySQL / MariaDB".to_string(),
        category: "database".to_string(),
        installed: get("MYSQL_INSTALLED") == "1",
        version: get("MYSQL_VERSION"),
        service_name: get("MYSQL_SERVICE"),
        running: get("MYSQL_RUNNING") == "active",
    });

    // PHP - detect all installed versions (dynamic, no hardcoded list)
    // ponytail: parse PHP_DETECT groups from detection script output
    let output_lines: Vec<&str> = combined.lines().collect();
    let mut i = 0;
    while i < output_lines.len() {
        if let Some(ver) = output_lines[i].strip_prefix("PHP_DETECT_VERSION=") {
            let ver = ver.trim();
            let fullver = output_lines.get(i + 1)
                .and_then(|l| l.strip_prefix("PHP_DETECT_FULLVER="))
                .unwrap_or("");
            let svc = output_lines.get(i + 2)
                .and_then(|l| l.strip_prefix("PHP_DETECT_SERVICE="))
                .unwrap_or("");
            let running = output_lines.get(i + 3)
                .map(|l| l.trim() == "PHP_DETECT_RUNNING=active")
                .unwrap_or(false);
            list.push(SoftwareInfo {
                name: format!("php{}", ver),
                display_name: format!("PHP {} FPM", ver),
                category: "web".to_string(),
                installed: true,
                version: fullver.to_string(),
                service_name: svc.to_string(),
                running,
            });
            i += 4;
        } else {
            i += 1;
        }
    }

    // Generic PHP entry (always shown for install card)
    list.push(SoftwareInfo {
        name: "php".to_string(),
        display_name: "PHP-FPM".to_string(),
        category: "web".to_string(),
        installed: get("PHP_GENERIC_INSTALLED") == "1",
        version: get("PHP_GENERIC_VERSION"),
        service_name: get("PHP_GENERIC_SERVICE"),
        running: get("PHP_GENERIC_RUNNING") == "active",
    });

    // Redis
    list.push(SoftwareInfo {
        name: "redis".to_string(),
        display_name: "Redis".to_string(),
        category: "database".to_string(),
        installed: get("REDIS_INSTALLED") == "1",
        version: get("REDIS_VERSION"),
        service_name: "redis".to_string(),
        running: get("REDIS_RUNNING") == "active",
    });

    // Memcached
    list.push(SoftwareInfo {
        name: "memcached".to_string(),
        display_name: "Memcached".to_string(),
        category: "database".to_string(),
        installed: get("MEMCACHED_INSTALLED") == "1",
        version: get("MEMCACHED_VERSION"),
        service_name: "memcached".to_string(),
        running: get("MEMCACHED_RUNNING") == "active",
    });

    // Node.js
    list.push(SoftwareInfo {
        name: "nodejs".to_string(),
        display_name: "Node.js".to_string(),
        category: "runtime".to_string(),
        installed: get("NODEJS_INSTALLED") == "1",
        version: get("NODEJS_VERSION"),
        service_name: String::new(),
        running: false,
    });

    // zip
    list.push(SoftwareInfo {
        name: "zip".to_string(),
        display_name: "Zip".to_string(),
        category: "tools".to_string(),
        installed: get("ZIP_INSTALLED") == "1",
        version: get("ZIP_VERSION"),
        service_name: String::new(),
        running: false,
    });

    // unzip
    list.push(SoftwareInfo {
        name: "unzip".to_string(),
        display_name: "Unzip".to_string(),
        category: "tools".to_string(),
        installed: get("UNZIP_INSTALLED") == "1",
        version: get("UNZIP_VERSION"),
        service_name: String::new(),
        running: false,
    });

    // Docker
    list.push(SoftwareInfo {
        name: "docker".to_string(),
        display_name: "Docker".to_string(),
        category: "container".to_string(),
        installed: get("DOCKER_INSTALLED") == "1",
        version: get("DOCKER_VERSION"),
        service_name: "docker".to_string(),
        running: get("DOCKER_RUNNING") == "active",
    });

    // PostgreSQL
    list.push(SoftwareInfo {
        name: "postgresql".to_string(),
        display_name: "PostgreSQL".to_string(),
        category: "database".to_string(),
        installed: get("PGSQL_INSTALLED") == "1",
        version: get("PGSQL_VERSION"),
        service_name: "postgresql".to_string(),
        running: get("PGSQL_RUNNING") == "active",
    });

    // ponytail: cache software list
    if let Ok(json) = serde_json::to_string(&list) {
        cache.put(session_id, "software_list", json);
    }
    Ok(list)
}

/// Detect status of user-added custom software packages
pub async fn detect_custom_software(
    session: &SshSession,
    packages: &[String],
) -> Result<Vec<SoftwareInfo>, String> {
    if packages.is_empty() {
        return Ok(Vec::new());
    }
    // ponytail: single SSH call checks all custom packages
    let mut script = String::from("#!/bin/bash\n");
    for pkg in packages {
        // Sanitize: only allow alphanumeric, dash, dot, underscore, plus
        let safe: String = pkg.chars().filter(|c| c.is_alphanumeric() || "-._+".contains(*c)).collect();
        if safe.is_empty() || safe != *pkg { continue; }
        script.push_str(&format!(
            r#"
# Check {safe}
if dpkg -l {safe} 2>/dev/null | grep -q '^ii' || rpm -q {safe} &>/dev/null; then
  echo "CUSTOM_{safe}_INSTALLED=1"
  _ver=$(dpkg -l {safe} 2>/dev/null | grep '^ii' | awk '{{print $3}}' | head -1 || rpm -q --qf '%{{VERSION}}' {safe} 2>/dev/null || echo '')
  echo "CUSTOM_{safe}_VERSION=$_ver"
else
  echo "CUSTOM_{safe}_INSTALLED=0"
fi
_svc=$(systemctl list-unit-files --type=service 2>/dev/null | grep -oP '^{safe}(?=[\d._-]*\.service)' | head -1 || echo '')
if [ -n "$_svc" ]; then
  echo "CUSTOM_{safe}_SERVICE=$_svc"
  echo "CUSTOM_{safe}_RUNNING=$(systemctl is-active $_svc 2>/dev/null || echo inactive)"
else
  echo "CUSTOM_{safe}_SERVICE="
  echo "CUSTOM_{safe}_RUNNING=inactive"
fi
"#,
            safe = safe
        ));
    }

    let (stdout, stderr, _) = crate::ssh::session_exec_with_output(session, &script, 15).await?;
    let combined = format!("{}{}", stdout, stderr);

    let get = |key: &str| -> String {
        combined.lines()
            .find(|l| l.starts_with(key))
            .map(|l| l.split('=').nth(1).unwrap_or("").trim().to_string())
            .unwrap_or_default()
    };

    let mut list = Vec::new();
    for pkg in packages {
        let safe: String = pkg.chars().filter(|c| c.is_alphanumeric() || "-._+".contains(*c)).collect();
        if safe.is_empty() || safe != *pkg { continue; }
        let prefix = format!("CUSTOM_{}_", safe);
        list.push(SoftwareInfo {
            name: pkg.clone(),
            display_name: pkg.clone(),
            category: "custom".to_string(),
            installed: get(&format!("{}INSTALLED", prefix)) == "1",
            version: get(&format!("{}VERSION", prefix)),
            service_name: get(&format!("{}SERVICE", prefix)),
            running: get(&format!("{}RUNNING", prefix)) == "active",
        });
    }
    Ok(list)
}

/// Install or uninstall a custom software package
pub async fn custom_software_action(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
    package_name: &str,
    action: &str,
    display_name: &str,
    app_handle: &AppHandle,
    timeout_secs: u64,
) -> Result<String, String> {
    // ponytail: sanitize package name — only allow safe chars
    let safe: String = package_name.chars().filter(|c| c.is_alphanumeric() || "-._+".contains(*c)).collect();
    if safe.is_empty() || safe != package_name {
        return Err("Invalid package name".to_string());
    }

    let script = format!(r#"#!/bin/bash
echo "=== {} {} ==="
if [ -f /etc/os-release ]; then
  . /etc/os-release
fi
if [ "{}" = "install" ]; then
  echo "Installing {}..."
  for i in $(seq 1 60); do
    if ! fuser /var/lib/dpkg/lock-frontend >/dev/null 2>&1 && ! fuser /var/lib/apt/lists/lock >/dev/null 2>&1 && ! fuser /var/cache/apt/archives/lock >/dev/null 2>&1 && ! fuser /var/run/yum.pid >/dev/null 2>&1 && ! fuser /var/run/dnf.pid >/dev/null 2>&1; then
      break
    fi
    echo "Waiting for package manager lock... ($i/60)"
    sleep 1
  done
  if [ "$ID" = "ubuntu" ] || [ "$ID" = "debian" ]; then
    apt-get update -qq --allow-releaseinfo-change || true
    apt-get install -y {} 2>&1
  else
    yum install -y --nogpgcheck --assumeyes {} 2>&1
  fi
  # Try to enable and start service if exists
  _svc=$(systemctl list-unit-files --type=service 2>/dev/null | grep -oP '^{}[\d._-]*\.service' | head -1 | sed 's/.service$//')
  if [ -n "$_svc" ]; then
    systemctl enable "$_svc" 2>/dev/null && systemctl start "$_svc" 2>/dev/null
    echo "Service $_svc enabled and started"
  fi
else
  echo "Removing {}..."
  _svc=$(systemctl list-unit-files --type=service 2>/dev/null | grep -oP '^{}[\d._-]*\.service' | head -1 | sed 's/.service$//')
  if [ -n "$_svc" ]; then
    systemctl stop "$_svc" 2>/dev/null || true
    systemctl disable "$_svc" 2>/dev/null || true
  fi
  if [ "$ID" = "ubuntu" ] || [ "$ID" = "debian" ]; then
    DEBIAN_FRONTEND=noninteractive apt-get purge -y {} 2>/dev/null || true
    apt-get autoremove -y 2>/dev/null || true
  else
    yum remove -y --assumeyes {} 2>/dev/null || true
  fi
fi
echo "ACTION_SUCCESS"
"#,
        action, safe, action, safe, safe, safe, safe, safe, safe, safe, safe
    );

    crate::ssh::session_write_file(session, "/tmp/software-action.sh", &script).await?;

    let event_name = "software-action-progress";
    let _ = app_handle.emit(event_name, serde_json::json!({
        "sessionId": session_id,
        "line": format!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"),
        "status": "running",
    }));
    let _ = app_handle.emit(event_name, serde_json::json!({
        "sessionId": session_id,
        "line": format!("Executing: bash /tmp/software-action.sh"),
        "status": "running",
    }));
    let _ = app_handle.emit(event_name, serde_json::json!({
        "sessionId": session_id,
        "line": format!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"),
        "status": "running",
    }));

    let mut channel = crate::ssh::session_open_channel(session).await?;
    // ponytail: redirect output to log file (not SSH channel) so install survives disconnect
    // write action info for recovery: "action:display_name"
    let info_cmd: String = format!("echo $$ > /tmp/leepanel-install.pid; echo '{}:{}' > /tmp/leepanel-install.info; > /tmp/leepanel-install.log; bash /tmp/software-action.sh >> /tmp/leepanel-install.log 2>&1; rm -f /tmp/leepanel-install.pid /tmp/leepanel-install.info", action, display_name);
    channel.exec(true, info_cmd).await
        .map_err(|e| format!("Failed to start script: {}", e))?;
    // ponytail: tail the log file for real-time output display
    let mut tail_channel = crate::ssh::session_open_channel(session).await?;
    let _ = tail_channel.exec(true, "tail -f /tmp/leepanel-install.log").await;
    let mut full_output = String::new();
    let mut exit_code: i32 = -1;
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(timeout_secs);
    loop {
        tokio::select! {
            msg = tail_channel.wait() => {
                match msg {
                    Some(russh::ChannelMsg::Data { data }) => {
                        let text = String::from_utf8_lossy(&data);
                        full_output.push_str(&text);
                        for line in text.lines() {
                            if !line.trim().is_empty() {
                                let _ = app_handle.emit(event_name, serde_json::json!({
                                    "sessionId": session_id,
                                    "line": line,
                                    "status": "running",
                                }));
                            }
                        }
                    }
                    Some(russh::ChannelMsg::ExtendedData { data, ext }) if ext == 1 => {
                        let text = String::from_utf8_lossy(&data);
                        full_output.push_str(&text);
                        for line in text.lines() {
                            if !line.trim().is_empty() {
                                let _ = app_handle.emit(event_name, serde_json::json!({
                                    "sessionId": session_id,
                                    "line": line,
                                    "status": "running",
                                }));
                            }
                        }
                    }
                    _ => {}
                }
            }
            msg = channel.wait() => {
                match msg {
                    Some(russh::ChannelMsg::ExitStatus { exit_status }) => {
                        exit_code = exit_status as i32;
                    }
                    Some(russh::ChannelMsg::Eof) | Some(russh::ChannelMsg::Close) => break,
                    None => break,
                    _ => {}
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                let _ = app_handle.emit(event_name, serde_json::json!({
                    "sessionId": session_id,
                    "line": format!("Operation timed out ({} minutes)", timeout_secs / 60),
                    "status": "error",
                }));
                break;
            }
        }
    }
    tail_channel.close().await.ok();
    channel.close().await.ok();
    // ponytail: read complete log file for final output
    if let Ok((final_log, _, _)) = crate::ssh::session_exec_with_output(session, "cat /tmp/leepanel-install.log 2>/dev/null || true", 10).await {
        if !final_log.is_empty() {
            full_output = final_log;
        }
    }
    crate::ssh::session_exec_with_output(session, "rm -f /tmp/leepanel-install.pid /tmp/leepanel-install.info", 5).await.ok();

    cache.invalidate(session_id, &["software_list", "service_statuses"]);

    if full_output.contains("ACTION_SUCCESS") {
        let _ = app_handle.emit(event_name, serde_json::json!({
            "sessionId": session_id,
            "line": "Done",
            "status": "done",
        }));
        Ok("OK".to_string())
    } else {
        let _ = app_handle.emit(event_name, serde_json::json!({
            "sessionId": session_id,
            "line": "Operation failed",
            "status": "error",
        }));
        Err(format!("Exit code: {}\n{}", exit_code, full_output))
    }
}

/// Query available PHP versions from system package manager
pub async fn get_available_php_versions(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
) -> Result<Vec<String>, String> {
    let cmd = r#"
if [ -f /etc/os-release ]; then
  . /etc/os-release
fi

if [ "$ID" = "ubuntu" ] || [ "$ID" = "debian" ]; then
  # Ubuntu/Debian: query apt-cache for php*-fpm packages
  apt-cache search --names-only '^php[0-9]+\.[0-9]+-fpm$' 2>/dev/null | \
    awk '{print $1}' | sed 's/^php//; s/-fpm$//' | sort -V | uniq
else
  # CentOS/RHEL: query yum/dnf for php*-fpm packages
  if command -v dnf &>/dev/null; then
    dnf list available 'php*-fpm' 2>/dev/null | grep -oP 'php\K[0-9.]+' | sort -V | uniq
  else
    yum list available 'php*-fpm' 2>/dev/null | grep -oP 'php\K[0-9.]+' | sort -V | uniq
  fi
fi
"#;

    let (stdout, _stderr, _exit_code) = crate::ssh::session_exec_with_output(session, cmd, 30).await?;

    let mut versions: Vec<String> = stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string())
        .collect();

    versions.sort();
    versions.dedup();

    Ok(versions)
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MysqlVariant {
    pub variant: String,
    pub version: String,
}

/// Get available MySQL/MariaDB variants from system repos
pub async fn get_available_mysql_versions(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
) -> Result<Vec<MysqlVariant>, String> {
    // ponytail: outputs "variant:version" lines (e.g. "mariadb:11.8.6")
    let cmd = r#"
if [ -f /etc/os-release ]; then
  . /etc/os-release
fi

if [ "$ID" = "ubuntu" ] || [ "$ID" = "debian" ]; then
  apt-get update -qq --allow-releaseinfo-change 2>/dev/null || true
  M_CAND=$(apt-cache policy mariadb-server 2>/dev/null | grep 'Candidate:' | awk '{print $2}')
  if [ -n "$M_CAND" ] && [ "$M_CAND" != "(none)" ]; then
    echo "mariadb:$M_CAND"
  fi
  MY_CAND=$(apt-cache policy mysql-server 2>/dev/null | grep 'Candidate:' | awk '{print $2}')
  if [ -n "$MY_CAND" ] && [ "$MY_CAND" != "(none)" ]; then
    echo "mysql:$MY_CAND"
  fi
else
  if command -v dnf &>/dev/null; then
    M_VER=$(dnf list available mariadb-server 2>/dev/null | grep mariadb | awk '{print $2}' | head -1)
    [ -n "$M_VER" ] && echo "mariadb:$M_VER"
    MY_VER=$(dnf list available mysql-server 2>/dev/null | grep mysql | awk '{print $2}' | head -1)
    [ -n "$MY_VER" ] && echo "mysql:$MY_VER"
  else
    M_VER=$(yum list available mariadb-server 2>/dev/null | grep mariadb | awk '{print $2}' | head -1)
    [ -n "$M_VER" ] && echo "mariadb:$M_VER"
    MY_VER=$(yum list available mysql-server 2>/dev/null | grep mysql | awk '{print $2}' | head -1)
    [ -n "$MY_VER" ] && echo "mysql:$MY_VER"
  fi
fi
"#;

    let (stdout, _stderr, _exit_code) = crate::ssh::session_exec_with_output(session, cmd, 60).await?;

    let versions: Vec<MysqlVariant> = stdout
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            let mut parts = l.splitn(2, ':');
            let variant = parts.next()?.to_string();
            let version = parts.next().unwrap_or("").to_string();
            if variant == "mariadb" || variant == "mysql" {
                Some(MysqlVariant { variant, version })
            } else {
                None
            }
        })
        .collect();

    Ok(versions)
}

/// Get list of removable package sources (third-party repos)
pub async fn get_removable_sources(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
) -> Result<Vec<String>, String> {
    let cmd = r#"
if [ -f /etc/os-release ]; then
  . /etc/os-release
fi

if [ "$ID" = "ubuntu" ] || [ "$ID" = "debian" ]; then
  # Ubuntu/Debian: list third-party sources in sources.list.d/
  ls /etc/apt/sources.list.d/*.list 2>/dev/null | xargs -n1 basename | sed 's/.list$//' | sort
else
  # CentOS/RHEL: list repo files (exclude system repos)
  ls /etc/yum.repos.d/*.repo 2>/dev/null | xargs -n1 basename | sed 's/.repo$//' | grep -vE '^epel$|^base$|^extras$|^updates$' | sort
fi
"#;

    let (stdout, _stderr, _exit_code) = crate::ssh::session_exec_with_output(session, cmd, 10).await?;
    
    let mut sources: Vec<String> = stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string())
        .collect();
    
    sources.sort();
    Ok(sources)
}

/// Remove specified package sources
pub async fn remove_sources(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    source_names: Vec<String>,
) -> Result<String, String> {
    if source_names.is_empty() {
        return Err("No sources selected for removal".to_string());
    }
    
    let cmd = format!(r#"
if [ -f /etc/os-release ]; then
  . /etc/os-release
fi

for src in {}; do
  if [ "$ID" = "ubuntu" ] || [ "$ID" = "debian" ]; then
    rm -f "/etc/apt/sources.list.d/${{src}}.list"
  else
    rm -f "/etc/yum.repos.d/${{src}}.repo"
  fi
done
echo "Sources removed successfully"
"#, source_names.iter().map(|s| format!("\"{}\"", s)).collect::<Vec<_>>().join(" "));

    let (_stdout, _stderr, _exit_code) = crate::ssh::session_exec_with_output(session, &cmd, 10).await?;
    Ok(format!("Removed {} source(s)", source_names.len()))
}

/// Clean and update package sources with streaming output
pub async fn clean_and_update_sources(
    session: &SshSession,
    _cache: &SshCache,
    session_id: &str,
    app_handle: &AppHandle,
) -> Result<String, String> {
    let cmd = r#"
if [ -f /etc/os-release ]; then
  . /etc/os-release
fi

echo "=== Cleaning package source cache ==="
if [ "$ID" = "ubuntu" ] || [ "$ID" = "debian" ]; then
  rm -rf /var/lib/apt/lists/*
  echo "Cache cleared"
  
  echo "=== Updating package sources ==="
  apt-get update --allow-releaseinfo-change 2>&1 | tee /tmp/apt-update.log
  
  if [ $? -eq 0 ]; then
    echo "ACTION_SUCCESS"
  else
    echo "ERROR: Failed to update package sources"
    exit 1
  fi
else
  yum clean all 2>&1 || dnf clean all 2>&1
  echo "Cache cleared"
  
  echo "=== Updating package sources ==="
  yum makecache 2>&1 || dnf makecache 2>&1 | tee /tmp/yum-makecache.log
  
  if [ $? -eq 0 ]; then
    echo "ACTION_SUCCESS"
  else
    echo "ERROR: Failed to update package sources"
    exit 1
  fi
fi
"#;

    // Write script to remote server
    crate::ssh::session_write_file(session, "/tmp/clean-sources.sh", cmd).await?;
    
    // Execute with streaming output
    let mut channel = crate::ssh::session_open_channel(session).await?;
    channel
        .exec(true, "bash /tmp/clean-sources.sh")
        .await
        .map_err(|e| format!("Failed to start script: {}", e))?;
    
    let event_name = "sources-action-progress";
    let mut full_output = String::new();
    let mut exit_code: i32 = -1;
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(60);
    
    loop {
        tokio::select! {
            msg = channel.wait() => {
                match msg {
                    Some(russh::ChannelMsg::Data { data }) | Some(russh::ChannelMsg::ExtendedData { data, .. }) => {
                        let text = String::from_utf8_lossy(&data);
                        for line in text.lines() {
                            if !line.trim().is_empty() {
                                full_output.push_str(line);
                                full_output.push('\n');
                                let _ = app_handle.emit(event_name, serde_json::json!({
                                    "sessionId": session_id,
                                    "line": line,
                                    "status": "running",
                                }));
                            }
                        }
                    }
                    Some(russh::ChannelMsg::ExitStatus { exit_status }) => {
                        exit_code = exit_status as i32;
                    }
                    Some(russh::ChannelMsg::Eof) | Some(russh::ChannelMsg::Close) | None => break,
                    _ => {}
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                return Err("Operation timed out (60 seconds)".to_string());
            }
        }
    }
    
    let success = full_output.contains("ACTION_SUCCESS");
    
    if exit_code == 0 || success {
        let _ = app_handle.emit(event_name, serde_json::json!({
            "sessionId": session_id,
            "line": "Package sources updated successfully!",
            "status": "done",
        }));
        Ok(full_output)
    } else {
        let _ = app_handle.emit(event_name, serde_json::json!({
            "sessionId": session_id,
            "line": "Failed to update package sources",
            "status": "error",
        }));
        Err(format!("Operation failed (exit code {}):\n{}", exit_code, full_output))
    }
}

/// Add a new package source
pub async fn add_source(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    name: &str,
    url: &str,
    gpg_key: Option<&str>,
) -> Result<String, String> {
    if name.is_empty() || url.is_empty() {
        return Err("Source name and URL are required".to_string());
    }

    // Validate name (alphanumeric, hyphen, underscore only)
    if !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        return Err("Source name can only contain letters, numbers, hyphens, and underscores".to_string());
    }

    let cmd = format!(r#"
if [ -f /etc/os-release ]; then
  . /etc/os-release
fi

echo "Adding package source: {}"

if [ "$ID" = "ubuntu" ] || [ "$ID" = "debian" ]; then
  # Debian/Ubuntu: Create .list file
  SOURCE_FILE="/etc/apt/sources.list.d/{}.list"
  
  if [ -f "$SOURCE_FILE" ]; then
    echo "ERROR: Source file already exists: $SOURCE_FILE"
    exit 1
  fi
  
  # Write the source line
  echo '{}' > "$SOURCE_FILE"
  
  # Add GPG key if provided
  if [ -n "{}" ]; then
    mkdir -p /etc/apt/keyrings
    curl -fsSL "{}" | gpg --dearmor -o /etc/apt/keyrings/{}.gpg 2>/dev/null || true
    chmod a+r /etc/apt/keyrings/{}.gpg 2>/dev/null || true
  fi
  
  echo "Source added successfully"
else
  # CentOS/RHEL: Create .repo file
  SOURCE_FILE="/etc/yum.repos.d/{}.repo"
  
  if [ -f "$SOURCE_FILE" ]; then
    echo "ERROR: Source file already exists: $SOURCE_FILE"
    exit 1
  fi
  
  # Write repo configuration
  cat > "$SOURCE_FILE" << 'EOF'
[{}]
name={}
baseurl={}
enabled=1
gpgcheck=0
EOF
  
  # Add GPG key if provided
  if [ -n "{}" ]; then
    sed -i "s/gpgcheck=0/gpgcheck=1\ngpgkey={}/" "$SOURCE_FILE"
  fi
  
  echo "Source added successfully"
fi
"#, name, name, url, gpg_key.unwrap_or(""), gpg_key.unwrap_or(""), name, name, name, name, name, name, url, gpg_key.unwrap_or(""));

    let (_stdout, _stderr, _exit_code) = crate::ssh::session_exec_with_output(session, &cmd, 15).await?;
    Ok(format!("Package source '{}' added successfully", name))
}

/// Install or uninstall software via SSH with real-time output
pub async fn software_action(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
    software: &str,
    action: &str,
    options: &str,
    display_name: &str,
    app_handle: &AppHandle,
    timeout_secs: u64,
) -> Result<String, String> {
    let os = detect_os(session, cache, session_id).await?;
    let is_debian = os.family == "debian";

    let script = build_software_script(&os, software, action, options, is_debian);

    crate::ssh::session_write_file(session, "/tmp/software-action.sh", &script)
        .await?;

    let event_name = "software-action-progress";
    
    // Log the command being executed
    let _ = app_handle.emit(event_name, serde_json::json!({
        "sessionId": session_id,
        "line": format!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"),
        "status": "running",
    }));
    let _ = app_handle.emit(event_name, serde_json::json!({
        "sessionId": session_id,
        "line": format!("Executing: bash /tmp/software-action.sh"),
        "status": "running",
    }));
    let _ = app_handle.emit(event_name, serde_json::json!({
        "sessionId": session_id,
        "line": format!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"),
        "status": "running",
    }));

    let mut channel = crate::ssh::session_open_channel(session).await?;
    // ponytail: redirect output to log file (not SSH channel) so install survives disconnect
    // write action info for recovery: "action:display_name"
    let info_cmd: String = format!("echo $$ > /tmp/leepanel-install.pid; echo '{}:{}' > /tmp/leepanel-install.info; > /tmp/leepanel-install.log; bash /tmp/software-action.sh >> /tmp/leepanel-install.log 2>&1; rm -f /tmp/leepanel-install.pid /tmp/leepanel-install.info", action, display_name);
    channel
        .exec(true, info_cmd)
        .await
        .map_err(|e| format!("Failed to start script: {}", e))?;
    // ponytail: tail the log file for real-time output display
    let mut tail_channel = crate::ssh::session_open_channel(session).await?;
    let _ = tail_channel.exec(true, "tail -f /tmp/leepanel-install.log").await;

    let mut full_output = String::new();
    let mut exit_code: i32 = -1;
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(timeout_secs);

    loop {
        tokio::select! {
            msg = tail_channel.wait() => {
                match msg {
                    Some(russh::ChannelMsg::Data { data }) => {
                        let text = String::from_utf8_lossy(&data);
                        full_output.push_str(&text);
                        for line in text.lines() {
                            if !line.trim().is_empty() {
                                let _ = app_handle.emit(event_name, serde_json::json!({
                                    "sessionId": session_id,
                                    "line": line,
                                    "status": "running",
                                }));
                            }
                        }
                    }
                    Some(russh::ChannelMsg::ExtendedData { data, ext }) if ext == 1 => {
                        let text = String::from_utf8_lossy(&data);
                        full_output.push_str(&text);
                        for line in text.lines() {
                            if !line.trim().is_empty() {
                                let _ = app_handle.emit(event_name, serde_json::json!({
                                    "sessionId": session_id,
                                    "line": line,
                                    "status": "running",
                                }));
                            }
                        }
                    }
                    _ => {}
                }
            }
            msg = channel.wait() => {
                match msg {
                    Some(russh::ChannelMsg::ExitStatus { exit_status }) => {
                        exit_code = exit_status as i32;
                    }
                    Some(russh::ChannelMsg::Eof) | Some(russh::ChannelMsg::Close) | None => break,
                    _ => {}
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                let _ = app_handle.emit(event_name, serde_json::json!({
                    "sessionId": session_id,
                    "line": format!("Operation timed out ({} minutes)", timeout_secs / 60),
                    "status": "error",
                }));
                break;
            }
        }
    }
    tail_channel.close().await.ok();
    channel.close().await.ok();
    // ponytail: read complete log file for final output
    if let Ok((final_log, _, _)) = crate::ssh::session_exec_with_output(session, "cat /tmp/leepanel-install.log 2>/dev/null || true", 10).await {
        if !final_log.is_empty() {
            full_output = final_log;
        }
    }
    crate::ssh::session_exec_with_output(session, "rm -f /tmp/leepanel-install.pid /tmp/leepanel-install.info", 5).await.ok();

    // ponytail: russh exit code unreliable, use output marker as fallback
    let success = full_output.contains("ACTION_SUCCESS");

    // Emit raw (unfiltered) terminal output for "view full output" collapsible section
    let _ = app_handle.emit("software-action-raw-output", serde_json::json!({
        "sessionId": session_id,
        "rawOutput": full_output,
    }));

    if exit_code == 0 || success {
        let _ = app_handle.emit(event_name, serde_json::json!({
            "sessionId": session_id,
            "line": format!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"),
            "status": "running",
        }));
        let _ = app_handle.emit(event_name, serde_json::json!({
            "sessionId": session_id,
            "line": format!("✅ {} {} completed successfully!", action, software),
            "status": "done",
        }));
        Ok(full_output)
    } else {
        // Extract key error lines for better visibility
        let error_lines: Vec<&str> = full_output
            .lines()
            .filter(|line| {
                let lower = line.to_lowercase();
                lower.contains("error") || 
                lower.contains("failed") ||
                lower.contains("fatal") ||
                line.starts_with("E:") ||
                line.contains("✗") ||
                line.contains("❌")
            })
            .collect();
        
        // Send error summary first
        let _ = app_handle.emit(event_name, serde_json::json!({
            "sessionId": session_id,
            "line": format!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"),
            "status": "running",
        }));
        
        if !error_lines.is_empty() {
            let _ = app_handle.emit(event_name, serde_json::json!({
                "sessionId": session_id,
                "line": format!("🔍 Key errors found ({}):", error_lines.len()),
                "status": "running",
            }));
            for err_line in &error_lines {
                let _ = app_handle.emit(event_name, serde_json::json!({
                    "sessionId": session_id,
                    "line": format!("   {}", err_line),
                    "status": "running",
                }));
            }
            let _ = app_handle.emit(event_name, serde_json::json!({
                "sessionId": session_id,
                "line": format!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"),
                "status": "running",
            }));
        }
        
        let _ = app_handle.emit(event_name, serde_json::json!({
            "sessionId": session_id,
            "line": format!("❌ {} {} failed (exit code {})", action, software, exit_code),
            "status": "error",
        }));
        
        Err(format!("Operation failed (exit code {}):\n{}", exit_code, full_output))
    }
}

/// Generate a PHP source compilation script (BT Panel style)
fn build_php_source_compile_script(php_ver: &str, action: &str) -> String {
    // ponytail: PHP source URLs — pinned to specific patch versions for reproducibility
    let source_url = match php_ver {
        "7.4" => "https://www.php.net/distributions/php-7.4.33.tar.gz",
        "8.0" => "https://www.php.net/distributions/php-8.0.30.tar.gz",
        "8.1" => "https://www.php.net/distributions/php-8.1.31.tar.gz",
        "8.2" => "https://www.php.net/distributions/php-8.2.27.tar.gz",
        "8.3" => "https://www.php.net/distributions/php-8.3.15.tar.gz",
        "8.4" => "https://www.php.net/distributions/php-8.4.2.tar.gz",
        _ => "https://www.php.net/distributions/php-8.3.15.tar.gz",
    };
    let full_ver = source_url.rsplit('/').next().unwrap().replace(".tar.gz", "").replace("php-", "");

    format!(r#"#!/bin/bash
# PHP source compile — dynamic detection (BT Panel style)
# no set -e: extension failures must not abort the install
PHP_VER="{php_ver}"
PHP_FULL="{full_ver}"
PHP_PREFIX="/www/server/php/$PHP_VER"
PHP_SRC_URL="{source_url}"
PHP_SRC_DIR="/tmp/php-build"
SKIPPED=""
MISSING=""

echo "=== {action} PHP $PHP_VER (source compile) ==="

# Detect OS
if [ -f /etc/os-release ]; then
  . /etc/os-release
fi
OS_ID="${{ID:-unknown}}"
OS_VER="${{VERSION_ID:-0}}"

install_deps() {{
  echo "Installing build dependencies..."
  if [ "$OS_ID" = "ubuntu" ] || [ "$OS_ID" = "debian" ]; then
    for i in $(seq 1 60); do
      if ! fuser /var/lib/dpkg/lock-frontend >/dev/null 2>&1 && ! fuser /var/lib/apt/lists/lock >/dev/null 2>&1 && ! fuser /var/cache/apt/archives/lock >/dev/null 2>&1; then break; fi
      echo "Waiting for package manager lock... ($i/60)"
      sleep 1
    done
    apt-get update -qq --allow-releaseinfo-change || true
    # Core build tools (always install)
    apt-get install -y build-essential autoconf pkg-config libtool re2c bison flex
    # Each lib individually — failures recorded but not fatal
    for pkg in libxml2-dev libssl-dev libcurl4-openssl-dev libsqlite3-dev \
               libpng-dev libjpeg-dev libfreetype6-dev libzip-dev \
               libonig-dev libsodium-dev libreadline-dev libxslt1-dev \
               zlib1g-dev libbz2-dev libicu-dev libgmp-dev \
               libwebp-dev libtidy-dev libmemcached-dev \
               libenchant-dev libavif-dev; do
      apt-get install -y "$pkg" 2>/dev/null || MISSING="$MISSING $pkg"
    done
  else
    # CentOS / RHEL / Alma / Rocky
    if command -v dnf >/dev/null 2>&1; then
      PM="dnf"
      # Enable CRB / PowerTools for devel packages
      dnf install -y dnf-plugins-core 2>/dev/null
      dnf config-manager --set-enabled crb 2>/dev/null \
        || dnf config-manager --set-enabled powertools 2>/dev/null \
        || dnf config-manager --set-enabled PowerTools 2>/dev/null || true
    else
      PM="yum"
    fi
    $PM install -y --nogpgcheck epel-release 2>/dev/null || true
    # Core build tools
    $PM install -y --nogpgcheck gcc gcc-c++ make autoconf automake libtool re2c bison flex pkgconf-pkg-config
    # Each lib individually — CentOS 9 compatible names
    for pkg in libxml2-devel openssl-devel libcurl-devel sqlite-devel \
               libpng-devel libjpeg-devel freetype-devel libzip-devel \
               oniguruma-devel libsodium-devel readline-devel libxslt-devel \
               zlib-devel bzip2-devel libicu-devel gmp-devel \
               libwebp-devel libtidy-devel enchant2-devel; do
      $PM install -y --nogpgcheck "$pkg" 2>/dev/null || MISSING="$MISSING $pkg"
    done
  fi
  [ -n "$MISSING" ] && echo "INFO: Some packages not available (non-fatal):$MISSING"
}}

# ponytail: detect lib availability — returns 0 if any marker found
check_lib() {{
  local name="$1"; shift
  for f in "$@"; do
    [ -f "$f" ] && return 0
  done
  pkg-config --exists "$name" 2>/dev/null && return 0
  return 1
}}

if [ "{action}" = "install" ]; then
  # Check if already installed
  if [ -x "$PHP_PREFIX/sbin/php-fpm" ]; then
    echo "PHP $PHP_VER is already installed at $PHP_PREFIX"
    echo "ACTION_SUCCESS"
    exit 0
  fi

  # Download source
  echo "Downloading PHP $PHP_FULL source..."
  mkdir -p "$PHP_SRC_DIR"
  cd "$PHP_SRC_DIR"
  if [ ! -f "php-$PHP_FULL.tar.gz" ]; then
    curl -fSL "$PHP_SRC_URL" -o "php-$PHP_FULL.tar.gz" || {{ echo "ERROR: download failed"; exit 1; }}
  fi
  tar xzf "php-$PHP_FULL.tar.gz"
  cd "php-$PHP_FULL"

  # Build configure flags dynamically based on detected libraries
  echo "Detecting available libraries..."
  install_deps

  CF="--prefix=$PHP_PREFIX"
  CF="$CF --with-config-file-path=$PHP_PREFIX/etc"
  CF="$CF --with-config-file-scan-dir=$PHP_PREFIX/etc/php.d"
  CF="$CF --enable-fpm --with-fpm-user=www --with-fpm-group=www"
  # Core extensions (always available, compiled into PHP)
  CF="$CF --with-mysqli --with-pdo-mysql --enable-opcache"
  CF="$CF --enable-bcmath --enable-calendar --enable-exif --enable-ftp"
  CF="$CF --enable-pcntl --enable-shmop --enable-soap"
  CF="$CF --enable-sysvmsg --enable-sysvsem --enable-sysvshm"
  CF="$CF --enable-sockets --with-gettext --with-mhash"
  # Optional: openssl
  check_lib openssl /usr/include/openssl/ssl.h /usr/local/include/openssl/ssl.h \
    && CF="$CF --with-openssl" || echo "SKIP: openssl"
  # Optional: curl
  check_lib libcurl /usr/include/curl/curl.h /usr/local/include/curl/curl.h \
    && CF="$CF --with-curl" || echo "SKIP: curl"
  # Optional: zlib
  check_lib zlib /usr/include/zlib.h /usr/local/include/zlib.h \
    && CF="$CF --with-zlib" || echo "SKIP: zlib"
  # Optional: bz2 — explicit path for CentOS 9
  check_lib bz2 /usr/include/bzlib.h /usr/local/include/bzlib.h \
    && CF="$CF --with-bz2=/usr" || echo "SKIP: bz2"
  # Optional: readline
  check_lib readline /usr/include/readline/readline.h /usr/local/include/readline/readline.h \
    && CF="$CF --with-readline" || echo "SKIP: readline"
  # Optional: mbstring (needs oniguruma)
  check_lib oniguruma /usr/include/oniguruma.h /usr/local/include/oniguruma.h \
    && CF="$CF --enable-mbstring" || echo "SKIP: mbstring"
  # Optional: zip
  check_lib libzip /usr/include/zip.h /usr/local/include/zip.h \
    && CF="$CF --with-zip" || echo "SKIP: zip"
  # Optional: intl (needs ICU)
  (check_lib icu-uc /usr/include/unicode/utypes.h /usr/local/include/unicode/utypes.h \
    || [ -f /usr/bin/icu-config ]) \
    && CF="$CF --enable-intl" || echo "SKIP: intl"
  # Optional: xsl
  check_lib libxslt /usr/include/libxslt/xslt.h /usr/local/include/libxslt/xslt.h \
    && CF="$CF --with-xsl" || echo "SKIP: xsl"
  # Optional: sodium
  check_lib libsodium /usr/include/sodium.h /usr/local/include/sodium.h \
    && CF="$CF --with-sodium" || echo "SKIP: sodium"
  # Optional: tidy
  check_lib tidy /usr/include/tidy.h /usr/include/tidy/tidybuffio.h /usr/local/include/tidy.h \
    && CF="$CF --with-tidy" || echo "SKIP: tidy"
  # Optional: enchant
  (check_lib enchant-2 /usr/include/enchant-2/enchant.h /usr/local/include/enchant-2/enchant.h \
    || check_lib enchant /usr/include/enchant.h /usr/local/include/enchant.h) \
    && CF="$CF --enable-enchant" || echo "SKIP: enchant"
  # Optional: avif (PHP 8.1+)
  if [ "$PHP_VER" != "7.4" ] && [ "$PHP_VER" != "8.0" ]; then
    check_lib libavif /usr/include/avif/avif.h /usr/local/include/avif/avif.h \
      && CF="$CF --with-avif" || echo "SKIP: avif"
  fi
  # Optional: gmp
  check_lib gmp /usr/include/gmp.h /usr/include/x86_64-linux-gnu/gmp.h /usr/local/include/gmp.h \
    && CF="$CF --with-gmp" || echo "SKIP: gmp"
  # GD + image libs — version-dependent flags
  if check_lib libpng /usr/include/png.h /usr/include/libpng16/png.h /usr/local/include/png.h; then
    if [ "$PHP_VER" = "7.4" ]; then
      CF="$CF --with-gd --enable-gd-native-ttf"
      [ -f /usr/include/jpeglib.h ] && CF="$CF --with-jpeg-dir=/usr"
      [ -f /usr/include/freetype2/freetype/freetype.h ] || [ -f /usr/include/freetype/freetype.h ] \
        && CF="$CF --with-freetype-dir=/usr"
      check_lib libwebp /usr/include/webp/encode.h /usr/local/include/webp/encode.h \
        && CF="$CF --with-webp-dir=/usr" || true
    else
      CF="$CF --enable-gd --with-jpeg --with-freetype"
      check_lib libwebp /usr/include/webp/encode.h /usr/local/include/webp/encode.h \
        && CF="$CF --with-webp" || echo "SKIP: webp"
    fi
  else
    echo "SKIP: gd"
  fi

  echo "Configuring PHP $PHP_FULL..."
  echo "Configure flags: $CF"
  eval ./configure $CF 2>&1 | tail -10
  if [ ${{PIPESTATUS[0]}} -ne 0 ]; then
    echo "ERROR: configure failed"
    exit 1
  fi

  # Compile & install
  echo "Compiling PHP $PHP_FULL (this may take 5-15 minutes)..."
  make -j$(nproc)
  if [ $? -ne 0 ]; then
    echo "ERROR: make failed"
    exit 1
  fi
  make install
  if [ $? -ne 0 ]; then
    echo "ERROR: make install failed"
    exit 1
  fi

  # Create user/group if not exists
  id -u www &>/dev/null || useradd -r -s /sbin/nologin www

  # Setup config files
  echo "Setting up configuration..."
  mkdir -p "$PHP_PREFIX/etc/php.d"
  cp php.ini-production "$PHP_PREFIX/etc/php.ini"
  cp "$PHP_PREFIX/etc/php-fpm.conf.default" "$PHP_PREFIX/etc/php-fpm.conf"
  mkdir -p "$PHP_PREFIX/etc/php-fpm.d"
  cp sapi/fpm/php-fpm.conf "$PHP_PREFIX/etc/php-fpm.conf" 2>/dev/null || true
  cat > "$PHP_PREFIX/etc/php-fpm.d/www.conf" << 'WCONF'
[www]
user = www
group = www
listen = /tmp/php-cgi-VER.sock
listen.owner = www
listen.group = www
pm = dynamic
pm.max_children = 50
pm.start_servers = 5
pm.min_spare_servers = 2
pm.max_spare_servers = 10
pm.max_requests = 500
request_terminate_timeout = 300
WCONF
  sed -i "s/VER.sock/$PHP_VER.sock/g" "$PHP_PREFIX/etc/php-fpm.d/www.conf"

  # Create systemd service
  SVC_NAME="php$(echo $PHP_VER | tr -d '.')-fpm"
  cat > "/etc/systemd/system/$SVC_NAME.service" << SVCEOF
[Unit]
Description=PHP $PHP_VER FPM (source compile)
After=network.target

[Service]
Type=simple
ExecStart=$PHP_PREFIX/sbin/php-fpm --nodaemonize --fpm-config $PHP_PREFIX/etc/php-fpm.conf
ExecReload=/bin/kill -USR2 $MAINPID
Restart=on-failure
RestartSec=5s

[Install]
WantedBy=multi-user.target
SVCEOF

  systemctl daemon-reload
  systemctl enable "$SVC_NAME"
  systemctl start "$SVC_NAME"
  echo "PHP $PHP_VER installed to $PHP_PREFIX"

  # Install PECL extensions (each one independent, failures are non-fatal)
  echo "Installing PECL extensions..."
  export PATH="$PHP_PREFIX/bin:$PHP_PREFIX/sbin:$PATH"
  "$PHP_PREFIX/bin/pecl" channel-update pecl.php.net 2>/dev/null || true

  for ext in redis imagick swoole memcached mongodb; do
    echo "--- Installing $ext ---"
    if yes | "$PHP_PREFIX/bin/pecl" install "$ext" 2>&1 | tail -5; then
      SO_FILE=$(find "$PHP_PREFIX/lib/php/extensions" -name "${{ext}}.so" 2>/dev/null | head -1)
      if [ -n "$SO_FILE" ]; then
        echo "extension=$SO_FILE" >> "$PHP_PREFIX/etc/php.ini"
        echo "OK: $ext installed"
      else
        echo "SKIP: $ext .so not found after install"
        SKIPPED="$SKIPPED $ext"
      fi
    else
      echo "SKIP: $ext install failed"
      SKIPPED="$SKIPPED $ext"
    fi
  done

  # Enable opcache in php.ini
  grep -q 'zend_extension.*opcache' "$PHP_PREFIX/etc/php.ini" 2>/dev/null || {{
    OP_SO=$(find "$PHP_PREFIX/lib/php/extensions" -name "opcache.so" 2>/dev/null | head -1)
    [ -n "$OP_SO" ] && echo "zend_extension=$OP_SO" >> "$PHP_PREFIX/etc/php.ini"
  }}

  # Restart to load extensions
  systemctl restart "$SVC_NAME"

  [ -n "$SKIPPED" ] && echo "WARNING: skipped extensions:$SKIPPED"
  echo "PHP $PHP_VER source compile complete"
else
  # Uninstall
  echo "Removing PHP $PHP_VER (source compile)..."
  SVC_NAME="php$(echo $PHP_VER | tr -d '.')-fpm"
  systemctl stop "$SVC_NAME" 2>/dev/null || true
  systemctl disable "$SVC_NAME" 2>/dev/null || true
  rm -f "/etc/systemd/system/$SVC_NAME.service"
  systemctl daemon-reload
  rm -rf "$PHP_PREFIX"
  rm -rf /tmp/php-build
  echo "PHP $PHP_VER removed"
fi
echo "ACTION_SUCCESS"
"#, php_ver = php_ver, full_ver = full_ver, source_url = source_url, action = action)
}

fn build_software_script(
    _os: &OsInfo,
    software: &str,
    action: &str,
    options: &str,
    is_debian: bool,
) -> String {
    let (pkg_mgr, pkg_install, pkg_remove) = if is_debian {
        ("apt-get", "apt-get install -y", "apt-get purge -y")
    } else {
        ("yum", "yum install -y --nogpgcheck --assumeyes", "yum remove -y --assumeyes")
    };

    let (packages, service_name, post_install, post_remove) = match software {
        "redis" => {
            // ponytail: version selection removed — system package manager picks the version
            return format!(r#"#!/bin/bash
echo "=== {} Redis ==="
if [ -f /etc/os-release ]; then
  . /etc/os-release
fi
if [ "{}" = "install" ]; then
  if command -v redis-server &>/dev/null; then
    echo "Redis is already installed: $(redis-server --version 2>/dev/null | head -1)"
    echo "ACTION_SUCCESS"
    exit 0
  fi
  echo "Installing Redis..."
  if [ "$ID" = "ubuntu" ] || [ "$ID" = "debian" ]; then
    # Wait for apt lock
    for i in $(seq 1 60); do
      if ! fuser /var/lib/dpkg/lock-frontend >/dev/null 2>&1 && ! fuser /var/lib/apt/lists/lock >/dev/null 2>&1 && ! fuser /var/cache/apt/archives/lock >/dev/null 2>&1; then
        break
      fi
      echo "Waiting for package manager lock... ($i/60)"
      sleep 1
    done
    apt-get update -qq --allow-releaseinfo-change || true
    apt-get install -y redis-server
  else
    yum install -y --nogpgcheck --assumeyes epel-release
    yum install -y --nogpgcheck --assumeyes redis
  fi
  systemctl enable redis-server && systemctl start redis-server 2>/dev/null || systemctl enable redis && systemctl start redis
else
  echo "Removing Redis..."
  systemctl stop redis-server 2>/dev/null || systemctl stop redis 2>/dev/null || true
  systemctl disable redis-server 2>/dev/null || systemctl disable redis 2>/dev/null || true
  if [ "$ID" = "ubuntu" ] || [ "$ID" = "debian" ]; then
    apt-get purge -y redis-server 2>/dev/null || true
    apt-get autoremove -y 2>/dev/null || true
  else
    yum remove -y redis 2>/dev/null || true
  fi
fi
echo "ACTION_SUCCESS"
"#, action, action);
        }
        "memcached" => (
            "memcached",
            "memcached",
            "systemctl enable memcached && systemctl start memcached",
            "systemctl stop memcached 2>/dev/null; systemctl disable memcached 2>/dev/null",
        ),
        "nodejs" => {
            // ponytail: version selection removed — system package manager picks the version
            return format!(r#"#!/bin/bash
echo "=== {} Node.js ==="
if [ "{}" = "install" ]; then
  if command -v node &>/dev/null; then
    echo "Node.js is already installed: $(node -v 2>/dev/null)"
    echo "ACTION_SUCCESS"
    exit 0
  fi
  echo "Installing Node.js..."
  if [ -f /etc/os-release ]; then
    . /etc/os-release
    if [ "$ID" = "ubuntu" ] || [ "$ID" = "debian" ]; then
      # Wait for apt lock
      for i in $(seq 1 60); do
        if ! fuser /var/lib/dpkg/lock-frontend >/dev/null 2>&1 && ! fuser /var/lib/apt/lists/lock >/dev/null 2>&1 && ! fuser /var/cache/apt/archives/lock >/dev/null 2>&1; then
          break
        fi
        echo "Waiting for package manager lock... ($i/60)"
        sleep 1
      done
      apt-get update -qq --allow-releaseinfo-change || true
      apt-get install -y nodejs npm
    else
      yum install -y --nogpgcheck --assumeyes nodejs npm
    fi
  fi
else
  echo "Removing Node.js..."
  if [ -f /etc/os-release ]; then
    . /etc/os-release
    if [ "$ID" = "ubuntu" ] || [ "$ID" = "debian" ]; then
      apt-get purge -y nodejs npm 2>/dev/null || true
      apt-get autoremove -y 2>/dev/null || true
    else
      yum remove -y nodejs npm 2>/dev/null || true
    fi
  fi
fi
echo "ACTION_SUCCESS"
"#, action, action);
        }
        "docker" => {
            // ponytail: dpkg lock wait — must run before get-docker.sh calls apt-get internally
            let lock_wait = r#"for _i in $(seq 1 60); do
    if ! fuser /var/lib/dpkg/lock-frontend >/dev/null 2>&1 && ! fuser /var/lib/apt/lists/lock >/dev/null 2>&1 && ! fuser /var/cache/apt/archives/lock >/dev/null 2>&1; then break; fi
    echo "Waiting for package manager lock... ($_i/60)"
    sleep 1
  done"#;
            let install_cmd = if options == "aliyun" {
                // ponytail: bypass get.docker.com (blocked by GFW) — use Aliyun Docker CE repo directly
                format!(r#"{} && . /etc/os-release
  if [ "$ID" = "ubuntu" ] || [ "$ID" = "debian" ]; then
    apt-get update -qq --allow-releaseinfo-change || true; apt-get install -y ca-certificates curl gnupg
    install -m 0755 -d /etc/apt/keyrings
    curl -fsSL https://mirrors.aliyun.com/docker-ce/linux/$ID/gpg | gpg --dearmor -o /etc/apt/keyrings/docker.gpg
    chmod a+r /etc/apt/keyrings/docker.gpg
    echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] https://mirrors.aliyun.com/docker-ce/linux/$ID $(lsb_release -cs) stable" > /etc/apt/sources.list.d/docker.list
    apt-get update -qq --allow-releaseinfo-change || true
    apt-get install -y docker-ce docker-ce-cli containerd.io docker-compose-plugin
  else
    yum install -y --nogpgcheck --assumeyes yum-utils
    yum-config-manager --add-repo https://mirrors.aliyun.com/docker-ce/linux/centos/docker-ce.repo
    sed -i 's+download.docker.com+mirrors.aliyun.com/docker-ce+' /etc/yum.repos.d/docker-ce.repo
    yum install -y --nogpgcheck --assumeyes docker-ce docker-ce-cli containerd.io docker-compose-plugin
  fi"#, lock_wait)
            } else {
                // Direct pipe for official source
                format!("{} && curl -fsSL https://get.docker.com | sh", lock_wait)
            };
            return format!(r#"#!/bin/bash
echo "=== {} Docker ==="
if [ "{}" = "install" ]; then
  if command -v docker &>/dev/null; then
    echo "Docker already installed: $(docker -v)"
    echo "ACTION_SUCCESS"
    exit 0
  fi
  echo "Installing Docker..."
  {}
  systemctl enable docker && systemctl start docker
  usermod -aG docker $(whoami) 2>/dev/null || true
else
  echo "Removing Docker..."
  systemctl stop docker 2>/dev/null || true
  systemctl disable docker 2>/dev/null || true
  if [ -f /etc/os-release ]; then
    . /etc/os-release
    if [ "$ID" = "ubuntu" ] || [ "$ID" = "debian" ]; then
      apt-get purge -y docker-ce docker-ce-cli containerd.io docker-compose-plugin 2>/dev/null || true
      apt-get autoremove -y 2>/dev/null || true
    else
      yum remove -y docker-ce docker-ce-cli containerd.io docker-compose-plugin 2>/dev/null || true
    fi
  fi
  rm -rf /var/lib/docker 2>/dev/null || true
  rm -f /etc/apt/sources.list.d/docker.list /etc/apt/keyrings/docker.gpg /etc/yum.repos.d/docker-ce.repo 2>/dev/null || true
fi
echo "ACTION_SUCCESS"
"#, action, action, install_cmd);
        }
        "zip" => (
            "zip",
            "zip",
            "",
            "",
        ),
        "unzip" => (
            "unzip",
            "unzip",
            "",
            "",
        ),
        "nginx" => (
            "nginx",
            "nginx",
            "systemctl enable nginx && systemctl start nginx",
            "systemctl stop nginx 2>/dev/null; systemctl disable nginx 2>/dev/null",
        ),
        "mysql" => {
            // ponytail: options = "mariadb" or "mysql" to force variant; empty = auto-detect
            let script = r#"#!/bin/bash
set -e
echo "=== __ACTION__ MySQL/MariaDB ==="
if [ -f /etc/os-release ]; then
  . /etc/os-release
fi

if [ "__ACTION__" = "install" ]; then
  VARIANT="__OPTIONS__"
  
  if [ "$ID" = "ubuntu" ] || [ "$ID" = "debian" ]; then
    # Wait for apt lock
    for i in $(seq 1 60); do
      if ! fuser /var/lib/dpkg/lock-frontend >/dev/null 2>&1 && ! fuser /var/lib/apt/lists/lock >/dev/null 2>&1 && ! fuser /var/cache/apt/archives/lock >/dev/null 2>&1; then
        break
      fi
      echo "Waiting for package manager lock... ($i/60)"
      sleep 1
    done
    apt-get update -qq --allow-releaseinfo-change || true
    
    # Auto-detect variant if not specified
    if [ -z "$VARIANT" ]; then
      M_CAND=$(apt-cache policy mariadb-server 2>/dev/null | grep 'Candidate:' | awk '{print $2}')
      MY_CAND=$(apt-cache policy mysql-server 2>/dev/null | grep 'Candidate:' | awk '{print $2}')
      if [ -n "$M_CAND" ] && [ "$M_CAND" != "(none)" ]; then
        VARIANT="mariadb"
      elif [ -n "$MY_CAND" ] && [ "$MY_CAND" != "(none)" ]; then
        VARIANT="mysql"
      else
        err "No MySQL or MariaDB package available in system repos"
      fi
    fi
    
    if [ "$VARIANT" = "mysql" ]; then
      apt-get install -y debconf-utils
      echo "mysql-server mysql-server/root_password password " | debconf-set-selections
      echo "mysql-server mysql-server/root_password_again password " | debconf-set-selections
      DEBIAN_FRONTEND=noninteractive apt-get install -y mysql-server
      SVC_NAME="mysql"
    else
      apt-get install -y mariadb-server
      SVC_NAME="mariadb"
    fi
  else
    # Auto-detect variant if not specified
    if [ -z "$VARIANT" ]; then
      if yum list available mariadb-server 2>/dev/null | grep -q mariadb; then
        VARIANT="mariadb"
      elif yum list available mysql-server 2>/dev/null | grep -q mysql; then
        VARIANT="mysql"
      else
        err "No MySQL or MariaDB package available in system repos"
      fi
    fi
    
    if [ "$VARIANT" = "mysql" ]; then
      yum install -y --nogpgcheck --assumeyes mysql-server
      SVC_NAME="mysqld"
    else
      yum install -y --nogpgcheck --assumeyes mariadb-server
      SVC_NAME="mariadb"
    fi
  fi
  
  echo "Installed variant: $VARIANT (service: $SVC_NAME)"
  
  systemctl enable $SVC_NAME
  systemctl start $SVC_NAME
  
  echo "Waiting for database to be ready..."
  for i in $(seq 1 30); do
    if mysqladmin ping 2>/dev/null | grep -q alive; then
      echo "Database is ready"
      break
    fi
    sleep 1
  done
  
  echo "Generating random root password..."
  # ponytail: alphanumeric only — avoids shell/SQL escaping issues with special chars
  ROOT_PASS=$(tr -dc 'A-Za-z0-9' < /dev/urandom | head -c 20)
  
  # ponytail: single mysql invocation handles all setup; works on fresh install (unix_socket auth)
  # Uses generic IDENTIFIED BY which is compatible with both MariaDB and MySQL
  mysql -u root <<SETUP_EOF
ALTER USER 'root'@'localhost' IDENTIFIED BY '${ROOT_PASS}';
DELETE FROM mysql.user WHERE User='';
DELETE FROM mysql.user WHERE User='root' AND Host NOT IN ('localhost','127.0.0.1','::1');
FLUSH PRIVILEGES;
SETUP_EOF
  
  echo "$ROOT_PASS" > /tmp/mysql_root_password.txt
  chmod 600 /tmp/mysql_root_password.txt
  printf '[client]\nuser=root\npassword=%s\n' "$ROOT_PASS" > /root/.my.cnf
  chmod 600 /root/.my.cnf
  echo "========================================="
  echo "Root password: $ROOT_PASS"
  echo "Saved to /tmp/mysql_root_password.txt and /root/.my.cnf"
  echo "========================================="
else
  echo "Removing MySQL/MariaDB..."
  if [ -f /etc/os-release ]; then
    . /etc/os-release
  fi
  if [ "$ID" = "ubuntu" ] || [ "$ID" = "debian" ]; then
    SVC=$(systemctl list-unit-files | grep -E '^mysql|^mariadb' | awk '{print $1}' | head -1)
    systemctl stop $SVC 2>/dev/null || true
    systemctl disable $SVC 2>/dev/null || true
    apt-get install -y debconf-utils 2>/dev/null || true
    DEBIAN_FRONTEND=noninteractive apt-get purge -y mysql-server mysql-client mysql-common mariadb-server 2>/dev/null || true
    DEBIAN_FRONTEND=noninteractive apt-get autoremove -y 2>/dev/null || true
    rm -rf /var/lib/mysql /etc/mysql 2>/dev/null || true
  else
    SVC=$(systemctl list-unit-files | grep -E '^mysql|^mariadb' | awk '{print $1}' | head -1)
    systemctl stop $SVC 2>/dev/null || true
    systemctl disable $SVC 2>/dev/null || true
    yum remove -y mysql-server mariadb-server 2>/dev/null || true
    rm -rf /var/lib/mysql /etc/my.cnf 2>/dev/null || true
  fi
fi
echo "ACTION_SUCCESS"
"#;
            return script.replace("__ACTION__", action).replace("__OPTIONS__", options);
        }
        "php" => {
            // ponytail: source compile mode — options = "source:X.Y" e.g. "source:8.3"
            if let Some(php_ver) = options.strip_prefix("source:") {
                return build_php_source_compile_script(php_ver, action);
            }
            // ponytail: generic PHP — options contains version (e.g. "8.2"), empty means default
            let version = if options.is_empty() {
                "".to_string()
            } else {
                options.to_string()
            };

            let script = r#"#!/bin/bash
# ponytail: no set -e — optional extensions may be missing from repos
echo "=== __ACTION__ PHP __VERSION__ ==="
if [ -f /etc/os-release ]; then
  . /etc/os-release
fi
SKIPPED=""
if [ "__ACTION__" = "install" ]; then
  echo "Installing PHP..."
  if [ "$ID" = "ubuntu" ] || [ "$ID" = "debian" ]; then
    # Wait for apt lock
    for i in $(seq 1 60); do
      if ! fuser /var/lib/dpkg/lock-frontend >/dev/null 2>&1 && ! fuser /var/lib/apt/lists/lock >/dev/null 2>&1 && ! fuser /var/cache/apt/archives/lock >/dev/null 2>&1; then
        break
      fi
      echo "Waiting for package manager lock... ($i/60)"
      sleep 1
    done
    apt-get update -qq
    if [ -n "__VERSION__" ]; then
      # Core package — must succeed
      apt-get install -y php__VERSION__-fpm || { echo "ERROR: php__VERSION__-fpm install failed"; exit 1; }
      # Optional extensions — skip if missing
      for pkg in php__VERSION__-mysql php__VERSION__-curl php__VERSION__-mbstring php__VERSION__-xml php__VERSION__-zip php__VERSION__-gd php__VERSION__-bcmath php__VERSION__-opcache; do
        apt-get install -y "$pkg" || { echo "SKIP: $pkg not available"; SKIPPED="$SKIPPED $pkg"; }
      done
    else
      apt-get install -y php-fpm || { echo "ERROR: php-fpm install failed"; exit 1; }
      for pkg in php-mysql php-curl php-mbstring php-xml php-zip php-gd php-bcmath php-opcache; do
        apt-get install -y "$pkg" || { echo "SKIP: $pkg not available"; SKIPPED="$SKIPPED $pkg"; }
      done
    fi
    # ponytail: use exact versioned service name when version is known — avoids picking older php from list-units
    if [ -n "__VERSION__" ]; then
      SVC="php__VERSION__-fpm"
    else
      SVC=$(systemctl list-units --type=service | grep -E 'php[0-9.]*-fpm' | awk '{print $1}' | head -1 | sed 's/.service//')
    fi
  else
    yum install -y --nogpgcheck --assumeyes epel-release 2>/dev/null || true
    if [ -n "__VERSION__" ]; then
      VER_NODOT=$(echo "__VERSION__" | tr -d '.')
      yum install -y --nogpgcheck --assumeyes php${VER_NODOT}-php-fpm || { echo "ERROR: php${VER_NODOT}-php-fpm install failed"; exit 1; }
      for pkg in php${VER_NODOT}-php-mysqlnd php${VER_NODOT}-php-curl php${VER_NODOT}-php-mbstring php${VER_NODOT}-php-xml php${VER_NODOT}-php-zip php${VER_NODOT}-php-gd php${VER_NODOT}-php-bcmath php${VER_NODOT}-php-opcache; do
        yum install -y --nogpgcheck --assumeyes "$pkg" || { echo "SKIP: $pkg not available"; SKIPPED="$SKIPPED $pkg"; }
      done
    else
      yum install -y --nogpgcheck --assumeyes php-fpm || { echo "ERROR: php-fpm install failed"; exit 1; }
      for pkg in php-mysqlnd php-curl php-mbstring php-xml php-zip php-gd php-bcmath php-opcache; do
        yum install -y --nogpgcheck --assumeyes "$pkg" || { echo "SKIP: $pkg not available"; SKIPPED="$SKIPPED $pkg"; }
      done
    fi
    # ponytail: use exact versioned service name when version is known
    if [ -n "__VERSION__" ]; then
      SVC="php${VER_NODOT}-php-fpm"
    else
      SVC=$(systemctl list-units --type=service | grep -E 'php' | awk '{print $1}' | head -1 | sed 's/.service//')
    fi
  fi
  if [ -n "$SVC" ]; then
    systemctl enable "$SVC" && systemctl start "$SVC"
  fi
  [ -n "$SKIPPED" ] && echo "WARNING: skipped packages (not in repo):$SKIPPED"
else
  echo "Removing PHP..."
  if [ "$ID" = "ubuntu" ] || [ "$ID" = "debian" ]; then
    systemctl list-units --type=service | grep -E 'php[0-9.]*-fpm' | awk '{print $1}' | xargs -r systemctl stop 2>/dev/null || true
    systemctl list-units --type=service | grep -E 'php[0-9.]*-fpm' | awk '{print $1}' | xargs -r systemctl disable 2>/dev/null || true
    apt-get purge -y 'php*' 2>/dev/null || true
    apt-get autoremove -y 2>/dev/null || true
  else
    systemctl list-units --type=service | grep -E 'php' | awk '{print $1}' | xargs -r systemctl stop 2>/dev/null || true
    systemctl list-units --type=service | grep -E 'php' | awk '{print $1}' | xargs -r systemctl disable 2>/dev/null || true
    yum remove -y 'php*' 2>/dev/null || true
  fi
fi
echo "ACTION_SUCCESS"
"#;
            return script
                .replace("__ACTION__", action)
                .replace("__VERSION__", &version);
        }
        _ if software.starts_with("php") => {
            // ponytail: dynamic PHP version — any phpX.Y name handled uniformly
            let php_ver = software.strip_prefix("php").unwrap_or("8.2");
            // ponytail: full extension list kept for uninstall purge; install uses per-package loop
            let extensions = format!(
                "php{}-fpm php{}-mysql php{}-curl php{}-mbstring php{}-xml php{}-zip php{}-gd php{}-bcmath php{}-opcache",
                php_ver, php_ver, php_ver, php_ver, php_ver, php_ver, php_ver, php_ver, php_ver
            );
            let svc_name = format!("php{}-fpm", php_ver);
            let script = "#!/bin/bash\n\
# ponytail: no set -e — optional extensions may be missing from repos\n\
SKIPPED=\"\"\n\
echo \"=== __ACTION__ PHP __VER__ ===\"\n\
if [ -f /etc/os-release ]; then\n\
  . /etc/os-release\n\
fi\n\
if [ \"__ACTION__\" = \"install\" ]; then\n\
  echo \"Installing PHP __VER__...\"\n\
  if [ \"$ID\" = \"ubuntu\" ] || [ \"$ID\" = \"debian\" ]; then\n\
    # Wait for apt lock\n\
    for i in $(seq 1 60); do\n\
      if ! fuser /var/lib/dpkg/lock-frontend >/dev/null 2>&1 && ! fuser /var/lib/apt/lists/lock >/dev/null 2>&1 && ! fuser /var/cache/apt/archives/lock >/dev/null 2>&1; then\n\
        break\n\
      fi\n\
      echo \"Waiting for package manager lock... ($i/60)\"\n\
      sleep 1\n\
    done\n\
    apt-get update -qq\n\
    apt-get install -y software-properties-common\n\
    add-apt-repository -y ppa:ondrej/php 2>/dev/null || true\n\
    apt-get update -qq\n\
    apt-get install -y php__VER__-fpm || { echo \"ERROR: php__VER__-fpm install failed\"; exit 1; }\n\
    for pkg in php__VER__-mysql php__VER__-curl php__VER__-mbstring php__VER__-xml php__VER__-zip php__VER__-gd php__VER__-bcmath php__VER__-opcache; do\n\
      apt-get install -y \"$pkg\" || { echo \"SKIP: $pkg not available\"; SKIPPED=\"$SKIPPED $pkg\"; }\n\
    done\n\
  else\n\
    yum install -y --nogpgcheck --assumeyes epel-release 2>/dev/null || true\n\
    yum install -y --nogpgcheck --assumeyes https://rpms.remirepo.net/enterprise/remi-release-$(rpm -E %{rhel}).rpm 2>/dev/null || true\n\
    yum module enable -y php:remi-__VER__ 2>/dev/null || true\n\
    yum install -y --nogpgcheck --assumeyes php__VER__-fpm || { echo \"ERROR: php__VER__-fpm install failed\"; exit 1; }\n\
    for pkg in php__VER__-mysqlnd php__VER__-curl php__VER__-mbstring php__VER__-xml php__VER__-zip php__VER__-gd php__VER__-bcmath php__VER__-opcache; do\n\
      yum install -y --nogpgcheck --assumeyes \"$pkg\" || { echo \"SKIP: $pkg not available\"; SKIPPED=\"$SKIPPED $pkg\"; }\n\
    done\n\
  fi\n\
  systemctl enable __SVC__ && systemctl start __SVC__\n\
  [ -n \"$SKIPPED\" ] && echo \"WARNING: skipped packages (not in repo):$SKIPPED\"\n\
else\n\
  echo \"Removing PHP __VER__...\"\n\
  systemctl stop __SVC__ 2>/dev/null || true\n\
  systemctl disable __SVC__ 2>/dev/null || true\n\
  if [ \"$ID\" = \"ubuntu\" ] || [ \"$ID\" = \"debian\" ]; then\n\
    apt-get purge -y __EXT__ 2>/dev/null || true\n\
  else\n\
    yum remove -y php__VER__-fpm php__VER__-mysqlnd php__VER__-curl php__VER__-mbstring php__VER__-xml php__VER__-zip php__VER__-gd php__VER__-bcmath php__VER__-opcache 2>/dev/null || true\n\
  fi\n\
fi\n\
echo \"ACTION_SUCCESS\"\n";
            return script
                .replace("__ACTION__", action)
                .replace("__VER__", php_ver)
                .replace("__EXT__", &extensions)
                .replace("__SVC__", &svc_name);
        }
        "apache" | "apache2.2" | "apache2.4" => {
            // ponytail: version selection removed — system package manager picks the version
            let svc_name = "apache2";
            let script = r#"#!/bin/bash
set -e
echo "=== __ACTION__ Apache ==="
if [ -f /etc/os-release ]; then
  . /etc/os-release
fi

if [ "__ACTION__" = "install" ]; then
  echo "Installing Apache..."
  if [ "$ID" = "ubuntu" ] || [ "$ID" = "debian" ]; then
    apt-get update -qq
    apt-get install -y apache2 apache2-utils
  else
    yum install -y httpd httpd-tools mod_ssl
  fi
  systemctl enable __SVC__ && systemctl start __SVC__
else
  echo "Removing Apache..."
  systemctl stop __SVC__ 2>/dev/null || true
  systemctl disable __SVC__ 2>/dev/null || true
  for alt_svc in apache httpd Baota-Apache; do
    systemctl stop "$alt_svc" 2>/dev/null || true
    systemctl disable "$alt_svc" 2>/dev/null || true
  done
  if [ "$ID" = "ubuntu" ] || [ "$ID" = "debian" ]; then
    DEBIAN_FRONTEND=noninteractive apt-get purge -y apache2 apache2-bin apache2-data apache2-utils libapache2-mod-* 2>/dev/null || true
    apt-get autoremove -y 2>/dev/null || true
  else
    yum remove -y httpd httpd-tools mod_ssl httpd-manual 2>/dev/null || true
    yum autoremove -y 2>/dev/null || true
  fi
  rm -rf /etc/apache2 2>/dev/null || true
  rm -rf /var/www/html 2>/dev/null || true
  rm -rf /etc/httpd 2>/dev/null || true
fi
echo "ACTION_SUCCESS"
"#;
            return script
                .replace("__ACTION__", action)
                .replace("__SVC__", svc_name);
        }
        "postgresql" => {
            return format!(r#"#!/bin/bash
echo "=== {} PostgreSQL ==="
if [ -f /etc/os-release ]; then
  . /etc/os-release
fi
if [ "{}" = "install" ]; then
  if command -v postgres &>/dev/null; then
    echo "PostgreSQL already installed: $(psql -V 2>/dev/null | head -1)"
    echo "ACTION_SUCCESS"
    exit 0
  fi
  echo "Installing PostgreSQL..."
  if [ "$ID" = "ubuntu" ] || [ "$ID" = "debian" ]; then
    apt-get update -qq
    apt-get install -y postgresql postgresql-contrib
  else
    yum install -y --nogpgcheck --assumeyes postgresql-server postgresql-contrib
    postgresql-setup --initdb 2>/dev/null || true
  fi
  systemctl enable postgresql && systemctl start postgresql
else
  echo "Removing PostgreSQL..."
  systemctl stop postgresql 2>/dev/null || true
  systemctl disable postgresql 2>/dev/null || true
  if [ "$ID" = "ubuntu" ] || [ "$ID" = "debian" ]; then
    echo "postgresql-* postgresql/remove_data_directory boolean true" | debconf-set-selections 2>/dev/null || true
    DEBIAN_FRONTEND=noninteractive apt-get purge -y postgresql postgresql-* 2>/dev/null || true
    apt-get autoremove -y 2>/dev/null || true
    rm -rf /var/lib/postgresql /etc/postgresql 2>/dev/null || true
  else
    yum remove -y postgresql-server postgresql-contrib postgresql 2>/dev/null || true
    rm -rf /var/lib/pgsql /etc/postgresql 2>/dev/null || true
  fi
fi
echo "ACTION_SUCCESS"
"#, action, action);
        }
        _ => return format!("echo 'Unknown software: {}'", software),
    };

    format!(r#"#!/bin/bash
echo "=== {} {} ==="
if [ "{}" = "install" ]; then
  if command -v {} &>/dev/null || dpkg -l {} 2>/dev/null | grep -q ^ii; then
    echo "{} is already installed"
  else
    echo "Installing {}..."
    # Wait for apt/yum/dnf/rpm lock to be released (max 60s)
    for i in $(seq 1 60); do
      if ! fuser /var/lib/dpkg/lock-frontend >/dev/null 2>&1 && ! fuser /var/lib/apt/lists/lock >/dev/null 2>&1 && ! fuser /var/cache/apt/archives/lock >/dev/null 2>&1 && ! fuser /var/run/yum.pid >/dev/null 2>&1 && ! fuser /var/run/dnf.pid >/dev/null 2>&1 && ! fuser /var/cache/yum >/dev/null 2>&1 && ! fuser /var/lib/rpm/.rpm.lock >/dev/null 2>&1; then
        break
      fi
      echo "Waiting for package manager lock... ($i/60)"
      sleep 1
    done
    {} update -qq 2>/dev/null || true
    {} {} 2>&1
    {}
  fi
else
  echo "Removing {}..."
  systemctl stop {} 2>/dev/null || true
  {} {} 2>&1
  {}
  # Clean up auto-installed dependencies
  {} autoremove -y 2>/dev/null || true
fi
echo "ACTION_SUCCESS"
"#,
        action, software, action,
        if software == "mysql" { "mysql" } else { software },
        packages,
        software,
        software,
        pkg_mgr,
        pkg_install, packages,
        if action == "install" { post_install } else { "" },
        software,
        service_name,
        pkg_remove, packages,
        post_remove,
        pkg_mgr,
    )
}

// ===== Server Settings =====

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SshKeyPair {
    pub private_key_pem: String,
    pub public_key_openssh: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SshAuthMode {
    pub password: bool,
    pub pubkey: bool,
}

/// Generate SSH key pair locally (no SSH connection needed)
pub fn generate_ssh_keypair(algorithm: &str) -> Result<SshKeyPair, String> {
    use russh_keys::PublicKeyBase64;

    let key_pair = match algorithm {
        "ed25519" => KeyPair::generate_ed25519()
            .ok_or_else(|| "Failed to generate Ed25519 key pair".to_string())?,
        "rsa" => KeyPair::generate_rsa(4096, SignatureHash::SHA2_256)
            .ok_or_else(|| "Failed to generate RSA key pair".to_string())?,
        _ => return Err(format!("Unsupported algorithm: {}. Use 'ed25519' or 'rsa'.", algorithm)),
    };

    let mut pem_buf = Vec::new();
    russh_keys::encode_pkcs8_pem(&key_pair, &mut pem_buf)
        .map_err(|e| format!("Failed to encode private key: {}", e))?;
    let private_key_pem = String::from_utf8(pem_buf)
        .map_err(|e| format!("Invalid UTF-8 in PEM output: {}", e))?;

    let public_key = key_pair.clone_public_key()
        .map_err(|e| format!("Failed to extract public key: {}", e))?;
    let public_key_openssh = format!("{} {}", public_key.name(), public_key.public_key_base64());

    Ok(SshKeyPair {
        private_key_pem,
        public_key_openssh,
    })
}

/// Reboot the server
pub async fn reboot_server(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    force: bool,
) -> Result<String, String> {
    let cmd = if force { "reboot -f" } else { "reboot" };
    let (_, stderr, code) = crate::ssh::session_exec_with_output(session, cmd, 10).await?;
    // reboot may kill SSH before returning exit code, so treat timeout/connection loss as success
    if code != 0 && !stderr.is_empty() && !stderr.contains("Connection") && !stderr.contains("closed") {
        return Err(format!("Reboot failed: {}", stderr.trim()));
    }
    Ok(format!("[{}] Server is rebooting. SSH connection will be disconnected.", cmd))
}

/// Get server boot time and uptime duration
pub async fn get_server_uptime(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
) -> Result<(String, String), String> {
    // Get boot time as ISO timestamp
    let (boot_stdout, _, boot_code) = crate::ssh::session_exec_with_output(session, "date -d \"$(uptime -s)\" +\"%Y-%m-%d %H:%M:%S\" 2>/dev/null || who -b 2>/dev/null | awk '{print $3, $4}' || echo 'unknown'", 5)
        .await?;
    let boot_time = boot_stdout.trim().to_string();
    if boot_code != 0 || boot_time.is_empty() || boot_time == "unknown" {
        return Err("Failed to get boot time".to_string());
    }

    // Get uptime duration using /proc/uptime (seconds)
    let (up_stdout, _, _) = crate::ssh::session_exec_with_output(session, "cat /proc/uptime 2>/dev/null | awk '{print int($1)}'", 5)
        .await?;
    let total_secs: u64 = up_stdout.trim().parse().unwrap_or(0);
    let days = total_secs / 86400;
    let hours = (total_secs % 86400) / 3600;
    let mins = (total_secs % 3600) / 60;
    let uptime_str = if days > 0 {
        format!("{}d {}h {}m", days, hours, mins)
    } else if hours > 0 {
        format!("{}h {}m", hours, mins)
    } else {
        format!("{}m", mins)
    };

    Ok((boot_time, uptime_str))
}

/// Change SSH user password
pub async fn change_ssh_password(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    username: &str,
    new_password: &str,
) -> Result<String, String> {
    // Escape single quotes in password to prevent shell injection
    let safe_password = new_password.replace('\'', "'\\''");
    let cmd = format!("echo '{}:{}' | chpasswd", username, safe_password);
    let (stdout, stderr, code) = crate::ssh::session_exec_with_output(session, &cmd, 15).await?;
    if code != 0 {
        return Err(format!("Failed to change password: {}", stderr.trim()));
    }
    let _ = stdout;
    Ok(format!("Password changed successfully for user '{}'.", username))
}

/// Deploy SSH public key to remote server's authorized_keys
pub async fn deploy_ssh_pubkey(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    pubkey: &str,
) -> Result<String, String> {
    // Escape any special characters in pubkey
    let safe_key = pubkey.replace('"', "\\\"");
    let cmd = format!(
        "mkdir -p ~/.ssh && chmod 700 ~/.ssh && echo \"{}\" >> ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys && echo KEY_DEPLOYED",
        safe_key
    );
    let (stdout, stderr, code) = crate::ssh::session_exec_with_output(session, &cmd, 15).await?;
    if code != 0 || !stdout.contains("KEY_DEPLOYED") {
        return Err(format!("Failed to deploy public key: {}", stderr.trim()));
    }
    Ok("Public key deployed successfully.".to_string())
}

/// Get SSH authentication mode from sshd_config
pub async fn get_ssh_auth_mode(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
) -> Result<SshAuthMode, String> {
    // ponytail: cache SSH auth mode for connection lifetime
    if let Some(cached) = cache.get(session_id, "ssh_auth_mode", 0) {
        if let Ok(mode) = serde_json::from_str::<SshAuthMode>(&cached) {
            return Ok(mode);
        }
    }
    // ponytail: use sshd -T to read effective config (handles Include / sshd_config.d/ overrides on Debian 13+)
    let cmd = r#"
sshd -T 2>/dev/null | grep -iE '^(passwordauthentication|pubkeyauthentication)\s' | head -2
echo "DONE"
"#;
    let (stdout, stderr, code) = crate::ssh::session_exec_with_output(session, cmd, 10).await?;
    if code != 0 && !stderr.is_empty() && !stdout.contains("DONE") {
        return Err(format!("Failed to read sshd config: {}", stderr.trim()));
    }

    let mut password = true; // default: enabled
    let mut pubkey = true;   // default: enabled

    for line in stdout.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();
        if lower.starts_with("passwordauthentication") {
            password = lower.contains("yes");
        } else if lower.starts_with("pubkeyauthentication") {
            pubkey = lower.contains("yes");
        }
    }

    let result = SshAuthMode { password, pubkey };
    // ponytail: cache SSH auth mode
    if let Ok(json) = serde_json::to_string(&result) {
        cache.put(session_id, "ssh_auth_mode", json);
    }
    Ok(result)
}

/// Set SSH authentication mode by modifying sshd_config and restarting sshd
pub async fn set_ssh_auth_mode(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    password_enabled: bool,
    pubkey_enabled: bool,
) -> Result<String, String> {
    let pw_val = if password_enabled { "yes" } else { "no" };
    let pk_val = if pubkey_enabled { "yes" } else { "no" };

    // ponytail: also remove overriding directives in sshd_config.d/ drop-in files
    // so the main sshd_config value actually takes effect (Debian 13+ uses Include)
    let cmd = format!(r#"
# Remove PasswordAuthentication overrides from drop-in configs
if [ -d /etc/ssh/sshd_config.d ]; then
  find /etc/ssh/sshd_config.d -name '*.conf' -exec sed -i '/^\s*PasswordAuthentication/d' {{}} +
fi

# Update or add PasswordAuthentication in main config
if grep -qE '^\s*PasswordAuthentication' /etc/ssh/sshd_config; then
  sed -i 's/^\s*PasswordAuthentication.*/PasswordAuthentication {}/' /etc/ssh/sshd_config
elif grep -qE '^\s*#\s*PasswordAuthentication' /etc/ssh/sshd_config; then
  sed -i 's/^\s*#\s*PasswordAuthentication.*/PasswordAuthentication {}/' /etc/ssh/sshd_config
else
  echo 'PasswordAuthentication {}' >> /etc/ssh/sshd_config
fi

# Remove PubkeyAuthentication overrides from drop-in configs
if [ -d /etc/ssh/sshd_config.d ]; then
  find /etc/ssh/sshd_config.d -name '*.conf' -exec sed -i '/^\s*PubkeyAuthentication/d' {{}} +
fi

# Update or add PubkeyAuthentication in main config
if grep -qE '^\s*PubkeyAuthentication' /etc/ssh/sshd_config; then
  sed -i 's/^\s*PubkeyAuthentication.*/PubkeyAuthentication {}/' /etc/ssh/sshd_config
elif grep -qE '^\s*#\s*PubkeyAuthentication' /etc/ssh/sshd_config; then
  sed -i 's/^\s*#\s*PubkeyAuthentication.*/PubkeyAuthentication {}/' /etc/ssh/sshd_config
else
  echo 'PubkeyAuthentication {}' >> /etc/ssh/sshd_config
fi

# Restart SSH service
systemctl restart sshd 2>/dev/null || systemctl restart ssh 2>/dev/null || service sshd restart 2>/dev/null
echo "MODE_UPDATED"
"#,
        pw_val, pw_val, pw_val, pk_val, pk_val, pk_val
    );

    let (stdout, stderr, code) = crate::ssh::session_exec_with_output(session, &cmd, 20).await?;
    if !stdout.contains("MODE_UPDATED") {
        return Err(format!("Failed to update SSH auth mode: {}", stderr.trim()));
    }
    let _ = code; // sshd restart may cause connection drop, so don't fail on non-zero
    Ok("SSH authentication mode updated successfully.".to_string())
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BbrStatus {
    pub enabled: bool,
    pub congestion_control: String,
    pub qdisc: String,
}

/// Get BBR congestion control status
pub async fn get_bbr_status(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
) -> Result<BbrStatus, String> {
    // ponytail: cache BBR status for connection lifetime
    if let Some(cached) = cache.get(session_id, "bbr_status", 0) {
        if let Ok(status) = serde_json::from_str::<BbrStatus>(&cached) {
            return Ok(status);
        }
    }
    let cmd = r#"
CC=$(sysctl -n net.ipv4.tcp_congestion_control 2>/dev/null || echo unknown)
QD=$(sysctl -n net.core.default_qdisc 2>/dev/null || echo unknown)
echo "CC=$CC"
echo "QD=$QD"
"#;
    let (stdout, stderr, code) = crate::ssh::session_exec_with_output(session, cmd, 10).await?;
    if code != 0 && stderr.contains("No such file") {
        return Ok(BbrStatus { enabled: false, congestion_control: "unknown".into(), qdisc: "unknown".into() });
    }
    let mut cc = "unknown".to_string();
    let mut qd = "unknown".to_string();
    for line in stdout.lines() {
        if let Some(v) = line.strip_prefix("CC=") { cc = v.trim().to_string(); }
        if let Some(v) = line.strip_prefix("QD=") { qd = v.trim().to_string(); }
    }
    let result = BbrStatus {
        enabled: cc == "bbr",
        congestion_control: cc,
        qdisc: qd,
    };
    // ponytail: cache BBR status
    if let Ok(json) = serde_json::to_string(&result) {
        cache.put(session_id, "bbr_status", json);
    }
    Ok(result)
}

/// Enable or disable BBR congestion control
pub async fn set_bbr_status(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    enable: bool,
) -> Result<String, String> {
    let cmd = if enable {
        r#"
# Check kernel version (BBR requires 4.9+)
KVER=$(uname -r | cut -d'-' -f1)
MAJOR=$(echo "$KVER" | cut -d'.' -f1)
MINOR=$(echo "$KVER" | cut -d'.' -f2)
if [ "$MAJOR" -lt 4 ] || ([ "$MAJOR" -eq 4 ] && [ "$MINOR" -lt 9 ]); then
  echo "BBR_ERROR: Kernel $KVER too old, BBR requires 4.9+"
  exit 1
fi

# Load BBR module
modprobe tcp_bbr 2>/dev/null || true

# Apply BBR settings
sysctl -w net.core.default_qdisc=fq
sysctl -w net.ipv4.tcp_congestion_control=bbr

# Persist to sysctl.conf
sed -i '/^net\.core\.default_qdisc/d' /etc/sysctl.conf 2>/dev/null
sed -i '/^net\.ipv4\.tcp_congestion_control/d' /etc/sysctl.conf 2>/dev/null
echo 'net.core.default_qdisc=fq' >> /etc/sysctl.conf
echo 'net.ipv4.tcp_congestion_control=bbr' >> /etc/sysctl.conf

echo "BBR_ENABLED"
"#
    } else {
        r#"
# Restore default congestion control
sysctl -w net.core.default_qdisc=fq_codel
sysctl -w net.ipv4.tcp_congestion_control=cubic

# Remove BBR from sysctl.conf
sed -i '/^net\.core\.default_qdisc=fq$/d' /etc/sysctl.conf 2>/dev/null
sed -i '/^net\.ipv4\.tcp_congestion_control=bbr$/d' /etc/sysctl.conf 2>/dev/null

echo "BBR_DISABLED"
"#
    };

    let (stdout, stderr, code) = crate::ssh::session_exec_with_output(session, cmd, 15).await?;
    if stdout.contains("BBR_ERROR") {
        let err_msg = stdout.lines()
            .find(|l| l.contains("BBR_ERROR"))
            .unwrap_or("Unknown error");
        return Err(err_msg.replace("BBR_ERROR: ", ""));
    }
    if code != 0 {
        return Err(format!("Failed to {} BBR: {}", if enable { "enable" } else { "disable" }, stderr.trim()));
    }
    if enable && !stdout.contains("BBR_ENABLED") {
        return Err("Failed to enable BBR".to_string());
    }
    if !enable && !stdout.contains("BBR_DISABLED") {
        return Err("Failed to disable BBR".to_string());
    }
    Ok(if enable { "BBR enabled successfully." } else { "BBR disabled successfully." }.to_string())
}

// ===== Site Logs =====

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SiteLogInfo {
    pub path: String,
    pub log_type: String,  // "access" or "error"
    pub size: u64,
}

/// Get available log files for a site
pub async fn get_site_logs(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    domain: &str,
) -> Result<Vec<SiteLogInfo>, String> {
    let safe_domain = domain.replace('\'', "'\\''");
    // ponytail: simple find+grep approach — scan all known log dirs for files containing domain name
    let cmd = format!(r#"
for dir in /var/log/nginx /www/wwwlogs; do
  [ -d "$dir" ] || continue
  find "$dir" -maxdepth 1 -type f -name "*{safe_domain}*" 2>/dev/null | while read -r f; do
    SIZE=$(stat -c%s "$f" 2>/dev/null || echo 0)
    case "$(basename "$f")" in
      *error*) echo "LOG|$f|error|$SIZE" ;;
      *)       echo "LOG|$f|access|$SIZE" ;;
    esac
  done
done

# Also extract log paths from nginx config (access_log / error_log directives)
for dir in /etc/nginx/sites-enabled /etc/nginx/conf.d /www/server/panel/vhost/nginx /www/server/nginx/conf/vhost; do
  [ -d "$dir" ] || continue
  for conf in "$dir"/*; do
    [ -f "$conf" ] || continue
    grep -q '{safe_domain}' "$conf" 2>/dev/null || continue
    sed -n 's/.*access_log[[:space:]][[:space:]]*\([^ ;]*\).*/\1/p' "$conf" 2>/dev/null | while read -r p; do
      [ -f "$p" ] && echo "LOG|$p|access|$(stat -c%s "$p" 2>/dev/null || echo 0)"
    done
    sed -n 's/.*error_log[[:space:]][[:space:]]*\([^ ;]*\).*/\1/p' "$conf" 2>/dev/null | while read -r p; do
      [ -f "$p" ] && echo "LOG|$p|error|$(stat -c%s "$p" 2>/dev/null || echo 0)"
    done
  done
done
echo "DONE"
"#, safe_domain = safe_domain);

    let (stdout, stderr, code) = crate::ssh::session_exec_with_output(session, &cmd, 15).await?;
    if code != 0 && !stdout.contains("DONE") {
        return Err(format!("Failed to get site logs: {}", stderr.trim()));
    }

    let mut logs = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("LOG|") {
            let parts: Vec<&str> = rest.splitn(3, '|').collect();
            if parts.len() != 3 { continue; }
            let path = parts[0].to_string();
            if !seen.insert(path.clone()) { continue; }
            let log_type = parts[1].to_string();
            let size = parts[2].parse::<u64>().unwrap_or(0);
            logs.push(SiteLogInfo { path, log_type, size });
        }
    }

    Ok(logs)
}

/// Read last N lines of a log file, optionally filtered by date range
pub async fn read_site_log(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    log_path: &str,
    lines: usize,
    date_from: Option<&str>,
    date_to: Option<&str>,
) -> Result<String, String> {
    let safe_path = log_path.replace('\'', "'\\''");
    
    // Check if file is gzip compressed (ends with .gz)
    let is_gzipped = log_path.to_lowercase().ends_with(".gz");
    
    // Choose the appropriate command based on compression
    // Use gunzip -c as it's more universally available than zcat
    let read_cmd = if is_gzipped {
        "gunzip -c"  // Decompress and output to stdout
    } else {
        "cat"        // Regular file read
    };
    
    let cmd = if date_from.is_some() || date_to.is_some() {
        // Use awk to filter by date range.
        // Converts nginx log date [DD/Mon/YYYY:HH:MM:SS +ZZZZ] → "YYYY-MM-DD HH:MM:SS"
        // then does string comparison (works because YYYY-MM-DD HH:MM:SS sorts chronologically).
        let from = date_from.unwrap_or("");
        let to = date_to.unwrap_or("");
        // Use [\\/] character class to match forward slash in awk regex
        format!(
            "{} '{}' | tail -n {} | awk 'BEGIN{{split(\"Jan Feb Mar Apr May Jun Jul Aug Sep Oct Nov Dec\",m,\" \");for(i=1;i<=12;i++)mn[m[i]]=sprintf(\"%02d\",i)}}{{if(match($0,/\\[([0-9]+)[\\/ ]([A-Za-z]+)[\\/ ]([0-9]+):([0-9:]+)/,a)){{d=sprintf(\"%s-%s-%s %s\",a[3],mn[a[2]],a[1],a[4]);if(\"{}\"==\"\"||d>=\"{}\"){{if(\"{}\"==\"\"||d<=\"{}\")print}}}}}}'",
            read_cmd, safe_path, lines.min(10000), from, from, to, to
        )
    } else {
        format!("{} '{}' | tail -n {}", read_cmd, safe_path, lines.min(10000))
    };
    let (stdout, stderr, code) = crate::ssh::session_exec_with_output(session, &cmd, 30).await?;
    if code != 0 {
        let err_msg = if !stderr.trim().is_empty() { stderr.trim() } else { stdout.trim() };
        return Err(format!("Failed to read log: {}", err_msg));
    }
    Ok(stdout)
}

// ===== Docker Management =====

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DockerStatus {
    pub installed: bool,
    pub version: String,
    pub compose_version: String,
    pub running: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DockerContainer {
    pub id: String,
    pub name: String,
    pub image: String,
    pub status: String,
    pub state: String,
    pub ports: String,
    pub created: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DockerImage {
    pub id: String,
    pub repository: String,
    pub tag: String,
    pub size: String,
    pub created: String,
}

/// Check Docker installation status
pub async fn check_docker(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
) -> Result<DockerStatus, String> {
    // ponytail: cache Docker status for connection lifetime
    if let Some(cached) = cache.get(session_id, "docker_status", 0) {
        if let Ok(status) = serde_json::from_str::<DockerStatus>(&cached) {
            return Ok(status);
        }
    }
    let (stdout, _, _) = crate::ssh::session_exec_with_output(session,
            r#"
if command -v docker &>/dev/null; then
    echo "INSTALLED=true"
    echo "VERSION=$(docker --version 2>/dev/null | grep -oP '[0-9]+\.[0-9]+\.[0-9]+' | head -1)"
    echo "RUNNING=$(docker info &>/dev/null && echo true || echo false)"
else
    echo "INSTALLED=false"
    echo "VERSION="
    echo "RUNNING=false"
fi

if command -v docker-compose &>/dev/null; then
    echo "COMPOSE=$(docker-compose --version 2>/dev/null | grep -oP '[0-9]+\.[0-9]+\.[0-9]+' | head -1)"
elif docker compose version &>/dev/null 2>&1; then
    echo "COMPOSE=$(docker compose version 2>/dev/null | grep -oP '[0-9]+\.[0-9]+\.[0-9]+' | head -1)"
else
    echo "COMPOSE="
fi
"#,
            15,
        )
        .await?;

    let mut status = DockerStatus {
        installed: false,
        version: String::new(),
        compose_version: String::new(),
        running: false,
    };

    for line in stdout.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("INSTALLED=") {
            status.installed = v == "true";
        } else if let Some(v) = line.strip_prefix("VERSION=") {
            status.version = v.to_string();
        } else if let Some(v) = line.strip_prefix("COMPOSE=") {
            status.compose_version = v.to_string();
        } else if let Some(v) = line.strip_prefix("RUNNING=") {
            status.running = v == "true";
        }
    }

    // ponytail: cache Docker status
    if let Ok(json) = serde_json::to_string(&status) {
        cache.put(session_id, "docker_status", json);
    }
    Ok(status)
}

/// Helper: run an SSH command with streaming output via Tauri events
async fn docker_stream_exec(
    session: &SshSession,
    _cache: &SshCache,
    session_id: &str,
    cmd: &str,
    timeout_secs: u64,
    app_handle: &AppHandle,
) -> Result<String, String> {
    let mut channel = crate::ssh::session_open_channel(session).await?;
    channel
        .exec(true, cmd)
        .await
        .map_err(|e| format!("Failed to execute command: {}", e))?;

    let mut full_output = String::new();
    let mut exit_code: i32 = -1;
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(timeout_secs);

    loop {
        tokio::select! {
            msg = channel.wait() => {
                match msg {
                    Some(russh::ChannelMsg::Data { data }) => {
                        let text = String::from_utf8_lossy(&data);
                        full_output.push_str(&text);
                        for line in text.lines() {
                            if !line.trim().is_empty() {
                                let _ = app_handle.emit("docker-action-progress", serde_json::json!({
                                    "sessionId": session_id,
                                    "line": line,
                                    "status": "running",
                                }));
                            }
                        }
                    }
                    Some(russh::ChannelMsg::ExtendedData { data, ext }) => {
                        if ext == 1 {
                            let text = String::from_utf8_lossy(&data);
                            full_output.push_str(&text);
                            for line in text.lines() {
                                if !line.trim().is_empty() {
                                    let _ = app_handle.emit("docker-action-progress", serde_json::json!({
                                        "sessionId": session_id,
                                        "line": line,
                                        "status": "running",
                                    }));
                                }
                            }
                        }
                    }
                    Some(russh::ChannelMsg::ExitStatus { exit_status }) => {
                        exit_code = exit_status as i32;
                    }
                    Some(russh::ChannelMsg::Eof) | Some(russh::ChannelMsg::Close) | None => break,
                    _ => {}
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                return Err(format!("Command timed out after {} seconds", timeout_secs));
            }
        }
    }

    if exit_code != 0 {
        // ponytail: russh may deliver ExitStatus after Eof/Close, so exit_code stays -1.
        // Only treat as failure if we actually got a non-zero exit code.
        if exit_code > 0 {
            return Err(full_output);
        }
    }

    Ok(full_output)
}

/// Generic helper: stream SSH command output via a custom event name
async fn stream_ssh_command(
    session: &SshSession,
    _cache: &SshCache,
    session_id: &str,
    cmd: &str,
    timeout_secs: u64,
    app_handle: &AppHandle,
    event_name: &str,
) -> Result<(String, i32), String> {
    let mut channel = crate::ssh::session_open_channel(session).await?;
    channel
        .exec(true, cmd)
        .await
        .map_err(|e| format!("Failed to execute command: {}", e))?;

    let mut full_output = String::new();
    let mut exit_code: i32 = -1;
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(timeout_secs);

    loop {
        tokio::select! {
            msg = channel.wait() => {
                match msg {
                    Some(russh::ChannelMsg::Data { data }) => {
                        let text = String::from_utf8_lossy(&data);
                        full_output.push_str(&text);
                        for line in text.lines() {
                            if !line.trim().is_empty() {
                                let _ = app_handle.emit(event_name, serde_json::json!({
                                    "sessionId": session_id,
                                    "line": line,
                                    "status": "running",
                                }));
                            }
                        }
                    }
                    Some(russh::ChannelMsg::ExtendedData { data, ext }) => {
                        if ext == 1 {
                            let text = String::from_utf8_lossy(&data);
                            full_output.push_str(&text);
                            for line in text.lines() {
                                if !line.trim().is_empty() {
                                    let _ = app_handle.emit(event_name, serde_json::json!({
                                        "sessionId": session_id,
                                        "line": line,
                                        "status": "running",
                                    }));
                                }
                            }
                        }
                    }
                    Some(russh::ChannelMsg::ExitStatus { exit_status }) => {
                        exit_code = exit_status as i32;
                    }
                    Some(russh::ChannelMsg::Eof) | Some(russh::ChannelMsg::Close) | None => break,
                    _ => {}
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                return Err(format!("Command timed out after {} seconds", timeout_secs));
            }
        }
    }

    Ok((full_output.trim().to_string(), exit_code))
}

/// Install Docker
pub async fn install_docker(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
    use_mirror: bool,
    app_handle: &AppHandle,
) -> Result<String, String> {
    let script = if use_mirror {
        r#"
set -e
if command -v docker &>/dev/null; then
    echo "Docker is already installed: $(docker --version)"
    exit 0
fi
export CHANNEL=stable
curl -fsSL https://get.docker.com -o /tmp/get-docker.sh
sh /tmp/get-docker.sh --mirror Aliyun
rm -f /tmp/get-docker.sh
systemctl enable docker
systemctl start docker
usermod -aG docker $(whoami) 2>/dev/null || true
echo "Docker installed successfully: $(docker --version)"
"#
    } else {
        r#"
set -e
if command -v docker &>/dev/null; then
    echo "Docker is already installed: $(docker --version)"
    exit 0
fi
curl -fsSL https://get.docker.com | sh
systemctl enable docker
systemctl start docker
usermod -aG docker $(whoami) 2>/dev/null || true
echo "Docker installed successfully: $(docker --version)"
"#
    };

    let output = docker_stream_exec(session, cache, session_id, script, 300, app_handle).await
        .map_err(|e| format!("Docker installation failed: {}", e))?;

    let _ = app_handle.emit("docker-action-progress", serde_json::json!({
        "sessionId": session_id,
        "line": "Installation completed!",
        "status": "done",
    }));

    Ok(output)
}

/// Uninstall Docker
pub async fn uninstall_docker(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
    app_handle: &AppHandle,
) -> Result<String, String> {
    let script = r#"
set -e
if ! command -v docker &>/dev/null; then
    echo "Docker is not installed"
    exit 0
fi

# Stop all running containers
echo "Stopping containers..."
docker ps -q | xargs -r docker stop 2>/dev/null || true
docker ps -aq | xargs -r docker rm -f 2>/dev/null || true

# Detect package manager and uninstall
echo "Removing Docker packages..."
if command -v apt-get &>/dev/null; then
    apt-get remove -y docker-ce docker-ce-cli docker-ce-rootless-extras docker-buildx-plugin docker-compose-plugin containerd.io 2>/dev/null || true
    apt-get autoremove -y 2>/dev/null || true
elif command -v yum &>/dev/null; then
    yum remove -y docker-ce docker-ce-cli docker-ce-rootless-extras docker-buildx-plugin docker-compose-plugin containerd.io 2>/dev/null || true
fi

systemctl daemon-reload 2>/dev/null || true

# Cleanup Docker apt/yum source
rm -f /etc/apt/sources.list.d/docker.list /etc/apt/keyrings/docker.gpg /etc/yum.repos.d/docker-ce.repo 2>/dev/null || true

echo "Docker uninstalled successfully"
"#;

    let output = docker_stream_exec(session, cache, session_id, script, 120, app_handle).await
        .map_err(|e| format!("Docker uninstall failed: {}", e))?;

    let _ = app_handle.emit("docker-action-progress", serde_json::json!({
        "sessionId": session_id,
        "line": "Uninstall completed!",
        "status": "done",
    }));

    Ok(output)
}

/// List Docker containers
pub async fn docker_container_list(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
) -> Result<Vec<DockerContainer>, String> {
    let cmd = r#"docker ps -a --format '{{.ID}}|||{{.Names}}|||{{.Image}}|||{{.Status}}|||{{.State}}|||{{.Ports}}|||{{.CreatedAt}}'"#;
    let (stdout, stderr, code) = crate::ssh::session_exec_with_output(session, cmd, 30).await?;

    let mut containers = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(7, "|||").collect();
        if parts.len() >= 7 {
            containers.push(DockerContainer {
                id: parts[0].to_string(),
                name: parts[1].to_string(),
                image: parts[2].to_string(),
                status: parts[3].to_string(),
                state: parts[4].to_string(),
                ports: parts[5].to_string(),
                created: parts[6].to_string(),
            });
        }
    }

    // ponytail: docker may return non-zero with warnings but still output valid data
    // exit_code -1 means ExitStatus was not received (russh may deliver it after Eof/Close)
    if containers.is_empty() && code > 0 {
        let err = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else if !stdout.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            format!("Command failed with exit code {}", code)
        };
        return Err(format!("Failed to list containers: {}", err));
    }

    Ok(containers)
}

/// Perform action on a container (start/stop/restart/pause/unpause)
pub async fn docker_container_action(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    container_id: &str,
    action: &str,
) -> Result<String, String> {
    let valid_actions = ["start", "stop", "restart", "pause", "unpause"];
    if !valid_actions.contains(&action) {
        return Err(format!("Invalid action: {}", action));
    }

    let safe_id = container_id.chars().filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_').collect::<String>();
    let cmd = format!("docker {} {}", action, safe_id);
    let (stdout, stderr, code) = crate::ssh::session_exec_with_output(session, &cmd, 30).await?;
    // ponytail: exit_code -1 means ExitStatus not received (russh behavior)
    if code > 0 {
        let err = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else if !stdout.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            format!("Command failed with exit code {}", code)
        };
        return Err(format!(
            "Failed to {} container: {}",
            action, err
        ));
    }
    Ok(format!("Container {} {}ed successfully", safe_id, action))
}

/// Remove a container
pub async fn docker_container_remove(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    container_id: &str,
    force: bool,
) -> Result<String, String> {
    let safe_id = container_id.chars().filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_').collect::<String>();
    let cmd = if force {
        format!("docker rm -f {}", safe_id)
    } else {
        format!("docker rm {}", safe_id)
    };
    let (stdout, stderr, code) = crate::ssh::session_exec_with_output(session, &cmd, 30).await?;
    // ponytail: exit_code -1 means ExitStatus not received (russh behavior)
    if code > 0 {
        let err = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else if !stdout.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            format!("Command failed with exit code {}", code)
        };
        return Err(format!("Failed to remove container: {}", err));
    }
    Ok(format!("Container {} removed successfully", safe_id))
}

/// Get container logs
pub async fn docker_container_logs(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    container_id: &str,
    lines: usize,
) -> Result<String, String> {
    let safe_id = container_id.chars().filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_').collect::<String>();
    let cmd = format!("docker logs --tail {} {} 2>&1", lines.min(5000), safe_id);
    let (stdout, stderr, code) = crate::ssh::session_exec_with_output(session, &cmd, 30).await?;
    if code != 0 {
        return Err(format!(
            "Failed to get logs: {}",
            if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() }
        ));
    }
    Ok(stdout)
}

/// List Docker images
pub async fn docker_image_list(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
) -> Result<Vec<DockerImage>, String> {
    let cmd = r#"docker images --format '{{.ID}}|||{{.Repository}}|||{{.Tag}}|||{{.Size}}|||{{.CreatedAt}}'"#;
    let (stdout, stderr, code) = crate::ssh::session_exec_with_output(session, cmd, 30).await?;

    let mut images = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(5, "|||").collect();
        if parts.len() >= 5 {
            images.push(DockerImage {
                id: parts[0].to_string(),
                repository: parts[1].to_string(),
                tag: parts[2].to_string(),
                size: parts[3].to_string(),
                created: parts[4].to_string(),
            });
        }
    }

    // ponytail: docker may return non-zero with warnings but still output valid data
    // exit_code -1 means ExitStatus was not received (russh may deliver it after Eof/Close)
    if images.is_empty() && code > 0 {
        let err = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else if !stdout.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            format!("Command failed with exit code {}", code)
        };
        return Err(format!("Failed to list images: {}", err));
    }

    Ok(images)
}

/// Pull a Docker image
pub async fn docker_image_pull(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
    image_name: &str,
    app_handle: &AppHandle,
) -> Result<String, String> {
    // Validate image name format
    if image_name.is_empty() || image_name.contains(|c: char| c.is_whitespace() || c == ';' || c == '|' || c == '&') {
        return Err("Invalid image name".to_string());
    }

    // ponytail: Docker requires lowercase repository names, auto-convert to avoid user error
    let image_name_lower = image_name.to_lowercase();
    let cmd = format!("docker pull {}", image_name_lower);
    let output = docker_stream_exec(session, cache, session_id, &cmd, 600, app_handle).await
        .map_err(|e| format!("Failed to pull image: {}", e))?;

    let _ = app_handle.emit("docker-action-progress", serde_json::json!({
        "sessionId": session_id,
        "line": format!("Image {} pulled successfully!", image_name),
        "status": "done",
    }));

    Ok(output)
}

/// Remove a Docker image
pub async fn docker_image_remove(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    image_id: &str,
) -> Result<String, String> {
    let safe_id = image_id.chars().filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == ':' || *c == '/' || *c == '.').collect::<String>();
    let cmd = format!("docker rmi {}", safe_id);
    let (stdout, stderr, code) = crate::ssh::session_exec_with_output(session, &cmd, 30).await?;
    // ponytail: exit_code -1 means ExitStatus not received (russh behavior)
    if code > 0 {
        let err = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else if !stdout.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            format!("Command failed with exit code {}", code)
        };
        return Err(format!("Failed to remove image: {}", err));
    }
    Ok(format!("Image {} removed successfully", safe_id))
}

/// Run a container from an image
pub async fn docker_image_run(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
    image_name: &str,
    run_args: &str,
    app_handle: &AppHandle,
) -> Result<String, String> {
    // Validate image name
    if image_name.is_empty() || image_name.contains(|c: char| c.is_whitespace() || c == ';' || c == '|' || c == '&' || c == '`' || c == '$') {
        return Err("Invalid image name".to_string());
    }

    // Validate run args - block shell injection characters
    if run_args.contains(';') || run_args.contains('|') || run_args.contains('&') || run_args.contains('`') || run_args.contains('$') || run_args.contains('\n') {
        return Err("Invalid arguments: dangerous characters detected".to_string());
    }

    // ponytail: auto-lowercase for consistency with pull
    let image_lower = image_name.to_lowercase();
    
    // Build command: docker run {args} {image}
    let cmd = if run_args.trim().is_empty() {
        format!("docker run -d {}", image_lower)
    } else {
        format!("docker run {} {}", run_args.trim(), image_lower)
    };

    let output = docker_stream_exec(session, cache, session_id, &cmd, 600, app_handle).await
        .map_err(|e| format!("Failed to run container: {}", e))?;

    let _ = app_handle.emit("docker-action-progress", serde_json::json!({
        "sessionId": session_id,
        "line": format!("Container started from {}!", image_lower),
        "status": "done",
    }));

    Ok(output)
}

/// Get Docker mirror/registry configuration
pub async fn docker_get_mirror_config(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
) -> Result<Vec<String>, String> {
    let cmd = r#"cat /etc/docker/daemon.json 2>/dev/null | python3 -c "import sys,json;d=json.load(sys.stdin);print('\n'.join(d.get('registry-mirrors',[])))" 2>/dev/null || echo """#;
    let (stdout, _, _) = crate::ssh::session_exec_with_output(session, cmd, 10).await?;
    let mirrors: Vec<String> = stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    Ok(mirrors)
}

/// Set Docker mirror/registry configuration
pub async fn docker_set_mirror_config(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    mirrors: &[String],
) -> Result<String, String> {
    // Build JSON for daemon.json
    let mirrors_json: Vec<String> = mirrors.iter().map(|m| format!("\"{}\"" , m)).collect();
    let mirrors_array = mirrors_json.join(",");

    let script = format!(r#"
mkdir -p /etc/docker
cat > /etc/docker/daemon.json << 'DAEMON_EOF'
{{
  "registry-mirrors": [{mirrors}]
}}
DAEMON_EOF
systemctl daemon-reload
systemctl restart docker
echo "Docker mirror configured: {mirrors}"
"#, mirrors = mirrors_array);

    let (stdout, stderr, code) = crate::ssh::session_exec_with_output(session, &script, 30).await?;
    if code != 0 {
        return Err(format!(
            "Failed to configure mirror: {}",
            if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() }
        ));
    }
    Ok("Docker mirror configured successfully. Docker service restarted.".to_string())
}

// ===== Database Management =====

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DbInfo {
    pub name: String,
    pub size_mb: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BackupInfo {
    pub filename: String,
    pub size_bytes: u64,
    pub created_at: String,
}

/// Try to get a mysql command prefix with credentials.
/// Tries multiple authentication methods in order of preference.
async fn get_mysql_cmd(session: &SshSession, _cache: &SshCache, _session_id: &str) -> String {
    // Method 1: Check /root/.my.cnf for password
    let (cnf, _, _) = crate::ssh::session_exec_with_output(session, "cat /root/.my.cnf 2>/dev/null", 5)
        .await
        .unwrap_or((String::new(), String::new(), -1));
    
    for line in cnf.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("password") {
            // formats: password=xxx, password = xxx, password="xxx"
            let val = rest.trim_start_matches([' ', '=', '"', '\'']).trim_end_matches(['"', '\'']).to_string();
            if !val.is_empty() {
                return format!("mysql -u root -p'{}'", val);
            }
        }
    }
    
    // Method 1.5: Check /tmp/mysql_root_password.txt (written by our install script)
    let (tmp_pw, _, _) = crate::ssh::session_exec_with_output(session, "cat /tmp/mysql_root_password.txt 2>/dev/null", 5)
        .await
        .unwrap_or((String::new(), String::new(), -1));
    let pw = tmp_pw.trim().to_string();
    if !pw.is_empty() {
        return format!("mysql -u root -p'{}'", pw.replace('\'', "'\\''"));
    }

    // Method 2: Try debian-sys-maint user (Debian/Ubuntu specific)
    let (debian_cnf, _, _) = crate::ssh::session_exec_with_output(session, "cat /etc/mysql/debian.cnf 2>/dev/null", 5)
        .await
        .unwrap_or((String::new(), String::new(), -1));
    
    let mut debian_user = String::new();
    let mut debian_pass = String::new();
    for line in debian_cnf.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("user") {
            debian_user = rest.trim_start_matches([' ', '=']).trim().to_string();
        } else if let Some(rest) = trimmed.strip_prefix("password") {
            debian_pass = rest.trim_start_matches([' ', '=', '"', '\'']).trim_end_matches(['"', '\'']).to_string();
        }
    }
    
    if !debian_user.is_empty() && !debian_pass.is_empty() {
        return format!("mysql -u {} -p'{}'", debian_user, debian_pass);
    }
    
    // Method 3: Try plain mysql (works if socket auth is configured or running as root)
    // Test it first
    let (_, _, test_code) = crate::ssh::session_exec_with_output(session, "mysql -e 'SELECT 1' 2>&1", 5)
        .await
        .unwrap_or((String::new(), String::new(), -1));
    
    if test_code == 0 {
        return "mysql".to_string();
    }
    
    // Method 4: Use sudo to run mysql as root (bypasses password requirement)
    // This works if the SSH user has sudo privileges
    "sudo mysql".to_string()
}

/// List all user databases (excluding system databases)
pub async fn list_databases(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
) -> Result<Vec<DbInfo>, String> {
    // Check cache first (30 seconds TTL for database list)
    if let Some(cached) = cache.get(session_id, "database_list", 30) {
        // Parse cached JSON
        if let Ok(dbs) = serde_json::from_str::<Vec<DbInfo>>(&cached) {
            return Ok(dbs);
        }
    }

    let mysql_cmd = get_mysql_cmd(session, cache, session_id).await;

    // Use SQL query to directly get user databases only (excludes system databases)
    // This avoids parsing issues with SHOW DATABASES output format variations
    let query = r#"SELECT SCHEMA_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME NOT IN ('information_schema', 'performance_schema', 'mysql', 'sys') ORDER BY SCHEMA_NAME"#;
    
    let (stdout, stderr, code) = crate::ssh::session_exec_with_output(session,
            &format!("{} --batch --skip-column-names -e \"{}\"", mysql_cmd, query),
            5,
        )
        .await?;

    // Parse database names from output (one per line, no header due to --skip-column-names)
    // Even if exit code is non-zero, if we have stdout content and no stderr, treat as success
    let mut dbs = Vec::new();
    
    for line in stdout.lines() {
        let db_name = line.trim();
        if !db_name.is_empty() {
            // No size query - just add database with 0.0 size as requested
            dbs.push(DbInfo { name: db_name.to_string(), size_mb: 0.0 });
        }
    }
    
    // If we got databases, cache and return them even if exit code was non-zero
    // (MySQL may return warnings but still succeed)
    if !dbs.is_empty() {
        // Cache result for 60 seconds to speed up repeated loads
        if let Ok(json) = serde_json::to_string(&dbs) {
            cache.put(session_id, "database_list", json);
        }
        return Ok(dbs);
    }
    
    // Only report error if we have no results and there's an actual error
    if code != 0 {
        let err_msg = if !stderr.trim().is_empty() {
            format!("Failed to list databases: {}", stderr.trim())
        } else {
            "Failed to list databases: unknown error".to_string()
        };
        return Err(err_msg);
    }
    
    Ok(dbs)
}

/// Validate IP address or CIDR notation (e.g., 192.168.1.100, 192.168.1.%, 10.0.0.0/8)
fn is_valid_ip_or_cidr(ip: &str) -> bool {
    // Allow wildcard %
    if ip == "%" {
        return true;
    }
    
    // Check for CIDR notation (e.g., 10.0.0.0/8)
    if let Some((ip_part, cidr_part)) = ip.split_once('/') {
        // Validate CIDR part is a number between 0-32
        if let Ok(cidr) = cidr_part.parse::<u32>() {
            if cidr > 32 {
                return false;
            }
        } else {
            return false;
        }
        // Validate IP part
        return is_valid_ipv4(ip_part);
    }
    
    // Check for wildcard pattern (e.g., 192.168.1.%)
    if ip.contains('%') {
        let parts: Vec<&str> = ip.split('.').collect();
        if parts.len() != 4 {
            return false;
        }
        // Each part should be either a number (0-255) or %
        for part in parts {
            if part == "%" {
                continue;
            }
            if let Ok(num) = part.parse::<u32>() {
                if num > 255 {
                    return false;
                }
            } else {
                return false;
            }
        }
        return true;
    }
    
    // Standard IPv4 validation
    is_valid_ipv4(ip)
}

/// Validate IPv4 address
fn is_valid_ipv4(ip: &str) -> bool {
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    for part in parts {
        if let Ok(num) = part.parse::<u32>() {
            if num > 255 {
                return false;
            }
        } else {
            return false;
        }
    }
    true
}

/// Ensure MySQL accepts remote connections by setting bind-address = 0.0.0.0
/// ponytail: idempotent — only modifies config and restarts if bind-address is not already 0.0.0.0
async fn ensure_mysql_remote_access(session: &SshSession) -> Result<(), String> {
    // Find the active MySQL/MariaDB config file
    let check_cmd = r#"
for f in /etc/mysql/mysql.conf.d/mysqld.cnf /etc/mysql/mariadb.conf.d/50-server.cnf /etc/my.cnf /etc/mysql/my.cnf; do
  if [ -f "$f" ]; then
    CURRENT=$(grep -E '^\s*bind-address' "$f" 2>/dev/null | tail -1 | sed 's/.*=\s*//' | tr -d ' ')
    if [ "$CURRENT" = "0.0.0.0" ]; then
      echo "ALREADY_OK"
      exit 0
    fi
    echo "CONFIG_FILE=$f"
    exit 0
  fi
done
echo "NO_CONFIG"
"#;
    let (stdout, _, _) = crate::ssh::session_exec_with_output(session, check_cmd, 10).await?;
    
    if stdout.contains("ALREADY_OK") {
        return Ok(());
    }
    
    let config_file = stdout.lines()
        .find(|l| l.starts_with("CONFIG_FILE="))
        .map(|l| l.trim_start_matches("CONFIG_FILE=").to_string());
    
    if let Some(cfg) = config_file {
        // Set bind-address = 0.0.0.0 (replace existing or append)
        let fix_cmd = format!(
            r#"if grep -qE '^\s*bind-address' '{cfg}'; then
  sed -i 's/^\s*bind-address\s*=.*/bind-address = 0.0.0.0/' '{cfg}'
else
  echo 'bind-address = 0.0.0.0' >> '{cfg}'
fi
# Restart MySQL/MariaDB
for svc in mysql mysqld mariadb; do
  if systemctl is-active $svc >/dev/null 2>&1; then
    systemctl restart $svc
    break
  fi
done
echo "DONE"
"#,
            cfg = cfg
        );
        let (out, _, _) = crate::ssh::session_exec_with_output(session, &fix_cmd, 30).await?;
        if !out.contains("DONE") {
            return Err("Failed to configure MySQL bind-address".to_string());
        }
    }
    // If no config file found, skip (unusual setup)
    Ok(())
}

/// Create a database with user and grant privileges
pub async fn create_database(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
    db_name: &str,
    db_user: &str,
    db_pass: &str,
    charset: &str,
    access_type: &str,
    allowed_ip: &str,
) -> Result<String, String> {
    // ponytail: skip redundant `command -v mysql` check — the SQL exec below will
    // fail with a clear error if mysql client is missing, saving 5s per call
    let safe_db = db_name.replace('`', "");
    let safe_user = db_user.replace('`', "");
    // ponytail: escape single quotes in password to prevent SQL syntax errors
    let safe_pw = db_pass.replace('\'', "\\'");
    
    // Validate and sanitize charset (whitelist approach)
    let valid_charsets = ["utf8mb4", "utf8", "gbk", "big5", "latin1"];
    let safe_charset = if valid_charsets.contains(&charset) {
        charset
    } else {
        "utf8mb4" // Default fallback
    };
    
    // Parse multiple IPs from allowed_ip (newline separated)
    let access_hosts: Vec<&str> = match access_type {
        "local" => vec!["localhost"],
        "any" => vec!["%"],
        "ip" => {
            // Split by newline, trim each line, filter empty lines
            let ips: Vec<&str> = allowed_ip
                .split('\n')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect();
            
            if ips.is_empty() {
                return Err("No IP addresses provided".to_string());
            }
            
            // Basic validation for each IP
            for ip in &ips {
                if !is_valid_ip_or_cidr(ip) {
                    return Err(format!("Invalid IP address format: {}", ip));
                }
            }
            ips
        },
        _ => vec!["localhost"], // Default fallback
    };
    
    // Build SQL for multiple access hosts
    let mut sql = format!(
        "CREATE DATABASE IF NOT EXISTS `{}` CHARACTER SET {} COLLATE {}_general_ci;\n",
        safe_db, safe_charset, safe_charset
    );
    
    // Create user and grant privileges for each access host
    for host in &access_hosts {
        sql.push_str(&format!(
            "CREATE USER IF NOT EXISTS '{}'@'{}' IDENTIFIED BY '{}';\n\
             GRANT ALL PRIVILEGES ON `{}`.* TO '{}'@'{}';\n",
            safe_user, host, safe_pw, safe_db, safe_user, host
        ));
    }
    
    sql.push_str("FLUSH PRIVILEGES;\n");

    // ponytail: configure MySQL bind-address for remote access when not localhost-only
    if access_type != "local" {
        ensure_mysql_remote_access(session).await.ok();
    }

    let mysql_cmd = get_mysql_cmd(session, cache, session_id).await;

    // Write SQL via SFTP (reliable escaping, same pattern as create_site)
    let tmp_sql = "/tmp/db_setup.sql";
    crate::ssh::session_write_file(session, tmp_sql, &sql).await?;

    let (db_out, db_err, db_code) = crate::ssh::session_exec_with_output(session, &format!("{} < {} 2>&1", mysql_cmd, tmp_sql), 30)
        .await?;

    // Verify database was created
    let verify_cmd = format!("{} -e 'SHOW DATABASES' 2>&1 | grep -iw '{}'", mysql_cmd, safe_db.replace('\'', ""));
    let (verify_out, _, _) = crate::ssh::session_exec_with_output(session, &verify_cmd, 10)
        .await?;
    let db_exists = !verify_out.trim().is_empty();

    // Cleanup temp file
    let _ = crate::ssh::session_exec_with_output(session, &format!("rm -f {}", tmp_sql), 5).await;

    if db_code != 0 && !db_exists {
        let full_output = format!("{} {}", db_out, db_err).trim().to_string();
        return Err(if full_output.is_empty() {
            "Database creation failed (unknown error)".to_string()
        } else {
            full_output
        });
    }

    // Invalidate database list cache so next list reflects the new db
    cache.invalidate(session_id, &["database_list"]);
    Ok(format!("Database '{}' created successfully", db_name))
}

/// Delete a database and its associated user
pub async fn delete_database(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
    db_name: &str,
    db_user: &str,
) -> Result<String, String> {
    let safe_db = db_name.replace('`', "");
    let safe_user = db_user.replace('`', "");
    // ponytail: drop user for ALL hosts — single DROP USER with comma-separated list (PREPARE supports single stmt only)
    let sql = format!(
        "DROP DATABASE IF EXISTS `{}`;\n\
         SET @sql = (SELECT CONCAT('DROP USER IF EXISTS ', GROUP_CONCAT(CONCAT('''', user, '''@''', host, '''') SEPARATOR ', ')) FROM mysql.user WHERE user = '{}');\n\
         SET @sql = IFNULL(@sql, 'SELECT 1');\n\
         PREPARE stmt FROM @sql;\n\
         EXECUTE stmt;\n\
         DEALLOCATE PREPARE stmt;\n\
         FLUSH PRIVILEGES;\n",
        safe_db, safe_user
    );

    let mysql_cmd = get_mysql_cmd(session, cache, session_id).await;

    let tmp_sql = "/tmp/db_drop.sql";
    crate::ssh::session_write_file(session, tmp_sql, &sql).await?;

    let (db_out, db_err, db_code) = crate::ssh::session_exec_with_output(session, &format!("{} < {} 2>&1", mysql_cmd, tmp_sql), 30)
        .await?;

    let _ = crate::ssh::session_exec_with_output(session, &format!("rm -f {}", tmp_sql), 5).await;

    // Any non-zero exit code = error
    if db_code != 0 {
        let combined = format!("{} {}", db_out, db_err).trim().to_string();
        return Err(if combined.is_empty() {
            "Database deletion failed (unknown error)".to_string()
        } else {
            combined
        });
    }

    // Invalidate database list cache so next list reflects the deletion
    cache.invalidate(session_id, &["database_list"]);
    Ok(format!("Database '{}' deleted successfully", db_name))
}

/// Clear (truncate all tables in) a database without dropping it
pub async fn clear_database(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
    db_name: &str,
) -> Result<String, String> {
    let safe_db = db_name.replace('`', "");
    // ponytail: use stored procedure with cursor to TRUNCATE each table individually
    // (PREPARE only supports single statement, so GROUP_CONCAT approach fails)
    let sql = format!(
        "SET FOREIGN_KEY_CHECKS = 0;\n\
         USE `{db}`;\n\
         DELIMITER //\n\
         CREATE PROCEDURE `clear_{db}`()\n\
         BEGIN\n\
             DECLARE done INT DEFAULT FALSE;\n\
             DECLARE tname VARCHAR(255);\n\
             DECLARE cur CURSOR FOR SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA = '{db}';\n\
             DECLARE CONTINUE HANDLER FOR NOT FOUND SET done = TRUE;\n\
             OPEN cur;\n\
             read_loop: LOOP\n\
                 FETCH cur INTO tname;\n\
                 IF done THEN LEAVE read_loop; END IF;\n\
                 SET @s = CONCAT('TRUNCATE TABLE `{db}`.', tname);\n\
                 PREPARE stmt FROM @s;\n\
                 EXECUTE stmt;\n\
                 DEALLOCATE PREPARE stmt;\n\
             END LOOP;\n\
             CLOSE cur;\n\
         END //\n\
         DELIMITER ;\n\
         CALL `clear_{db}`();\n\
         DROP PROCEDURE `clear_{db}`;\n\
         SET FOREIGN_KEY_CHECKS = 1;\n",
        db = safe_db
    );

    let mysql_cmd = get_mysql_cmd(session, cache, session_id).await;

    let tmp_sql = "/tmp/db_clear.sql";
    crate::ssh::session_write_file(session, tmp_sql, &sql).await?;

    let (db_out, db_err, db_code) = crate::ssh::session_exec_with_output(session, &format!("{} < {} 2>&1", mysql_cmd, tmp_sql), 30)
        .await?;

    let _ = crate::ssh::session_exec_with_output(session, &format!("rm -f {}", tmp_sql), 5).await;

    if db_code != 0 {
        let combined = format!("{} {}", db_out, db_err).trim().to_string();
        return Err(if combined.is_empty() {
            "Database clear failed (unknown error)".to_string()
        } else {
            combined
        });
    }

    Ok(format!("Database '{}' cleared successfully (all tables truncated)", db_name))
}

/// Change database access permission
pub async fn change_db_access(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
    db_name: &str,
    db_user: &str,
    db_pass: &str,
    access_type: &str,
    allowed_ip: &str,
) -> Result<String, String> {
    let safe_db = db_name.replace('`', "");
    let safe_user = db_user.replace('`', "");
    // ponytail: escape single quotes in password
    let safe_pw = db_pass.replace('\'', "\\'");
    
    // Parse multiple IPs from allowed_ip (newline separated)
    let access_hosts: Vec<&str> = match access_type {
        "local" => vec!["localhost"],
        "any" => vec!["%"],
        "ip" => {
            // Split by newline, trim each line, filter empty lines
            let ips: Vec<&str> = allowed_ip
                .split('\n')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect();
            
            if ips.is_empty() {
                return Err("No IP addresses provided".to_string());
            }
            
            // Basic validation for each IP
            for ip in &ips {
                if !is_valid_ip_or_cidr(ip) {
                    return Err(format!("Invalid IP address format: {}", ip));
                }
            }
            ips
        },
        _ => vec!["localhost"], // Default fallback
    };
    
    // ponytail: dynamic delete all old users — single DROP USER with comma-separated list
    let mut sql = String::new();
    sql.push_str(&format!(
        "SET @drop_sql = (SELECT CONCAT('DROP USER IF EXISTS ', GROUP_CONCAT(CONCAT('''', user, '''@''', host, '''') SEPARATOR ', ')) FROM mysql.user WHERE user = '{}');\n\
         SET @drop_sql = IFNULL(@drop_sql, 'SELECT 1');\n\
         PREPARE stmt FROM @drop_sql;\n\
         EXECUTE stmt;\n\
         DEALLOCATE PREPARE stmt;\n",
        safe_user
    ));
    
    // Create user and grant privileges for each new access host
    for host in &access_hosts {
        sql.push_str(&format!(
            "CREATE USER IF NOT EXISTS '{}'@'{}' IDENTIFIED BY '{}';\n\
             GRANT ALL PRIVILEGES ON `{}`.* TO '{}'@'{}';\n",
            safe_user, host, safe_pw, safe_db, safe_user, host
        ));
    }
    
    sql.push_str("FLUSH PRIVILEGES;\n");

    // ponytail: configure MySQL bind-address for remote access when not localhost-only
    if access_type != "local" {
        ensure_mysql_remote_access(session).await.ok();
    }

    let mysql_cmd = get_mysql_cmd(session, cache, session_id).await;

    let tmp_sql = "/tmp/db_access.sql";
    crate::ssh::session_write_file(session, tmp_sql, &sql).await?;

    let (db_out, db_err, db_code) = crate::ssh::session_exec_with_output(session, &format!("{} < {} 2>&1", mysql_cmd, tmp_sql), 30)
        .await?;

    let _ = crate::ssh::session_exec_with_output(session, &format!("rm -f {}", tmp_sql), 5).await;

    // Any non-zero exit code = error
    if db_code != 0 {
        let combined = format!("{} {}", db_out, db_err).trim().to_string();
        return Err(if combined.is_empty() {
            "Failed to change database access permission (unknown error)".to_string()
        } else {
            combined
        });
    }

    Ok(format!("Database '{}' access permission changed to {}", db_name, access_type))
}

// ===== Redis Management =====

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RedisKeyInfo {
    pub key: String,
    pub value_preview: String,
    pub data_type: String,
    pub length: usize,
    pub ttl: i64, // -1 means no expiry
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RedisDbSize {
    pub db_index: u8,
    pub key_count: usize,
}

/// Check if Redis is installed and running
/// Returns: Ok(true) if running, Ok(false) if installed but stopped, Err("not_installed") if not installed
pub async fn check_redis_status(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
) -> Result<bool, String> {
    // Try redis-cli ping directly
    let (out, stderr, code) = crate::ssh::session_exec_with_output(session, "redis-cli ping 2>&1", 5)
        .await?;

    log::info!("[REDIS] ping => stdout='{}' stderr='{}' exit_code={}", out, stderr, code);

    let combined = format!("{}{}", out, stderr).to_lowercase();

    // Got PONG response - Redis is running
    if combined.contains("pong") {
        log::info!("[REDIS] RUNNING (PONG received)");
        return Ok(true);
    }

    // redis-cli command not found - not installed
    if combined.contains("command not found")
        || combined.contains("no such file or directory")
        || combined.contains("not found")
    {
        log::info!("[REDIS] NOT INSTALLED");
        return Err("not_installed".to_string());
    }

    // redis-cli exists but ping failed (connection refused, auth required, etc.)
    // Fallback: check if redis-server process is running
    let (p_out, _, _) = crate::ssh::session_exec_with_output(session, "pgrep -x redis-server || pgrep redis-server", 3)
        .await?;
    log::info!("[REDIS] pgrep output='{}'", p_out);

    // If pgrep found a process, redis-server is running
    if !p_out.trim().is_empty() {
        log::info!("[REDIS] RUNNING (process found via pgrep)");
        return Ok(true);
    }

    // Installed but not running
    log::info!("[REDIS] STOPPED");
    Ok(false)
}

/// Get Redis version
pub async fn get_redis_version(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
) -> Result<String, String> {
    let (out, _, code) = crate::ssh::session_exec_with_output(session, "redis-cli --version 2>&1", 5)
        .await?;
    
    if code != 0 {
        return Err("Redis not found".to_string());
    }
    
    // Parse version from output like "redis-cli 7.2.4"
    let parts: Vec<&str> = out.split_whitespace().collect();
    if parts.len() >= 2 {
        Ok(parts[1].to_string())
    } else {
        Ok(out.trim().to_string())
    }
}

/// Get sizes of all databases (0-15)
/// ponytail: single INFO keyspace call replaces 16 redis-cli DBSIZE processes
pub async fn redis_dbsize_all(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
) -> Result<Vec<RedisDbSize>, String> {
    let (out, _, code) = crate::ssh::session_exec_with_output(session, "redis-cli INFO keyspace", 5)
        .await?;

    if code != 0 {
        return Err(format!("Failed to get database sizes: {}", out));
    }

    let mut results = vec![0usize; 16];
    for line in out.lines() {
        // Format: db0:keys=123,expires=0,avg_ttl=0
        if let Some(rest) = line.strip_prefix("db") {
            if let Some((idx_str, kv)) = rest.split_once(':') {
                if let Ok(idx) = idx_str.parse::<usize>() {
                    if idx < 16 {
                        for part in kv.split(',') {
                            if let Some(n) = part.strip_prefix("keys=") {
                                results[idx] = n.trim().parse().unwrap_or(0);
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(results.into_iter().enumerate().map(|(i, c)| RedisDbSize {
        db_index: i as u8,
        key_count: c,
    }).collect())
}

/// Scan keys in a database with pattern matching and pagination
/// ponytail: redis-cli pipeline mode replaces ~4N redis-cli processes with 3 (SCAN + 2 pipelines)
pub async fn redis_scan_keys(
    session: &SshSession,
    _cache: &SshCache,
    session_id: &str,
    db_index: u8,
    pattern: &str,
    search_type: &str,
    cursor: usize,
    count: usize,
) -> Result<(Vec<RedisKeyInfo>, usize), String> {
    let match_pattern = if pattern.is_empty() { "*" } else { pattern };
    let safe_pat = match_pattern.replace('\'', "'\\''");

    // Helper: send commands via base64-encoded pipeline (avoids all shell escaping issues)
    let run_pipeline = |cmds: String| {
        let b64 = B64.encode(cmds.as_bytes());
        let cmd = format!("printf '%s' '{}' | base64 -d | redis-cli --raw -n {}", b64, db_index);
        let sess = session.clone();
        let _sid = session_id.to_string();
        async move {
            let (out, _, code) = crate::ssh::session_exec_with_output(&sess, &cmd, 30).await?;
            if code != 0 {
                return Err(format!("Pipeline failed: {}", out));
            }
            Ok(out.lines().map(|l| l.trim_end().to_string()).collect::<Vec<_>>())
        }
    };

    // Step 1: SCAN
    let scan_match = if search_type == "value" { "*" } else { &safe_pat };
    let scan_cmd = format!(
        "redis-cli --raw -n {} SCAN {} MATCH '{}' COUNT {}",
        db_index, cursor, scan_match, count
    );
    let (scan_out, _, code) = crate::ssh::session_exec_with_output(session, &scan_cmd, 15).await?;
    if code != 0 {
        return Err(format!("Failed to scan keys: {}", scan_out));
    }

    let mut scan_lines = scan_out.lines();
    let next_cursor: usize = scan_lines.next().unwrap_or("0").trim().parse().unwrap_or(0);
    let key_names: Vec<String> = scan_lines
        .map(|l| l.to_string())
        .filter(|l| !l.is_empty())
        .collect();

    if key_names.is_empty() {
        return Ok((vec![], next_cursor));
    }

    // Step 2: Pipeline TYPE + TTL for all keys
    let type_ttl_cmds: String = key_names.iter()
        .map(|k| format!("TYPE \"{}\"\nTTL \"{}\"", k, k))
        .collect::<Vec<_>>()
        .join("\n");
    let type_ttl_lines = run_pipeline(type_ttl_cmds).await?;

    let mut types = Vec::with_capacity(key_names.len());
    let mut ttls = Vec::with_capacity(key_names.len());
    let mut i = 0;
    while i < type_ttl_lines.len() {
        let t = type_ttl_lines.get(i).map(|s| s.as_str()).unwrap_or("none");
        let ttl = type_ttl_lines.get(i + 1).and_then(|s| s.parse::<i64>().ok()).unwrap_or(-1);
        // Skip error results (e.g. key deleted between SCAN and TYPE)
        if !t.starts_with("ERR") && !t.starts_with("WRONGTYPE") {
            types.push(t.to_string());
            ttls.push(ttl);
        } else {
            types.push("none".to_string());
            ttls.push(-1);
        }
        i += 2;
    }

    // Value search: filter string-type keys by GET value
    let (key_names, types, ttls) = if search_type == "value" {
        let mut get_cmds = String::new();
        let mut string_indices = Vec::new();
        for (idx, t) in types.iter().enumerate() {
            if t == "string" {
                get_cmds.push_str(&format!("GET \"{}\"\n", key_names[idx]));
                string_indices.push(idx);
            }
        }
        if string_indices.is_empty() {
            return Ok((vec![], next_cursor));
        }
        let get_lines = run_pipeline(get_cmds).await?;
        let search_pat = match_pattern;
        // ponytail: all kept keys are type=string (value search only matches strings)
        let mut kept_names = Vec::new();
        let mut kept_ttls = Vec::new();
        for (gi, &idx) in string_indices.iter().enumerate() {
            if let Some(val) = get_lines.get(gi) {
                if val.contains(search_pat) {
                    kept_names.push(key_names[idx].clone());
                    kept_ttls.push(ttls[idx]);
                }
            }
        }
        let kept_types = vec!["string".to_string(); kept_names.len()];
        (kept_names, kept_types, kept_ttls)
    } else {
        (key_names, types, ttls)
    };

    if key_names.is_empty() {
        return Ok((vec![], next_cursor));
    }

    // Step 3: Pipeline length + value preview based on type
    let mut len_cmds = String::new();
    let mut results_per_key: Vec<usize> = Vec::new();
    for (idx, k) in key_names.iter().enumerate() {
        match types[idx].as_str() {
            "string" => {
                len_cmds.push_str(&format!("STRLEN \"{}\"\nGETRANGE \"{}\" 0 199\n", k, k));
                results_per_key.push(2);
            }
            "list" => {
                len_cmds.push_str(&format!("LLEN \"{}\"\n", k));
                results_per_key.push(1);
            }
            "set" => {
                len_cmds.push_str(&format!("SCARD \"{}\"\n", k));
                results_per_key.push(1);
            }
            "hash" => {
                len_cmds.push_str(&format!("HLEN \"{}\"\n", k));
                results_per_key.push(1);
            }
            "zset" => {
                len_cmds.push_str(&format!("ZCARD \"{}\"\n", k));
                results_per_key.push(1);
            }
            _ => results_per_key.push(0),
        }
    }

    let len_lines = if len_cmds.is_empty() {
        vec![]
    } else {
        run_pipeline(len_cmds).await?
    };

    // Assemble final results
    let mut keys = Vec::new();
    let mut line_idx = 0;
    for (idx, k) in key_names.iter().enumerate() {
        let t = types.get(idx).map(|s| s.as_str()).unwrap_or("none");
        let ttl = ttls.get(idx).copied().unwrap_or(-1);
        let n = results_per_key.get(idx).copied().unwrap_or(0);

        let (length, preview) = if n >= 2 {
            let len_val = len_lines.get(line_idx).and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
            let vp = len_lines.get(line_idx + 1).cloned().unwrap_or_default();
            line_idx += 2;
            (len_val, vp)
        } else if n == 1 {
            let len_val = len_lines.get(line_idx).and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
            line_idx += 1;
            let vp = match t {
                "list" => format!("List with {} elements", len_val),
                "set" => format!("Set with {} members", len_val),
                "hash" => format!("Hash with {} fields", len_val),
                "zset" => format!("Sorted set with {} members", len_val),
                _ => String::new(),
            };
            (len_val, vp)
        } else {
            (0usize, "<Unknown type>".to_string())
        };

        keys.push(RedisKeyInfo {
            key: k.clone(),
            data_type: t.to_string(),
            ttl,
            length,
            value_preview: preview,
        });
    }

    Ok((keys, next_cursor))
}

/// Set or update a key-value pair
pub async fn redis_set_key(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    db_index: u8,
    key: &str,
    value: &str,
    ttl: Option<i64>,
) -> Result<String, String> {
    let escaped_key = key.replace('\'', "'\\''");
    let escaped_value = value.replace('\'', "'\\''");
    
    let cmd = if let Some(ttl_val) = ttl {
        format!("redis-cli -n {} SET '{}' '{}' EX {}", db_index, escaped_key, escaped_value, ttl_val)
    } else {
        format!("redis-cli -n {} SET '{}' '{}'", db_index, escaped_key, escaped_value)
    };
    
    let (out, err, code) = crate::ssh::session_exec_with_output(session, &cmd, 10).await?;
    
    if code != 0 || !out.trim().to_lowercase().contains("ok") {
        return Err(format!("Failed to set key: {} {}", out, err));
    }
    
    Ok(format!("Key '{}' set successfully", key))
}

/// Delete one or more keys
pub async fn redis_del_key(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    db_index: u8,
    keys: &[String],
) -> Result<usize, String> {
    if keys.is_empty() {
        return Ok(0);
    }
    
    let escaped_keys: Vec<String> = keys
        .iter()
        .map(|k| k.replace('\'', "'\\''"))
        .collect();
    
    let cmd = format!(
        "redis-cli -n {} DEL {}",
        db_index,
        escaped_keys.iter().map(|k| format!("'{}'", k)).collect::<Vec<_>>().join(" ")
    );
    
    let (out, _, code) = crate::ssh::session_exec_with_output(session, &cmd, 10).await?;
    
    if code != 0 {
        return Err(format!("Failed to delete keys: {}", out));
    }
    
    let deleted = out.trim().parse::<usize>().unwrap_or(0);
    Ok(deleted)
}

/// Flush a database (delete all keys)
pub async fn redis_flushdb(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    db_index: u8,
) -> Result<String, String> {
    let cmd = format!("redis-cli -n {} FLUSHDB", db_index);
    let (out, err, code) = crate::ssh::session_exec_with_output(session, &cmd, 10).await?;
    
    if code != 0 {
        return Err(format!("Failed to flush database: {} {}", out, err));
    }
    
    Ok(format!("Database {} flushed successfully", db_index))
}

/// Create a backup using BGSAVE
pub async fn redis_save_backup(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
) -> Result<String, String> {
    // Trigger background save
    let (out, _, code) = crate::ssh::session_exec_with_output(session, "redis-cli BGSAVE 2>&1", 10)
        .await?;
    
    if code != 0 {
        return Err(format!("Failed to trigger backup: {}", out));
    }
    
    // Wait a bit for the save to complete
    std::thread::sleep(std::time::Duration::from_secs(2));
    
    // Find latest RDB file
    let (ls_out, _, _) = crate::ssh::session_exec_with_output(session, "ls -lht /var/lib/redis/*.rdb 2>/dev/null | head -1", 5)
        .await?;
    
    if ls_out.trim().is_empty() {
        // Try alternative location
        let (alt_out, _, _) = crate::ssh::session_exec_with_output(session, "find /var -name '*.rdb' -type f -mmin -5 2>/dev/null | head -1", 5)
            .await?;
        
        if alt_out.trim().is_empty() {
            return Err("Backup file not found".to_string());
        }
        
        return Ok(alt_out.trim().to_string());
    }
    
    // Extract filename from ls output
    let parts: Vec<&str> = ls_out.split_whitespace().collect();
    if parts.len() >= 9 {
        Ok(parts[8].to_string())
    } else {
        Err("Could not parse backup file path".to_string())
    }
}

/// List available backup files
pub async fn redis_list_backups(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
) -> Result<Vec<BackupInfo>, String> {
    let (out, _, code) = crate::ssh::session_exec_with_output(session, "ls -lh /var/lib/redis/*.rdb 2>/dev/null", 5)
        .await?;
    
    if code != 0 || out.trim().is_empty() {
        return Ok(Vec::new());
    }
    
    let mut backups = Vec::new();
    for line in out.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 9 {
            let size_str = parts[4];
            let month = parts[5];
            let day = parts[6];
            let time_or_year = parts[7];
            let filename = parts[8];
            
            // Parse size
            let size_bytes = parse_size_string(size_str);
            
            // Parse date
            let created_at = format!("{} {} {}", month, day, time_or_year);
            
            backups.push(BackupInfo {
                filename: filename.to_string(),
                size_bytes,
                created_at,
            });
        }
    }
    
    Ok(backups)
}

fn parse_size_string(size_str: &str) -> u64 {
    let size_str = size_str.trim();
    if let Some(num) = size_str.strip_suffix('K') {
        (num.parse::<f64>().unwrap_or(0.0) * 1024.0) as u64
    } else if let Some(num) = size_str.strip_suffix('M') {
        (num.parse::<f64>().unwrap_or(0.0) * 1024.0 * 1024.0) as u64
    } else if let Some(num) = size_str.strip_suffix('G') {
        (num.parse::<f64>().unwrap_or(0.0) * 1024.0 * 1024.0 * 1024.0) as u64
    } else {
        size_str.parse::<u64>().unwrap_or(0)
    }
}

/// Change MySQL root password
pub async fn change_mysql_root_password(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
    new_password: &str,
) -> Result<String, String> {
    // ponytail: reuse get_mysql_cmd for reliable auth detection instead of hardcoded sudo mysql
    let mysql_cmd = get_mysql_cmd(session, cache, session_id).await;
    
    let safe_pw = new_password.replace('\'', "\\'");
    let sql = format!("ALTER USER 'root'@'localhost' IDENTIFIED BY '{}';\nFLUSH PRIVILEGES;\n", safe_pw);
    
    // Write SQL via SFTP
    let tmp_sql = "/tmp/mysql_change_root_pw.sql";
    crate::ssh::session_write_file(session, tmp_sql, &sql).await?;
    
    let (out, err, code) = crate::ssh::session_exec_with_output(session, &format!("{} < {} 2>&1", mysql_cmd, tmp_sql), 30)
        .await?;
    
    // Cleanup temp file
    let _ = crate::ssh::session_exec_with_output(session, &format!("rm -f {}", tmp_sql), 5).await;
    
    if code != 0 {
        let combined = format!("{} {}", out, err).trim().to_string();
        return Err(if combined.is_empty() {
            "Failed to change root password (unknown error)".to_string()
        } else {
            combined
        });
    }
    
    // ponytail: update /root/.my.cnf so future get_mysql_cmd calls use the new password
    let cnf = format!("[client]\nuser=root\npassword={}\n", new_password);
    let _ = crate::ssh::session_exec_with_output(
        session,
        &format!("echo '{}' > /root/.my.cnf && chmod 600 /root/.my.cnf", cnf.replace('\'', "'\\''" )),
        5,
    ).await;
    
    Ok("MySQL root password changed successfully".to_string())
}

/// Change MySQL database user password
pub async fn change_db_user_password(
    session: &SshSession,
    cache: &SshCache,
    session_id: &str,
    db_user: &str,
    new_password: &str,
    access_type: &str,
    allowed_ip: &str,
) -> Result<String, String> {
    let safe_user = db_user.replace('`', "");
    let safe_pw = new_password.replace('`', "").replace('\'', "\\'");

    let access_hosts: Vec<&str> = match access_type {
        "local" => vec!["localhost"],
        "any" => vec!["%"],
        "ip" => {
            let ips: Vec<&str> = allowed_ip
                .split('\n')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect();
            if ips.is_empty() {
                return Err("No IP addresses provided".to_string());
            }
            ips
        },
        _ => vec!["localhost"],
    };

    let mut sql = String::new();
    for host in &access_hosts {
        sql.push_str(&format!(
            "ALTER USER '{}'@'{}' IDENTIFIED BY '{}';\n",
            safe_user, host, safe_pw
        ));
    }
    sql.push_str("FLUSH PRIVILEGES;\n");

    let mysql_cmd = get_mysql_cmd(session, cache, session_id).await;
    let tmp_sql = "/tmp/db_change_pw.sql";
    crate::ssh::session_write_file(session, tmp_sql, &sql).await?;

    let (out, err, code) = crate::ssh::session_exec_with_output(session, &format!("{} < {} 2>&1", mysql_cmd, tmp_sql), 30)
        .await?;

    let _ = crate::ssh::session_exec_with_output(session, &format!("rm -f {}", tmp_sql), 5).await;

    if code != 0 {
        let combined = format!("{} {}", out, err).trim().to_string();
        return Err(if combined.is_empty() {
            "Failed to change database user password (unknown error)".to_string()
        } else {
            combined
        });
    }

    Ok("Database user password changed successfully".to_string())
}

// ===== Database Remarks Management =====

/// Save database remark to SQLite
pub async fn save_db_remark(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    db_name: &str,
    remark: &str,
) -> Result<String, String> {
    let db_conn = crate::db::init_db()
        .map_err(|e| format!("Failed to init DB: {}", e))?;
    let conn = db_conn.lock().map_err(|_| "DB lock failed".to_string())?;
    
    // Get server host from session
    let server_host = session.connect_info.host.clone();
    
    crate::db::DbRemarksManager::save(&conn, &server_host, db_name, remark)?;
    
    Ok("Remark saved successfully".to_string())
}

/// Get all database remarks for a server
pub async fn get_db_remarks(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
) -> Result<Vec<(String, String)>, String> {
    let db_conn = crate::db::init_db()
        .map_err(|e| format!("Failed to init DB: {}", e))?;
    let conn = db_conn.lock().map_err(|_| "DB lock failed".to_string())?;
    
    // Get server host from session
    let server_host = session.connect_info.host.clone();
    
    Ok(crate::db::DbRemarksManager::list_for_server(&conn, &server_host))
}

// ===== Database Credentials =====

/// Save database credentials (password, access_type, allowed_ip)
pub async fn save_db_credentials(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    db_name: &str,
    db_user: &str,
    password: &str,
    access_type: &str,
    allowed_ip: &str,
) -> Result<String, String> {
    let db_conn = crate::db::init_db()
        .map_err(|e| format!("Failed to init DB: {}", e))?;
    let conn = db_conn.lock().map_err(|_| "DB lock failed".to_string())?;
    let server_host = session.connect_info.host.clone();
    crate::db::DbCredentialsManager::save(&conn, &server_host, db_name, db_user, password, access_type, allowed_ip)?;
    Ok("Credentials saved".to_string())
}

/// List all database credentials for a server
pub async fn get_db_credentials(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
) -> Result<Vec<crate::db::DbCredential>, String> {
    let db_conn = crate::db::init_db()
        .map_err(|e| format!("Failed to init DB: {}", e))?;
    let conn = db_conn.lock().map_err(|_| "DB lock failed".to_string())?;
    let server_host = session.connect_info.host.clone();
    Ok(crate::db::DbCredentialsManager::list_for_server(&conn, &server_host))
}

/// Get credentials for a specific database
pub async fn get_db_credential(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    db_name: &str,
) -> Result<Option<crate::db::DbCredential>, String> {
    let db_conn = crate::db::init_db()
        .map_err(|e| format!("Failed to init DB: {}", e))?;
    let conn = db_conn.lock().map_err(|_| "DB lock failed".to_string())?;
    let server_host = session.connect_info.host.clone();
    Ok(crate::db::DbCredentialsManager::get(&conn, &server_host, db_name))
}

/// Update only the password for a database
pub async fn update_db_credential_password(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    db_name: &str,
    password: &str,
) -> Result<String, String> {
    let db_conn = crate::db::init_db()
        .map_err(|e| format!("Failed to init DB: {}", e))?;
    let conn = db_conn.lock().map_err(|_| "DB lock failed".to_string())?;
    let server_host = session.connect_info.host.clone();
    if password.is_empty() {
        crate::db::DbCredentialsManager::clear_password(&conn, &server_host, db_name)?;
    } else {
        crate::db::DbCredentialsManager::update_password(&conn, &server_host, db_name, password)?;
    }
    Ok("Password updated".to_string())
}

// ===== Database Backup and Import =====

/// Write a temporary MySQL credentials file via SFTP.
/// Returns the remote path. Caller must `rm -f` it when done.
async fn write_mysql_cnf_file(
    session: &SshSession,
    db_user: &str,
    db_password: &str,
) -> Result<String, String> {
    // Escape for .cnf double-quoted value: backslash and double-quote only
    let escaped_pw = db_password.replace('\\', "\\\\").replace('"', "\\\"");
    let cnf_content = format!(
        "[client]\nuser=\"{}\"\npassword=\"{}\"\n",
        db_user, escaped_pw
    );
    let cnf_path = "/tmp/.db_credentials.cnf";
    crate::ssh::session_write_file(session, cnf_path, &cnf_content).await?;
    // Restrict permissions so other users can't read the password
    let _ = crate::ssh::session_exec_with_output(session, "chmod 600 /tmp/.db_credentials.cnf", 5).await;
    Ok(cnf_path.to_string())
}

/// Create a backup of the specified database using mysqldump
pub async fn backup_database(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    db_name: &str,
    db_user: &str,
    db_password: &str,
) -> Result<String, String> {
    // Validate database name (alphanumeric + underscore only)
    if !db_name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err("Invalid database name".to_string());
    }

    if db_user.is_empty() || db_password.is_empty() {
        return Err("数据库账号或密码为空，请先在本地保存密码".to_string());
    }

    // Create backup directory if it doesn't exist
    let create_dir_cmd = "mkdir -p /tmp/db_backups";
    let (_, _, code) = crate::ssh::session_exec_with_output(session, create_dir_cmd, 5)
        .await?;
    
    if code != 0 {
        return Err("Failed to create backup directory".to_string());
    }

    // Generate timestamp for filename
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("Time error: {}", e))?
        .as_secs();
    
    let sql_filename = format!("{}_{}.sql", db_name, timestamp);
    let tar_filename = format!("{}_{}.tar.gz", db_name, timestamp);
    let sql_path = format!("/tmp/db_backups/{}", sql_filename);
    let tar_path = format!("/tmp/db_backups/{}", tar_filename);

    // Write temp credentials file to avoid shell special-character issues with inline passwords
    let cnf_path = write_mysql_cnf_file(session, db_user, db_password).await?;

    // Execute mysqldump using credentials file (no password on command line)
    let dump_cmd = format!(
        "mysqldump --defaults-extra-file={} {} > {}",
        cnf_path, db_name, sql_path
    );
    
    let (_stdout, stderr, code) = crate::ssh::session_exec_with_output(session, &dump_cmd, 300)
        .await?;
    
    // Clean up credentials file regardless of outcome
    let _ = crate::ssh::session_exec_with_output(session, &format!("rm -f {}", cnf_path), 5).await;

    if code != 0 {
        return Err(format!("Backup failed: {}", stderr));
    }

    // Verify SQL file exists
    let verify_cmd = format!("test -f {} && echo 'exists'", sql_path);
    let (verify_out, _, verify_code) = crate::ssh::session_exec_with_output(session, &verify_cmd, 5)
        .await?;
    
    if verify_code != 0 || !verify_out.trim().contains("exists") {
        return Err("SQL backup file was not created".to_string());
    }

    // Compress to tar.gz and remove original SQL file
    let compress_cmd = format!(
        "cd /tmp/db_backups && tar -czf {} {} && rm -f {}",
        tar_filename, sql_filename, sql_filename
    );
    
    let (_, stderr, code) = crate::ssh::session_exec_with_output(session, &compress_cmd, 60)
        .await?;
    
    if code != 0 {
        return Err(format!("Compression failed: {}", stderr));
    }

    // Verify tar.gz file exists
    let verify_tar_cmd = format!("test -f {} && echo 'exists'", tar_path);
    let (verify_tar_out, _, verify_tar_code) = crate::ssh::session_exec_with_output(session, &verify_tar_cmd, 5)
        .await?;
    
    if verify_tar_code != 0 || !verify_tar_out.trim().contains("exists") {
        return Err("tar.gz backup file was not created".to_string());
    }

    Ok(tar_filename)
}

/// List all backup files for a specific database
pub async fn list_db_backups(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    db_name: &str,
) -> Result<Vec<BackupInfo>, String> {
    // Validate database name
    if !db_name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err("Invalid database name".to_string());
    }

    // List backup files for this database (.sql, .tar.gz, and .zip)
    let pattern = format!("/tmp/db_backups/{}*", db_name);
    let cmd = format!("ls -lht {} 2>/dev/null | grep -E '\\.(sql|tar\\.gz|zip)$'", pattern);
    
    let (out, _, code) = crate::ssh::session_exec_with_output(session, &cmd, 5)
        .await?;
    
    if code != 0 || out.trim().is_empty() {
        return Ok(Vec::new());
    }
    
    let mut backups = Vec::new();
    for line in out.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 9 {
            let size_str = parts[4];
            let month = parts[5];
            let day = parts[6];
            let time_or_year = parts[7];
            let filename = parts[8];
            
            // Extract basename (in case ls returns full path)
            let basename = filename.rsplit('/').next().unwrap_or(filename);
            
            // Parse size
            let size_bytes = parse_size_string(size_str);
            
            // Parse date
            let created_at = format!("{} {} {}", month, day, time_or_year);
            
            backups.push(BackupInfo {
                filename: basename.to_string(),
                size_bytes,
                created_at,
            });
        }
    }
    
    Ok(backups)
}

/// Delete a specific backup file
pub async fn delete_db_backup(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    backup_filename: &str,
) -> Result<String, String> {
    // Validate filename (prevent path traversal)
    if backup_filename.contains("..") {
        return Err("Invalid backup filename".to_string());
    }
    
    if !backup_filename.ends_with(".sql") && !backup_filename.ends_with(".tar.gz") && !backup_filename.ends_with(".zip") {
        return Err("Invalid backup file extension".to_string());
    }

    // Support both full path and relative filename
    let backup_path = if backup_filename.starts_with("/") {
        backup_filename.to_string()
    } else {
        format!("/tmp/db_backups/{}", backup_filename)
    };
    let cmd = format!("rm -f {}", backup_path);
    
    let (_, _, code) = crate::ssh::session_exec_with_output(session, &cmd, 5)
        .await?;
    
    if code != 0 {
        return Err("Failed to delete backup file".to_string());
    }
    
    Ok(format!("Backup {} deleted successfully", backup_filename))
}

/// Download database backup file content as bytes
pub async fn download_db_backup(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    backup_filename: &str,
) -> Result<Vec<u8>, String> {
    // Validate filename (prevent path traversal)
    // Allow full paths like /tmp/db_backups/file.sql or just filenames
    if backup_filename.contains("..") {
        return Err("Invalid backup filename".to_string());
    }
    
    if !backup_filename.ends_with(".sql") && !backup_filename.ends_with(".tar.gz") && !backup_filename.ends_with(".zip") {
        return Err("Invalid backup file extension".to_string());
    }

    // Use the provided path directly (it should already be /tmp/db_backups/filename.sql or .tar.gz)
    let backup_path = if backup_filename.starts_with("/") {
        backup_filename.to_string()
    } else {
        format!("/tmp/db_backups/{}", backup_filename)
    };
    
    // Check if file exists
    let check_cmd = format!("test -f {} && echo 'exists'", backup_path);
    let (stdout, _, code) = crate::ssh::session_exec_with_output(session, &check_cmd, 5)
        .await?;
    
    if code != 0 || !stdout.trim().contains("exists") {
        return Err("Backup file not found".to_string());
    }
    
    // Read file as raw bytes via SFTP (preserves binary data)
    crate::ssh::session_read_file_bytes(session, &backup_path).await
}

/// Import database from uploaded SQL content
pub async fn import_database_from_file(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    db_name: &str,
    db_user: &str,
    db_password: &str,
    sql_content: &str,
) -> Result<String, String> {
    // Validate database name
    if !db_name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err("Invalid database name".to_string());
    }

    if db_user.is_empty() || db_password.is_empty() {
        return Err("数据库账号或密码为空，请先在本地保存密码".to_string());
    }

    // Create temporary file with unique name
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("Time error: {}", e))?
        .as_secs();
    
    let temp_path = format!("/tmp/import_{}.sql", timestamp);

    // Write SQL content to temporary file via SFTP (reliable for large files)
    crate::ssh::session_write_file(session, &temp_path, sql_content).await?;

    // Write temp credentials file to avoid shell special-character issues with inline passwords
    let cnf_path = write_mysql_cnf_file(session, db_user, db_password).await?;

    // Import SQL file into database using credentials file (no password on command line)
    let import_cmd = format!(
        "mysql --defaults-extra-file={} {} < {}",
        cnf_path, db_name, temp_path
    );
    
    let (_import_stdout, import_stderr, import_code) = crate::ssh::session_exec_with_output(session, &import_cmd, 300)
        .await?;
    
    // Clean up temp files regardless of outcome
    let _ = crate::ssh::session_exec_with_output(session, &format!("rm -f {} {}", temp_path, cnf_path), 5).await;
    
    if import_code != 0 {
        return Err(format!("Import failed: {}", import_stderr));
    }
    
    Ok(format!("Database {} imported successfully", db_name))
}

/// Import database from uploaded raw bytes (supports .sql, .tar.gz, .zip)
pub async fn import_database_from_file_bytes(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    db_name: &str,
    db_user: &str,
    db_password: &str,
    file_name: &str,
    file_bytes: Vec<u8>,
) -> Result<String, String> {
    // Validate database name
    if !db_name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err("Invalid database name".to_string());
    }

    if db_user.is_empty() || db_password.is_empty() {
        return Err("数据库账号或密码为空，请先在本地保存密码".to_string());
    }

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("Time error: {}", e))?
        .as_secs();

    // Write uploaded bytes to temp file
    let upload_path = format!("/tmp/import_{}_{}", timestamp, file_name);
    crate::ssh::session_write_file_bytes(session, &upload_path, &file_bytes).await?;

    // Determine SQL path based on file extension
    let sql_path = if file_name.ends_with(".tar.gz") {
        let temp_sql = format!("/tmp/import_{}.sql", timestamp);
        // Use gunzip + tar separately for better error diagnosis
        let tar_cmd = format!("gunzip -c {} | tar -xf - -O > {}", upload_path, temp_sql);
        let (_, stderr, code) = crate::ssh::session_exec_with_output(session, &tar_cmd, 60).await?;
        if code != 0 {
            let _ = crate::ssh::session_exec_with_output(session, &format!("rm -f {} {}", upload_path, temp_sql), 5).await;
            return Err(format!("Failed to extract tar.gz: {}", stderr));
        }
        temp_sql
    } else if file_name.ends_with(".zip") {
        let temp_sql = format!("/tmp/import_{}.sql", timestamp);
        let unzip_cmd = format!("unzip -p {} > {}", upload_path, temp_sql);
        let (_, stderr, code) = crate::ssh::session_exec_with_output(session, &unzip_cmd, 60).await?;
        if code != 0 {
            let _ = crate::ssh::session_exec_with_output(session, &format!("rm -f {}", upload_path), 5).await;
            return Err(format!("Failed to extract zip: {}", stderr));
        }
        temp_sql
    } else {
        upload_path.clone()
    };

    // Write temp credentials file
    let cnf_path = write_mysql_cnf_file(session, db_user, db_password).await?;

    // Import SQL into database
    let import_cmd = format!(
        "mysql --defaults-extra-file={} {} < {}",
        cnf_path, db_name, sql_path
    );
    let (_import_stdout, import_stderr, import_code) = crate::ssh::session_exec_with_output(session, &import_cmd, 300).await?;

    // Cleanup
    let cleanup = if file_name.ends_with(".tar.gz") || file_name.ends_with(".zip") {
        format!("{} {} {}", upload_path, sql_path, cnf_path)
    } else {
        format!("{} {}", upload_path, cnf_path)
    };
    let _ = crate::ssh::session_exec_with_output(session, &format!("rm -f {}", cleanup), 5).await;

    if import_code != 0 {
        return Err(format!("Import failed: {}", import_stderr));
    }

    Ok(format!("Database {} imported successfully", db_name))
}

/// Import database from existing backup file
pub async fn import_database_from_backup(
    session: &SshSession,
    _cache: &SshCache,
    _session_id: &str,
    db_name: &str,
    db_user: &str,
    db_password: &str,
    backup_filename: &str,
) -> Result<String, String> {
    // Validate database name and backup filename
    if !db_name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err("Invalid database name".to_string());
    }
    
    if backup_filename.contains('/') || backup_filename.contains("..") {
        return Err("Invalid backup filename".to_string());
    }
    
    if !backup_filename.ends_with(".sql") && !backup_filename.ends_with(".tar.gz") && !backup_filename.ends_with(".zip") {
        return Err("Invalid backup file extension".to_string());
    }

    if db_user.is_empty() || db_password.is_empty() {
        return Err("数据库账号或密码为空，请先在本地保存密码".to_string());
    }

    let backup_path = format!("/tmp/db_backups/{}", backup_filename);
    
    // Verify backup file exists
    let verify_cmd = format!("test -f {} && echo 'exists'", backup_path);
    let (verify_out, _, verify_code) = crate::ssh::session_exec_with_output(session, &verify_cmd, 5)
        .await?;
    
    if verify_code != 0 || !verify_out.trim().contains("exists") {
        return Err("Backup file not found".to_string());
    }

    // If it's a tar.gz or zip file, extract it first
    let sql_path = if backup_filename.ends_with(".tar.gz") {
        let temp_sql = format!("/tmp/import_{}.sql", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| format!("Time error: {}", e))?
            .as_secs());
        
        let tar_cmd = format!("tar -xzf {} -O > {}", backup_path, temp_sql);
        let (_, stderr, code) = crate::ssh::session_exec_with_output(session, &tar_cmd, 60)
            .await?;
        
        if code != 0 {
            return Err(format!("Failed to extract tar.gz: {}", stderr));
        }
        temp_sql
    } else if backup_filename.ends_with(".zip") {
        let temp_sql = format!("/tmp/import_{}.sql", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| format!("Time error: {}", e))?
            .as_secs());
        
        let unzip_cmd = format!("unzip -p {} > {}", backup_path, temp_sql);
        let (_, stderr, code) = crate::ssh::session_exec_with_output(session, &unzip_cmd, 60)
            .await?;
        
        if code != 0 {
            return Err(format!("Failed to extract zip: {}", stderr));
        }
        temp_sql
    } else {
        backup_path.clone()
    };

    // Write temp credentials file to avoid shell special-character issues with inline passwords
    let cnf_path = write_mysql_cnf_file(session, db_user, db_password).await?;

    // Import SQL into database using credentials file (no password on command line)
    let import_cmd = format!(
        "mysql --defaults-extra-file={} {} < {}",
        cnf_path, db_name, sql_path
    );
    
    let (_import_stdout, import_stderr, import_code) = crate::ssh::session_exec_with_output(session, &import_cmd, 300)
        .await?;
    
    // Clean up extracted SQL file if it was a tar.gz or zip, and credentials file
    let cleanup_files = if backup_filename.ends_with(".tar.gz") || backup_filename.ends_with(".zip") {
        format!("{} {}", sql_path, cnf_path)
    } else {
        cnf_path.clone()
    };
    let _ = crate::ssh::session_exec_with_output(session, &format!("rm -f {}", cleanup_files), 5).await;
    
    if import_code != 0 {
        return Err(format!("Import failed: {}", import_stderr));
    }
    
    Ok(format!("Database {} imported from backup {} successfully", db_name, backup_filename))
}
