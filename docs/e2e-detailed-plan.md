# LeePanel E2E 测试详细方案

## 一、项目 Invoke 命令全清单（共 143 个）

### SSH 基础操作（33 个）
| # | 命令 | 远端 shell | E2E 容器需求 |
|---|---|---|---|
| 1 | `ssh_connect` | SSH 握手 | 任意 SSH 容器 |
| 2 | `ssh_input` | PTY 输入 | 任意 SSH 容器 |
| 3 | `ssh_resize` | PTY resize | 任意 SSH 容器 |
| 4 | `ssh_disconnect` | 断开连接 | 任意 SSH 容器 |
| 5 | `ssh_get_cwd` | `pwd` | 任意 SSH 容器 |
| 6 | `ssh_list_dir` | `ls -la` | 任意 SSH 容器 |
| 7 | `ssh_stat_file` | `stat` | 任意 SSH 容器 |
| 8 | `ssh_read_file` | `cat` | 任意 SSH 容器 |
| 9 | `ssh_write_file` | SFTP write | 任意 SSH 容器 |
| 10 | `ssh_delete_file` | `rm -rf` | 任意 SSH 容器 |
| 11 | `ssh_delete_files_batch` | `rm -rf` 批量 | 任意 SSH 容器 |
| 12 | `ssh_create_dir` | `mkdir -p` | 任意 SSH 容器 |
| 13 | `ssh_rename_file` | `mv` | 任意 SSH 容器 |
| 14 | `ssh_rename_files_batch` | `mv` 批量 | 任意 SSH 容器 |
| 15 | `ssh_copy_files_batch` | `cp/mv` | 任意 SSH 容器 |
| 16 | `ssh_set_permissions_batch` | `chmod` 批量 | 任意 SSH 容器 |
| 17 | `ssh_copy_file` | `cp` | 任意 SSH 容器 |
| 18 | `ssh_copy_dir` | `cp -r` | 任意 SSH 容器 |
| 19 | `ssh_set_permissions` | `chmod` | 任意 SSH 容器 |
| 20 | `ssh_check_space` | `df -h` | 任意 SSH 容器 |
| 21 | `ssh_upload` | SFTP | 任意 SSH 容器 |
| 22 | `ssh_upload_chunk` | SFTP | 任意 SSH 容器 |
| 23 | `ssh_sftp_reset` | 重建 SFTP | 任意 SSH 容器 |
| 24 | `ssh_upload_files_batch` | SFTP 批量 | 任意 SSH 容器 |
| 25 | `ssh_create_dirs_batch` | `mkdir -p` 批量 | 任意 SSH 容器 |
| 26 | `ssh_exec` | 任意命令 | 任意 SSH 容器 |
| 27 | `ssh_download_file` | curl/wget | 任意 SSH 容器 |
| 28 | `ssh_download_to_local` | SFTP read | 任意 SSH 容器 |
| 29 | `ssh_save_as_local` | SFTP read + 本地 dialog | Tauri 依赖，需 mock dialog |
| 30 | `ssh_compress` | `tar/zip` | 任意 SSH 容器 |
| 31 | `ssh_extract` | `tar/zip/unzip` | 任意 SSH 容器 |
| 32 | `ssh_reconnect` | 断开+重连 | 任意 SSH 容器 |
| 33 | `ssh_generate_keypair` | 纯本地（ssh-keygen） | 无需容器 |

### Config/Settings/Favorites（10 个）
| # | 命令 | 远端 shell | E2E 容器需求 |
|---|---|---|---|
| 34 | `config_list` | 无（本地 SQLite） | 无需容器 |
| 35 | `config_save` | 无（本地 SQLite） | 无需容器 |
| 36 | `config_delete` | 无（本地 SQLite） | 无需容器 |
| 37 | `config_save_credentials` | 无（本地 SQLite） | 无需容器 |
| 38 | `settings_load` | 无（本地 SQLite） | 无需容器 |
| 39 | `settings_save` | 无（本地 SQLite） | 无需容器 |
| 40 | `favorites_list` | 无（本地 SQLite） | 无需容器 |
| 41 | `favorites_add` | 无（本地 SQLite） | 无需容器 |
| 42 | `favorites_remove` | 无（本地 SQLite） | 无需容器 |
| 43 | `save_key_to_local` | 本地文件写入 + dialog | Tauri 依赖，需 mock dialog |

### Dashboard/系统信息（4 个）
| # | 命令 | 远端 shell | E2E 容器需求 |
|---|---|---|---|
| 44 | `server_get_system_info` | `uname`, `free`, `df`, `top` 等 | SSH 容器（Ubuntu） |
| 45 | `server_get_service_statuses` | `systemctl` | SSH 容器（需 systemd 或模拟） |
| 46 | `server_get_uptime` | `uptime` | SSH 容器 |
| 47 | `server_get_monitor_data` | `top`, `df`, `iostat` 等 | SSH 容器 |

### Nginx/站点管理（14 个）
| # | 命令 | 远端 shell | E2E 容器需求 |
|---|---|---|---|
| 48 | `server_test_nginx_config` | `nginx -t` | Nginx 容器 |
| 49 | `server_list_nginx_vhosts` | `ls /etc/nginx/` | Nginx 容器 |
| 50 | `server_list_sites` | Nginx vhost 解析 | Nginx 容器 |
| 51 | `server_create_site` | 创建 Nginx config + 目录 | Nginx 容器 |
| 52 | `server_toggle_site` | enable/disable site | Nginx 容器 |
| 53 | `server_delete_site` | 删除 config + 可选删除文件 | Nginx 容器 |
| 54 | `server_update_site` | 修改 Nginx config | Nginx 容器 |
| 55 | `server_update_site_full` | 修改 Nginx config（防盗链+反代） | Nginx 容器 |
| 56 | `server_save_site_config` | 写入 config 文件 | Nginx 容器 |
| 57 | `server_set_hotlink_protection` | 修改 Nginx config | Nginx 容器 |
| 58 | `server_set_reverse_proxy` | 修改 Nginx config | Nginx 容器 |
| 59 | `server_list_php_versions` | `ls /usr/sbin/php-fpm*` | PHP 容器 |
| 60 | `server_list_subdirs` | `find -type d` | SSH 容器 |
| 61 | `server_setup_ssl` | `certbot` / 自签证书 | Nginx + certbot 容器 |

### MySQL/MariaDB 数据库（19 个）
| # | 命令 | 远端 shell | E2E 容器需求 |
|---|---|---|---|
| 62 | `server_find_mysql_service` | `systemctl list-units` | MySQL 容器 |
| 63 | `server_mysql_processes` | `SHOW PROCESSLIST` | MySQL 容器 |
| 64 | `server_mysql_query` | `mysql -e` | MySQL 容器 |
| 65 | `server_list_databases` | `SHOW DATABASES` | MySQL 容器 |
| 66 | `server_mysql_create_database` | `CREATE DATABASE` + 用户 | MySQL 容器 |
| 67 | `server_mysql_delete_database` | `DROP DATABASE` + 用户 | MySQL 容器 |
| 68 | `server_mysql_clear_database` | 存储过程 TRUNCATE | MySQL 容器 |
| 69 | `server_mysql_change_db_access` | `GRANT/REVOKE` | MySQL 容器 |
| 70 | `server_change_mysql_root_password` | `ALTER USER` | MySQL 容器 |
| 71 | `server_change_db_user_password` | `ALTER USER` | MySQL 容器 |
| 72 | `server_backup_database` | `mysqldump` | MySQL 容器 |
| 73 | `server_list_db_backups` | `ls /root/db_backup/` | MySQL 容器 |
| 74 | `server_delete_db_backup` | `rm` | MySQL 容器 |
| 75 | `server_download_db_backup` | SFTP read | MySQL 容器 |
| 76 | `server_save_db_backup_to_local` | SFTP + dialog | Tauri 依赖 |
| 77 | `server_import_database_from_file` | `mysql <` | MySQL 容器 |
| 78 | `server_import_database_from_file_bytes` | `mysql <` | MySQL 容器 |
| 79 | `server_import_database_from_backup` | `mysql <` | MySQL 容器 |
| 80 | `server_find_php_service` | `systemctl` | PHP 容器 |

### 数据库备注/凭据（6 个）
| # | 命令 | 远端 shell | E2E 容器需求 |
|---|---|---|---|
| 81 | `server_save_db_remark` | SSH + 本地 SQLite | MySQL 容器 |
| 82 | `server_get_db_remarks` | 本地 SQLite | 无需容器 |
| 83 | `server_save_db_credentials` | 本地 SQLite | 无需容器 |
| 84 | `server_get_db_credentials` | 本地 SQLite | 无需容器 |
| 85 | `server_get_db_credential` | 本地 SQLite | 无需容器 |
| 86 | `server_update_db_credential_password` | 本地 SQLite | 无需容器 |

### Redis（9 个）
| # | 命令 | 远端 shell | E2E 容器需求 |
|---|---|---|---|
| 87 | `server_redis_check_status` | `systemctl is-active redis` | Redis 容器 |
| 88 | `server_redis_get_version` | `redis-cli INFO` | Redis 容器 |
| 89 | `server_redis_dbsize_all` | `redis-cli DBSIZE` | Redis 容器 |
| 90 | `server_redis_scan_keys` | `redis-cli SCAN` | Redis 容器 |
| 91 | `server_redis_set_key` | `redis-cli SET` | Redis 容器 |
| 92 | `server_redis_del_key` | `redis-cli DEL` | Redis 容器 |
| 93 | `server_redis_flushdb` | `redis-cli FLUSHDB` | Redis 容器 |
| 94 | `server_redis_save_backup` | `redis-cli BGSAVE` + 备份 | Redis 容器 |
| 95 | `server_redis_list_backups` | `ls /root/redis_backup/` | Redis 容器 |

### Docker 管理（12 个）
| # | 命令 | 远端 shell | E2E 容器需求 |
|---|---|---|---|
| 96 | `server_check_docker` | `docker info` | **Docker-in-Docker** ⚠️ |
| 97 | `server_install_docker` | `curl get.docker.com` | Docker-in-Docker |
| 98 | `server_uninstall_docker` | `apt remove docker` | Docker-in-Docker |
| 99 | `server_docker_container_list` | `docker ps` | Docker-in-Docker |
| 100 | `server_docker_container_action` | `docker start/stop/restart` | Docker-in-Docker |
| 101 | `server_docker_container_remove` | `docker rm` | Docker-in-Docker |
| 102 | `server_docker_container_logs` | `docker logs` | Docker-in-Docker |
| 103 | `server_docker_image_list` | `docker images` | Docker-in-Docker |
| 104 | `server_docker_image_pull` | `docker pull` | Docker-in-Docker |
| 105 | `server_docker_image_remove` | `docker rmi` | Docker-in-Docker |
| 106 | `server_docker_image_run` | `docker run` | Docker-in-Docker |
| 107 | `server_docker_get_mirror_config` | `cat /etc/docker/daemon.json` | Docker-in-Docker |
| 108 | `server_docker_set_mirror_config` | 写入 daemon.json | Docker-in-Docker |

### 软件安装/LNMP（9 个）
| # | 命令 | 远端 shell | E2E 容器需求 |
|---|---|---|---|
| 109 | `server_check_lnmp` | `nginx -V`, `mysql -V`, `php -v` | SSH 容器 |
| 110 | `server_install_lnmp` | `apt install` | SSH 容器（**5-10分钟**） |
| 111 | `server_get_software_list` | `dpkg -l` / `rpm -qa` | SSH 容器 |
| 112 | `server_get_available_php_versions` | `apt-cache search` | SSH 容器 |
| 113 | `server_get_available_mysql_versions` | `apt-cache search` | SSH 容器 |
| 114 | `server_software_action` | `apt install/remove` | SSH 容器（**5-30分钟**） |
| 115 | `server_get_removable_sources` | `ls /etc/apt/sources.list.d/` | SSH 容器 |
| 116 | `server_remove_sources` | `rm sources.list.d/` | SSH 容器 |
| 117 | `server_clean_and_update_sources` | `apt clean && apt update` | SSH 容器 |
| 118 | `server_add_source` | 写入 sources.list.d + gpg | SSH 容器 |
| 119 | `server_find_php_fpm_config` | `find php-fpm.conf` | PHP 容器 |

### 防火墙/BBR/SSH 配置（7 个）
| # | 命令 | 远端 shell | E2E 容器需求 |
|---|---|---|---|
| 120 | `server_firewall_list` | `iptables -L` / `ufw` | **需 privileged 容器** ⚠️ |
| 121 | `server_firewall_add` | `iptables/ufw` | privileged 容器 |
| 122 | `server_firewall_remove` | `iptables/ufw` | privileged 容器 |
| 123 | `server_firewall_toggle` | `ufw enable/disable` | privileged 容器 |
| 124 | `server_get_bbr_status` | `sysctl net.ipv4.tcp_congestion_control` | **需内核访问** ⚠️ |
| 125 | `server_set_bbr_status` | `sysctl -w` | 需内核访问 |
| 126 | `server_get_ssh_auth_mode` | `cat /etc/ssh/sshd_config` | SSH 容器 |
| 127 | `server_set_ssh_auth_mode` | 修改 sshd_config + restart | SSH 容器 |
| 128 | `server_deploy_pubkey` | 写入 `~/.ssh/authorized_keys` | SSH 容器 |
| 129 | `server_reboot` | `reboot` / `shutdown` | SSH 容器（**会断连**） |

### 站点日志（2 个）
| # | 命令 | 远端 shell | E2E 容器需求 |
|---|---|---|---|
| 130 | `server_get_site_logs` | `ls /var/log/nginx/` | Nginx 容器 |
| 131 | `server_read_site_log` | `tail/head/grep` | Nginx 容器 |

### 文件浏览器 SQLite 缓存（8 个）
| # | 命令 | 远端 shell | E2E 容器需求 |
|---|---|---|---|
| 132 | `fb_favorites_list` | 本地 SQLite | 无需容器 |
| 133 | `fb_favorites_add` | 本地 SQLite | 无需容器 |
| 134 | `fb_favorites_remove` | 本地 SQLite | 无需容器 |
| 135 | `fb_cache_get` | 本地 SQLite | 无需容器 |
| 136 | `fb_cache_put` | 本地 SQLite | 无需容器 |
| 137 | `fb_cache_touch` | 本地 SQLite | 无需容器 |
| 138 | `fb_cache_clear_all` | 本地 SQLite | 无需容器 |
| 139 | `fb_cache_count` | 本地 SQLite | 无需容器 |

### UI 状态/自定义软件/安装检测（8 个）
| # | 命令 | 远端 shell | E2E 容器需求 |
|---|---|---|---|
| 140 | `ui_state_get` | 本地 SQLite | 无需容器 |
| 141 | `ui_state_set` | 本地 SQLite | 无需容器 |
| 142 | `custom_software_list` | `dpkg -s` | SSH 容器 |
| 143 | `custom_software_add` | 本地 SQLite | 无需容器 |
| 144 | `custom_software_remove` | 本地 SQLite | 无需容器 |
| 145 | `custom_software_action` | `systemctl` | SSH 容器 |
| 146 | `server_check_installation` | `pgrep`, `cat /tmp/leepanel-install.*` | SSH 容器 |
| 147 | `server_cache_invalidate` | 本地缓存清除 | 无需容器 |
| 148 | `server_read_remote_file` | `cat` | SSH 容器 |
| 149 | `server_write_remote_file` | SFTP write | SSH 容器 |
| 150 | `server_get_log_lines` | `tail -n` | SSH 容器 |

> 注：实际去重后约 143 个独立 invoke 命令

## 二、容器分层策略

### 第一层：不需要容器的命令（30 个，占 21%）

这些命令只操作本地 SQLite 或本地文件，E2E 直接测试：

```
config_list, config_save, config_delete, config_save_credentials,
settings_load, settings_save,
favorites_list, favorites_add, favorites_remove,
fb_favorites_list, fb_favorites_add, fb_favorites_remove,
fb_cache_get, fb_cache_put, fb_cache_touch, fb_cache_clear_all, fb_cache_count,
ui_state_get, ui_state_set,
server_get_db_remarks,
server_save_db_credentials, server_get_db_credentials,
server_get_db_credential, server_update_db_credential_password,
custom_software_add, custom_software_remove,
server_cache_invalidate,
ssh_generate_keypair,
save_key_to_local（mock dialog）
```

**Mock Proxy 处理**：直接操作本地 SQLite 文件，不走 SSH。

### 第二层：基础 SSH 容器（48 个，占 33%）

一个 `ubuntu:22.04 + openssh` 容器即可覆盖：

```
ssh_connect, ssh_input, ssh_resize, ssh_disconnect,
ssh_get_cwd, ssh_list_dir, ssh_stat_file, ssh_read_file, ssh_write_file,
ssh_delete_file, ssh_delete_files_batch, ssh_create_dir, ssh_rename_file,
ssh_rename_files_batch, ssh_copy_files_batch, ssh_set_permissions_batch,
ssh_copy_file, ssh_copy_dir, ssh_set_permissions, ssh_check_space,
ssh_upload, ssh_upload_chunk, ssh_sftp_reset, ssh_upload_files_batch,
ssh_create_dirs_batch, ssh_exec, ssh_download_file, ssh_download_to_local,
ssh_compress, ssh_extract, ssh_reconnect,
server_get_system_info, server_get_uptime, server_get_monitor_data,
server_check_lnmp, server_list_subdirs,
server_get_software_list, server_get_available_php_versions,
server_get_available_mysql_versions,
server_get_removable_sources, server_remove_sources,
server_clean_and_update_sources, server_add_source,
server_get_ssh_auth_mode, server_set_ssh_auth_mode, server_deploy_pubkey,
server_read_remote_file, server_write_remote_file, server_get_log_lines,
server_check_installation, server_reboot
```

### 第三层：MySQL 容器（19 个，占 13%）

`mysql:8.0` 官方镜像 + SSH 访问：

```
server_find_mysql_service, server_mysql_processes, server_mysql_query,
server_list_databases, server_mysql_create_database, server_mysql_delete_database,
server_mysql_clear_database, server_mysql_change_db_access,
server_change_mysql_root_password, server_change_db_user_password,
server_backup_database, server_list_db_backups, server_delete_db_backup,
server_download_db_backup, server_save_db_backup_to_local,
server_import_database_from_file, server_import_database_from_file_bytes,
server_import_database_from_backup, server_save_db_remark
```

### 第四层：Nginx + PHP 容器（17 个，占 12%）

`ubuntu:22.04 + nginx + php-fpm`：

```
server_test_nginx_config, server_list_nginx_vhosts, server_list_sites,
server_create_site, server_toggle_site, server_delete_site,
server_update_site, server_update_site_full, server_save_site_config,
server_set_hotlink_protection, server_set_reverse_proxy,
server_list_php_versions, server_find_php_service, server_find_php_fpm_config,
server_setup_ssl, server_get_site_logs, server_read_site_log
```

### 第五层：Redis 容器（9 个，占 6%）

`redis:7` 官方镜像 + SSH 访问：

```
server_redis_check_status, server_redis_get_version,
server_redis_dbsize_all, server_redis_scan_keys,
server_redis_set_key, server_redis_del_key, server_redis_flushdb,
server_redis_save_backup, server_redis_list_backups
```

### 第六层：无法完全覆盖（13 个，占 9%）

```
Docker 管理（12 个）—— Docker-in-Docker 不稳定
防火墙/BBR（6 个）—— 需 privileged/内核访问
```

**替代方案**：E2E 只验证 UI 渲染（mock 返回假数据），不验证真实操作。

## 三、Mock Invoke Proxy 架构

```
┌──────────────────┐     HTTP POST      ┌──────────────────┐
│   Playwright     │ ──────────────────▶│   Vite Dev       │
│   (Chromium)     │                    │   Server :5173   │
│                  │◀──────────────────│                  │
└──────────────────┘   DOM / UI        └──────┬───────────┘
                                              │ invoke() 调用
                                              ▼
                                     ┌──────────────────┐
                                     │   Mock Proxy     │
                                     │   :3333          │
                                     └────┬───┬───┬────┘
                                          │   │   │
                    ┌─────────────────────┘   │   └─────────────────────┐
                    ▼                         ▼                         ▼
            ┌──────────────┐    ┌──────────────────┐    ┌──────────────────┐
            │ 本地 SQLite  │    │  ubuntu:22.04    │    │  mysql:8.0       │
            │ (config/db)  │    │  SSH :2222       │    │  SSH :3306+2223  │
            └──────────────┘    └──────────────────┘    └──────────────────┘
                                          │
                                ┌─────────┼─────────┐
                                ▼                   ▼
                        ┌──────────────┐    ┌──────────────┐
                        │ nginx+php    │    │  redis:7     │
                        │ SSH :2224    │    │  SSH :2225   │
                        └──────────────┘    └──────────────┘
```

### Proxy 路由规则

```typescript
// mock-proxy/router.ts — 伪代码
const routes: Record<string, Handler> = {
  // SSH 连接管理（proxy 自己维护 SSH session pool）
  'ssh_connect': async (args) => {
    const session = await ssh2Connect(args.config.host, args.config.port, ...)
    return session.id
  },
  
  // 透传到 SSH 容器
  'ssh_list_dir': async (args) => sshExec(args.sessionId, `ls -la ${args.path}`),
  'server_list_databases': async (args) => sshExec(args.sessionId, `mysql -e 'SHOW DATABASES'`),
  
  // 本地 SQLite 操作
  'config_list': async () => sqliteQuery('SELECT * FROM connections'),
  'settings_load': async () => sqliteQuery('SELECT * FROM settings'),
  
  // Tauri 专属功能 mock
  'ssh_save_as_local': async () => ({ error: 'Not supported in E2E' }),
  'save_key_to_local': async () => ({ error: 'Not supported in E2E' }),
}
```

### 关键设计：Proxy 需要复刻 Rust 命令的 shell 逻辑

Proxy 的核心价值在于：**把 Rust 代码中拼装 SSH 命令的逻辑，用 Node.js 重新实现一遍**。

这不是简单的 mock（返回假数据），而是 **真正的 SSH 命令透传**：

```typescript
// 示例：server_list_databases 的 proxy 实现
async function serverListDatabases(sessionId: string) {
  const ssh = getSession(sessionId)  // proxy 维护的 SSH 连接池
  
  // 复刻 server.rs 中 list_databases() 的 shell 命令
  const sql = `SELECT schema_name, 
    (SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = schema_name) as table_count
    FROM information_schema.schemata 
    WHERE schema_name NOT IN ('information_schema','mysql','performance_schema','sys')`
  
  const result = await ssh.exec(`mysql -N -e "${sql}"`)
  return parseDbList(result.stdout)  // 复刻 server.rs 中的解析逻辑
}
```

## 四、docker-compose.yml 设计

```yaml
version: '3.8'

services:
  # 基础 Ubuntu SSH 服务器
  ubuntu-ssh:
    build: ./e2e/containers/ubuntu
    ports:
      - "2222:22"
    environment:
      - SSH_USER=testuser
      - SSH_PASS=testpass123

  # MySQL 8.0 + SSH
  mysql-ssh:
    build: ./e2e/containers/mysql
    ports:
      - "2223:22"
      - "3306:3306"
    environment:
      - MYSQL_ROOT_PASSWORD=rootpass123
      - SSH_USER=testuser
      - SSH_PASS=testpass123

  # Nginx + PHP-FPM + SSH
  nginx-ssh:
    build: ./e2e/containers/nginx
    ports:
      - "2224:22"
      - "8080:80"
    environment:
      - SSH_USER=testuser
      - SSH_PASS=testpass123

  # Redis + SSH
  redis-ssh:
    build: ./e2e/containers/redis
    ports:
      - "2225:22"
      - "6379:6379"
    environment:
      - SSH_USER=testuser
      - SSH_PASS=testpass123
```

### 容器 Dockerfile 示例（Ubuntu SSH）

```dockerfile
FROM ubuntu:22.04

RUN apt-get update && apt-get install -y \
    openssh-server curl wget tar zip unzip \
    && mkdir -p /run/sshd \
    && echo 'root:testpass123' | chpasswd \
    && sed -i 's/#PermitRootLogin.*/PermitRootLogin yes/' /etc/ssh/sshd_config \
    && sed -i 's/#PasswordAuthentication.*/PasswordAuthentication yes/' /etc/ssh/sshd_config

# 预置测试文件
RUN mkdir -p /home/test/docs /var/www/html \
    && echo "hello world" > /home/test/hello.txt \
    && echo '{"name":"test"}' > /home/test/config.json

EXPOSE 22
CMD ["/usr/sbin/sshd", "-D"]
```

## 五、Playwright 测试用例设计

### 测试文件结构

```
e2e/
├── containers/              # Docker 容器 Dockerfile
│   ├── ubuntu/Dockerfile
│   ├── mysql/Dockerfile
│   ├── nginx/Dockerfile
│   └── redis/Dockerfile
├── mock-proxy/              # Mock Invoke Proxy
│   ├── server.ts            # Express 服务器
│   ├── router.ts            # 命令路由
│   ├── ssh-pool.ts          # SSH 连接池
│   └── handlers/            # 各命令处理器
│       ├── ssh.ts
│       ├── server.ts
│       ├── database.ts
│       ├── sites.ts
│       ├── redis.ts
│       └── config.ts
├── tests/                   # Playwright 测试
│   ├── connection.spec.ts   # 连接/断开
│   ├── dashboard.spec.ts    # 系统信息
│   ├── files.spec.ts        # 文件浏览器
│   ├── database.spec.ts     # 数据库管理
│   ├── sites.spec.ts        # 站点管理
│   ├── redis.spec.ts        # Redis 管理
│   ├── docker.spec.ts       # Docker 管理（mock）
│   ├── software.spec.ts     # 软件安装（mock 长操作）
│   ├── firewall.spec.ts     # 防火墙（mock）
│   ├── settings.spec.ts     # 设置面板
│   ├── i18n.spec.ts         # 多语言切换
│   └── upload.spec.ts       # 文件上传
├── playwright.config.ts
└── docker-compose.yml
```

### 测试用例示例

```typescript
// e2e/tests/files.spec.ts
test('文件浏览器：列出目录', async ({ page }) => {
  // 1. 连接到 SSH 容器
  await connectToServer(page, 'ubuntu-ssh')
  
  // 2. 导航到文件面板
  await page.click('[data-section="files"]')
  
  // 3. 等待目录列表加载
  await page.waitForSelector('.fb-file-row')
  
  // 4. 验证预置文件存在
  await expect(page.locator('.fb-file-row')).toContainText('hello.txt')
})

test('文件操作：创建→重命名→删除', async ({ page }) => {
  await connectToServer(page, 'ubuntu-ssh')
  await page.click('[data-section="files"]')
  
  // 创建目录
  await page.click('.fb-btn-new-folder')
  await page.fill('.fb-dialog-input', 'test-dir')
  await page.click('.fb-dialog-btn:has-text("Create")')
  await expect(page.locator('.fb-file-row')).toContainText('test-dir')
  
  // 重命名
  await page.click('.fb-file-row:has-text("test-dir") .fb-action-rename')
  await page.fill('.fb-dialog-input', 'renamed-dir')
  await page.click('.fb-dialog-btn:has-text("Rename")')
  await expect(page.locator('.fb-file-row')).toContainText('renamed-dir')
  
  // 删除
  await page.click('.fb-file-row:has-text("renamed-dir") .fb-action-delete')
  await page.click('.fb-dialog-btn:has-text("Delete")')
  await expect(page.locator('.fb-file-row')).not.toContainText('renamed-dir')
})
```

## 六、实施优先级

### Phase 1：基础设施（预计 2 天）
1. 写 docker-compose.yml + 4 个容器 Dockerfile
2. 写 Mock Invoke Proxy（Express + SSH2 连接池）
3. Proxy 实现 30 个本地 SQLite 命令的 handler
4. 配置 Playwright + Vite 集成
5. 验证：`ssh_connect → ssh_list_dir → ssh_disconnect` 完整链路

### Phase 2：核心功能测试（预计 3 天）
1. 连接管理（创建/编辑/删除/重连）
2. 文件浏览器（CRUD + 上传 + 压缩解压）
3. Dashboard 系统信息展示
4. 终端 xterm 基本交互

### Phase 3：数据库 + 站点（预计 3 天）
1. MySQL CRUD（创建/删除/清空/导入/备份）
2. 站点管理（创建/编辑/删除/SSL）
3. Redis 管理（key 操作/备份）

### Phase 4：剩余功能 + Mock 覆盖（预计 2 天）
1. Docker 管理（mock 返回假数据，验证 UI）
2. 防火墙/BBR（mock 返回假数据，验证 UI）
3. 软件安装（验证触发命令，mock 完成状态）
4. i18n 多语言截图对比
5. 设置面板

### 总计：约 10 个工作日

## 七、依赖清单

| 依赖 | 用途 | 安装命令 |
|---|---|---|
| `@playwright/test` | E2E 测试框架 | `npm i -D @playwright/test` |
| `express` | Mock Proxy 服务器 | `npm i express` |
| `ssh2` | Node.js SSH 客户端 | `npm i ssh2` |
| `better-sqlite3` | Proxy 本地 SQLite | `npm i better-sqlite3` |
| `tsx` | 运行 TS proxy | `npm i -D tsx` |

## 八、CI 集成（GitHub Actions）

```yaml
name: E2E Tests

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  e2e:
    runs-on: ubuntu-latest
    timeout-minutes: 15
    
    steps:
      - uses: actions/checkout@v4
      
      - uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: npm
      
      - run: npm ci
      - run: npx playwright install --with-deps chromium
      
      - name: Start Docker containers
        run: docker compose -f e2e/docker-compose.yml up -d --build
        timeout-minutes: 3
      
      - name: Wait for SSH
        run: |
          for port in 2222 2223 2224 2225; do
            for i in $(seq 1 30); do
              nc -z localhost $port && break
              sleep 1
            done
          done
      
      - name: Start Mock Proxy
        run: npx tsx e2e/mock-proxy/server.ts &
      
      - name: Start Vite Dev Server
        run: npx vite --port 5173 &
      
      - name: Run E2E Tests
        run: npx playwright test
        env:
          MOCK_PROXY_URL: http://localhost:3333
      
      - name: Upload test results
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: playwright-report
          path: playwright-report/
      
      - name: Print container logs on failure
        if: failure()
        run: docker compose -f e2e/docker-compose.yml logs
```
