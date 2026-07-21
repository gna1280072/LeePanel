mod config;
mod db;
mod server;
mod ssh;
mod commands;

use rusqlite::Connection as SqliteConn;
use ssh::SshManager;
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex as AsyncMutex;

type DbPool = std::sync::Mutex<SqliteConn>;

// ===== App Entry =====

pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir { file_name: None }),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Webview),
                ])
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let ssh_mgr = Arc::new(AsyncMutex::new(SshManager::new()));
            app.manage(ssh_mgr);

            // Initialize SQLite database
            let db = db::init_db().expect("Failed to initialize database");
            app.manage(db);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // SSH
            commands::ssh::ssh_connect, commands::ssh::ssh_input, commands::ssh::ssh_resize, commands::ssh::ssh_disconnect,
            commands::ssh::ssh_get_cwd, commands::ssh::ssh_list_dir, commands::ssh::ssh_stat_file, commands::ssh::ssh_read_file, commands::ssh::ssh_write_file,
            commands::ssh::ssh_delete_file, commands::ssh::ssh_delete_files_batch, commands::ssh::ssh_create_dir, commands::ssh::ssh_rename_file, commands::ssh::ssh_rename_files_batch,
            commands::ssh::ssh_copy_file, commands::ssh::ssh_copy_files_batch, commands::ssh::ssh_copy_dir, commands::ssh::ssh_set_permissions, commands::ssh::ssh_set_permissions_batch,
            commands::ssh::ssh_check_space, commands::ssh::ssh_upload, commands::ssh::ssh_upload_chunk, commands::ssh::ssh_sftp_reset, commands::ssh::ssh_upload_files_batch, commands::ssh::ssh_create_dirs_batch, commands::ssh::ssh_exec, commands::ssh::ssh_download_file,
            commands::ssh::ssh_download_to_local, commands::ssh::ssh_save_as_local,
            commands::ssh::ssh_compress, commands::ssh::ssh_extract, commands::ssh::ssh_reconnect,
            commands::ssh::ssh_generate_keypair, commands::ssh::save_key_to_local,
            // Config
            commands::config::config_list, commands::config::config_save, commands::config::config_delete, commands::config::config_save_credentials,
            // Settings
            commands::config::settings_load, commands::config::settings_save,
            // Favorites
            commands::config::favorites_list, commands::config::favorites_add, commands::config::favorites_remove,
            // Server
            commands::server::server_get_system_info, commands::server::server_get_service_statuses,
            commands::server::server_get_service_info, commands::server::server_service_action,
            commands::server::server_read_remote_file, commands::server::server_write_remote_file,
            commands::server::server_get_log_lines, commands::server::server_test_nginx_config,
            commands::server::server_list_nginx_vhosts, commands::server::server_find_mysql_service,
            commands::server::server_find_php_service, commands::server::server_find_php_fpm_config,
            commands::server::server_mysql_processes, commands::server::server_mysql_query,
            commands::server::server_list_databases, commands::server::server_mysql_create_database, commands::server::server_mysql_delete_database,
            commands::server::server_mysql_clear_database,
            commands::server::server_mysql_change_db_access,
            commands::server::server_change_mysql_root_password,
            commands::server::server_change_db_user_password,
            // Redis
            commands::server::server_redis_check_status, commands::server::server_redis_get_version,
            commands::server::server_redis_dbsize_all, commands::server::server_redis_scan_keys,
            commands::server::server_redis_set_key, commands::server::server_redis_del_key,
            commands::server::server_redis_flushdb, commands::server::server_redis_save_backup, commands::server::server_redis_list_backups,
            commands::server::server_check_lnmp, commands::server::server_install_lnmp,
            // Sites
            commands::server_ops::server_list_sites, commands::server_ops::server_create_site,
            commands::server_ops::server_toggle_site,
            commands::server_ops::server_delete_site, commands::server_ops::server_update_site, commands::server_ops::server_update_site_full,
            commands::server_ops::server_save_site_config, commands::server_ops::server_set_hotlink_protection, commands::server_ops::server_set_reverse_proxy,
            commands::server_ops::server_list_php_versions, commands::server_ops::server_list_subdirs,
            commands::server_ops::server_setup_ssl, commands::server_ops::server_get_monitor_data,
            // Firewall
            commands::server_ops::server_firewall_list, commands::server_ops::server_firewall_add,
            commands::server_ops::server_firewall_remove, commands::server_ops::server_firewall_toggle,
            // Software
            commands::server_ops::server_get_software_list, commands::server_ops::server_get_available_php_versions, commands::server_ops::server_get_available_mysql_versions, commands::server_ops::server_software_action,
            commands::server_ops::server_get_removable_sources, commands::server_ops::server_remove_sources, commands::server_ops::server_clean_and_update_sources, commands::server_ops::server_add_source,
            // System Misc
            commands::server_ops::server_reboot, commands::server_ops::server_get_uptime,
            commands::server_ops::server_deploy_pubkey, commands::server_ops::server_get_ssh_auth_mode,
            commands::server_ops::server_set_ssh_auth_mode, commands::server_ops::server_get_bbr_status,
            commands::server_ops::server_set_bbr_status, commands::server_ops::server_get_site_logs,
            commands::server_ops::server_read_site_log,
            // File Browser
            commands::fb::fb_favorites_list, commands::fb::fb_favorites_add, commands::fb::fb_favorites_remove,
            commands::fb::fb_cache_get, commands::fb::fb_cache_put, commands::fb::fb_cache_touch, commands::fb::fb_cache_clear_all, commands::fb::fb_cache_count,
            commands::fb::ui_state_get, commands::fb::ui_state_set,
            // Docker
            commands::server_ops::server_check_docker, commands::server_ops::server_install_docker, commands::server_ops::server_uninstall_docker,
            commands::server_ops::server_docker_container_list, commands::server_ops::server_docker_container_action,
            commands::server_ops::server_docker_container_remove, commands::server_ops::server_docker_container_logs,
            commands::server_ops::server_docker_image_list, commands::server_ops::server_docker_image_pull, commands::server_ops::server_docker_image_remove, commands::server_ops::server_docker_image_run,
            commands::server_ops::server_docker_get_mirror_config, commands::server_ops::server_docker_set_mirror_config,
            // Cache
            commands::server_ops::server_cache_invalidate,
            // Database Remarks
            commands::server::server_save_db_remark, commands::server::server_get_db_remarks,
            // Database Credentials
            commands::server::server_save_db_credentials, commands::server::server_get_db_credentials,
            commands::server::server_get_db_credential, commands::server::server_update_db_credential_password,
            // Database Backup & Import
            commands::server::server_backup_database, commands::server::server_list_db_backups, commands::server::server_delete_db_backup,
            commands::server::server_download_db_backup, commands::server::server_save_db_backup_to_local,
            commands::server::server_import_database_from_file, commands::server::server_import_database_from_file_bytes, commands::server::server_import_database_from_backup,
            // Custom Software
            commands::server_ops::custom_software_list, commands::server_ops::custom_software_add, commands::server_ops::custom_software_remove, commands::server_ops::custom_software_action,
            commands::server_ops::server_check_installation,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
