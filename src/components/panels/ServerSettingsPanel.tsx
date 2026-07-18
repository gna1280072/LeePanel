import { useState, useEffect, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { getVersion } from '@tauri-apps/api/app'
import { check } from '@tauri-apps/plugin-updater'
import { useTranslation } from 'react-i18next'

interface SshKeyPair {
  private_key_pem: string
  public_key_openssh: string
}

interface SshAuthMode {
  password: boolean
  pubkey: boolean
}

interface AppSettings {
  auto_reconnect: boolean
  reconnect_interval: number
  max_reconnect_attempts: number
  cache_ttl_hours: number
  cache_max_files: number
  cache_enabled: boolean
  command_timeout_minutes: number
  upload_workers: number
}

interface ServerSettingsPanelProps {
  sessionId: string | null
  appSettings?: AppSettings
  onToggleAutoReconnect?: () => void
  onUpdateSettings?: (settings: Partial<AppSettings>) => Promise<void>
}

export default function ServerSettingsPanel({ sessionId, appSettings, onToggleAutoReconnect, onUpdateSettings }: ServerSettingsPanelProps) {
  const { t } = useTranslation()
  // Reboot state
  const [rebootLoading, setRebootLoading] = useState(false)
  const [rebootConfirm, setRebootConfirm] = useState<{ show: boolean; force: boolean }>({ show: false, force: false })
  const [rebootExecPanel, setRebootExecPanel] = useState<{ show: boolean; logs: string[]; status: 'running' | 'done' | 'error' }>({ show: false, logs: [], status: 'running' })

  // SSH Auth mode state
  const [authMode, setAuthMode] = useState<SshAuthMode | null>(null)
  const [authModeLoading, setAuthModeLoading] = useState(false)
  const [authModeSaving, setAuthModeSaving] = useState(false)

  // SSH Key generation state
  const [keyAlgorithm, setKeyAlgorithm] = useState('ed25519')
  const [keyPair, setKeyPair] = useState<SshKeyPair | null>(null)
  const [keyGenLoading, setKeyGenLoading] = useState(false)
  const [keyDeployLoading, setKeyDeployLoading] = useState(false)
  const [keyMessage, setKeyMessage] = useState('')
  const [keyError, setKeyError] = useState('')

  // App settings editing state
  const [reconnectIntervalInput, setReconnectIntervalInput] = useState<string>('')
  const [maxAttemptsInput, setMaxAttemptsInput] = useState<string>('')
  const [cacheLimitInput, setCacheLimitInput] = useState<string>('')
  const [cacheMaxFilesInput, setCacheMaxFilesInput] = useState<string>('')
  const [commandTimeoutInput, setCommandTimeoutInput] = useState<string>('')
  const [uploadWorkersInput, setUploadWorkersInput] = useState<string>('')
  const [settingsSaving, setSettingsSaving] = useState(false)

  // Update check state
  const [updateChecking, setUpdateChecking] = useState(false)
  const [updateMessage, setUpdateMessage] = useState('')
  const [appVersion, setAppVersion] = useState('')

  // Fetch app version
  useEffect(() => {
    getVersion().then(setAppVersion).catch(() => {})
  }, [])

  const handleCheckUpdate = async () => {
    setUpdateChecking(true)
    setUpdateMessage('')
    try {
      const update = await check()
      if (update?.available) {
        setUpdateMessage(`New version ${update.version} found, downloading...`)
        await update.downloadAndInstall()
        setUpdateMessage(`Version ${update.version} installed, restarting...`)
      } else {
        setUpdateMessage('You are on the latest version')
      }
    } catch (e) {
      setUpdateMessage(`Update check failed: ${String(e).slice(0, 100)}`)
    } finally {
      setUpdateChecking(false)
    }
  }

  // Cache management state
  const [cacheCount, setCacheCount] = useState<number>(0)
  const [cacheClearing, setCacheClearing] = useState(false)

  // Uptime state
  const [bootTime, setBootTime] = useState('')
  const [uptimeDuration, setUptimeDuration] = useState('')

  // Fetch uptime
  const fetchUptime = useCallback(async () => {
    if (!sessionId) return
    try {
      const [boot, duration] = await invoke<[string, string]>('server_get_uptime', { sessionId })
      setBootTime(boot)
      setUptimeDuration(duration)
    } catch {
      // ignore
    }
  }, [sessionId])

  // Fetch SSH auth mode
  const fetchAuthMode = useCallback(async () => {
    if (!sessionId) return
    setAuthModeLoading(true)
    try {
      const mode = await invoke<SshAuthMode>('server_get_ssh_auth_mode', { sessionId })
      setAuthMode(mode)
    } catch {
      // ignore
    } finally {
      setAuthModeLoading(false)
    }
  }, [sessionId])

  // ponytail: fetch cache count
  const fetchCacheCount = useCallback(async () => {
    try {
      const count = await invoke<number>('fb_cache_count')
      setCacheCount(count)
    } catch { /* ignore */ }
  }, [])

  // ponytail: clear all directory cache
  const handleClearCache = useCallback(async () => {
    setCacheClearing(true)
    try {
      await invoke('fb_cache_clear_all')
      setCacheCount(0)
    } catch { /* ignore */ } finally {
      setCacheClearing(false)
    }
  }, [])

  useEffect(() => {
    fetchAuthMode()
    fetchUptime()
    fetchCacheCount()
  }, [fetchAuthMode, fetchUptime, fetchCacheCount])

  // Sync input values when appSettings changes
  useEffect(() => {
    if (appSettings) {
      setReconnectIntervalInput(String(appSettings.reconnect_interval))
      setMaxAttemptsInput(String(appSettings.max_reconnect_attempts))
      setCacheLimitInput(String(appSettings.cache_ttl_hours))
      setCacheMaxFilesInput(String(appSettings.cache_max_files))
      setCommandTimeoutInput(String(appSettings.command_timeout_minutes))
        setUploadWorkersInput(String(appSettings.upload_workers))
    }
  }, [appSettings])

  // Save settings
  const handleSaveSettings = async () => {
    if (!onUpdateSettings || !appSettings) return
    const interval = parseInt(reconnectIntervalInput, 10)
    const attempts = parseInt(maxAttemptsInput, 10)
    const ttl = parseInt(cacheLimitInput, 10)
    const maxFiles = parseInt(cacheMaxFilesInput, 10)
    const timeout = parseInt(commandTimeoutInput, 10)
    const workers = parseInt(uploadWorkersInput, 10)
    if (isNaN(interval) || isNaN(attempts) || isNaN(ttl) || isNaN(maxFiles) || isNaN(timeout) || isNaN(workers) || interval < 1 || attempts < 1 || ttl < 1 || maxFiles < 1 || timeout < 1 || workers < 1) return
    setSettingsSaving(true)
    try {
      await onUpdateSettings({
        reconnect_interval: interval,
        max_reconnect_attempts: attempts,
        cache_ttl_hours: ttl,
        cache_max_files: maxFiles,
        command_timeout_minutes: timeout,
        upload_workers: workers,
      })
    } finally {
      setSettingsSaving(false)
    }
  }

  // Reboot handler - open confirm dialog
  const handleReboot = (force: boolean) => {
    if (!sessionId) return
    setRebootConfirm({ show: true, force })
  }

  // Actually execute reboot after user confirms
  const execReboot = async () => {
    if (!sessionId) return
    const force = rebootConfirm.force
    const cmd = force ? 'reboot -f' : 'reboot'
    setRebootConfirm({ show: false, force: false })
    setRebootLoading(true)

    // Show execution panel
    setRebootExecPanel({
      show: true,
      logs: [`[CMD] ${cmd}`, 'Executing reboot command...'],
      status: 'running',
    })

    try {
      const result = await invoke<string>('server_reboot', { sessionId, force })
      setRebootExecPanel(prev => ({
        ...prev,
        logs: [...prev.logs, `[OK] ${result}`, '', 'Server is rebooting. SSH connection will be lost.'],
        status: 'done',
      }))
    } catch (e) {
      const errMsg = String(e)
      // reboot may kill connection, so connection errors are expected
      if (errMsg.includes('Connection') || errMsg.includes('closed') || errMsg.includes('disconnected')) {
        setRebootExecPanel(prev => ({
          ...prev,
          logs: [...prev.logs, '[OK] Server is rebooting. SSH connection has been disconnected.'],
          status: 'done',
        }))
      } else {
        setRebootExecPanel(prev => ({
          ...prev,
          logs: [...prev.logs, `[ERROR] ${errMsg}`],
          status: 'error',
        }))
      }
    }
    setRebootLoading(false)
  }

  // Toggle auth mode
  const handleToggleAuthMode = async (field: 'password' | 'pubkey', value: boolean) => {
    if (!sessionId || !authMode) return
    const newMode = { ...authMode, [field]: value }
    // Prevent disabling both
    if (!newMode.password && !newMode.pubkey) return
    setAuthModeSaving(true)
    try {
      await invoke('server_set_ssh_auth_mode', {
        sessionId,
        passwordEnabled: newMode.password,
        pubkeyEnabled: newMode.pubkey,
      })
      setAuthMode(newMode)
    } catch {
      // revert
      await fetchAuthMode()
    } finally {
      setAuthModeSaving(false)
    }
  }

  // Generate key pair
  const handleGenerateKey = async () => {
    setKeyGenLoading(true)
    setKeyError('')
    setKeyMessage('')
    try {
      const kp = await invoke<SshKeyPair>('ssh_generate_keypair', { algorithm: keyAlgorithm })
      setKeyPair(kp)
    } catch (e) {
      setKeyError(String(e))
    } finally {
      setKeyGenLoading(false)
    }
  }

  // Download private key
  const handleDownloadKey = async () => {
    if (!keyPair) return
    try {
      await invoke<string>('save_key_to_local', {
        content: keyPair.private_key_pem,
        fileName: `id_${keyAlgorithm}`,
      })
    } catch (e) {
      const msg = String(e)
      if (!msg.includes('cancelled')) setKeyError(msg)
    }
  }

  // Deploy public key to server
  const handleDeployKey = async () => {
    if (!sessionId || !keyPair) return
    setKeyDeployLoading(true)
    setKeyError('')
    setKeyMessage('')
    try {
      const result = await invoke<string>('server_deploy_pubkey', {
        sessionId,
        pubkey: keyPair.public_key_openssh,
      })
      setKeyMessage(result)
    } catch (e) {
      setKeyError(String(e))
    } finally {
      setKeyDeployLoading(false)
    }
  }

  if (!sessionId) return <div className="sp-empty">{t('common.connectFirst')}</div>

  return (
    <div className="settings-panel">
      <h2 className="settings-panel-title">{t('settings.title')}</h2>

      {/* Grid layout for settings cards */}
      <div className="settings-grid">
        {/* App Settings - Auto Reconnect */}
        {appSettings && (
          <div className="settings-card">
            <div className="settings-card-header">{t('settings.appSettings')}</div>
            <div className="settings-card-body">
              <div className="settings-row">
                <span className="settings-label">{t('settings.autoReconnect')}</span>
                <button
                  className={`firewall-toggle ${appSettings.auto_reconnect ? 'on' : 'off'}`}
                  onClick={onToggleAutoReconnect}
                >
                  <span className="toggle-track"><span className="toggle-thumb" /></span>
                  <span className="toggle-label">{appSettings.auto_reconnect ? t('common.on') : t('common.off')}</span>
                </button>
              </div>
              <div className="edit-field">
                <label>{t('settings.reconnectInterval')}</label>
                <input
                  type="number"
                  min="1"
                  value={reconnectIntervalInput}
                  onChange={(e) => setReconnectIntervalInput(e.target.value)}
                  className="create-input"
                  disabled={!onUpdateSettings || settingsSaving}
                />
              </div>
              <div className="edit-field">
                <label>{t('settings.maxAttempts')}</label>
                <input
                  type="number"
                  min="1"
                  value={maxAttemptsInput}
                  onChange={(e) => setMaxAttemptsInput(e.target.value)}
                  className="create-input"
                  disabled={!onUpdateSettings || settingsSaving}
                />
              </div>
              <div className="edit-field">
                <label>{t('settings.commandTimeout')} <span style={{ color: '#8b949e', fontWeight: 400 }}>— {t('settings.commandTimeoutHint')}</span></label>
                <input
                  type="number"
                  min="1"
                  value={commandTimeoutInput}
                  onChange={(e) => setCommandTimeoutInput(e.target.value)}
                  className="create-input"
                  disabled={!onUpdateSettings || settingsSaving}
                />
              </div>
              <div className="edit-field">
                <label>{t('settings.uploadWorkers')} <span style={{ color: '#8b949e', fontWeight: 400 }}>— {t('settings.uploadWorkersHint')}</span></label>
                <input
                  type="number"
                  min="1"
                  value={uploadWorkersInput}
                  onChange={(e) => setUploadWorkersInput(e.target.value)}
                  className="create-input"
                  disabled={!onUpdateSettings || settingsSaving}
                />
              </div>
              {onUpdateSettings && (
                <button
                  className="svc-cfg-btn primary"
                  onClick={handleSaveSettings}
                  disabled={settingsSaving}
                  style={{ marginTop: 8 }}
                >
                  {settingsSaving ? t('common.saving') : t('common.save')}
                </button>
              )}
            </div>
          </div>
        )}

        {/* File Cache Settings */}
        {appSettings && (
          <div className="settings-card">
            <div className="settings-card-header" style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
              <span>{t('settings.fileCache')} <span style={{ color: '#8b949e', fontWeight: 400, fontSize: 12 }}>({cacheCount} {t('settings.directories')})</span></span>
              <button
                className={`firewall-toggle ${appSettings.cache_enabled ? 'on' : 'off'}`}
                onClick={() => onUpdateSettings?.({ cache_enabled: !appSettings.cache_enabled })}
                disabled={!onUpdateSettings}
              >
                <span className="toggle-track"><span className="toggle-thumb" /></span>
                <span className="toggle-label">{appSettings.cache_enabled ? t('common.on') : t('common.off')}</span>
              </button>
            </div>
            <div className="settings-card-body">
              <div className="edit-field">
                <label>{t('settings.cacheTtl')} <span style={{ color: '#8b949e', fontWeight: 400 }}>— {t('settings.cacheTtlHint')}</span></label>
                <input
                  type="number"
                  min="1"
                  value={cacheLimitInput}
                  onChange={(e) => setCacheLimitInput(e.target.value)}
                  className="create-input"
                  disabled={!onUpdateSettings || settingsSaving || !appSettings.cache_enabled}
                />
              </div>
              <div className="edit-field">
                <label>{t('settings.maxFilesPerDir')} <span style={{ color: '#8b949e', fontWeight: 400 }}>— {t('settings.maxFilesHint')}</span></label>
                <input
                  type="number"
                  min="1"
                  value={cacheMaxFilesInput}
                  onChange={(e) => setCacheMaxFilesInput(e.target.value)}
                  className="create-input"
                  disabled={!onUpdateSettings || settingsSaving || !appSettings.cache_enabled}
                />
              </div>
              <div style={{ display: 'flex', gap: 8, marginTop: 8 }}>
                {onUpdateSettings && (
                  <button
                    className="svc-cfg-btn primary"
                    onClick={handleSaveSettings}
                    disabled={settingsSaving || !appSettings.cache_enabled}
                  >
                    {settingsSaving ? t('common.saving') : t('common.save')}
                  </button>
                )}
                <button
                  className="svc-cfg-btn"
                  onClick={handleClearCache}
                  disabled={cacheClearing || cacheCount === 0}
                  style={{ background: '#da3633', color: '#fff', borderColor: '#da3633' }}
                >
                  {cacheClearing ? t('settings.clearing') : t('settings.clearAllCache')}
                </button>
              </div>
            </div>
          </div>
        )}

        {/* Software Update */}
        <div className="settings-card">
          <div className="settings-card-header">{t('settings.softwareUpdate')}</div>
          <div className="settings-card-body">
            <div className="settings-row">
              <span className="settings-label">{t('settings.currentVersion')}</span>
              <span className="settings-value" style={{ fontFamily: "'JetBrains Mono', monospace", fontSize: 13, color: '#8b949e' }}>{appVersion || '—'}</span>
            </div>
            <button
              className="svc-cfg-btn primary"
              style={{ width: '100%' }}
              onClick={handleCheckUpdate}
              disabled={updateChecking}
            >
              {updateChecking ? t('settings.checking') : t('settings.checkUpdates')}
            </button>
            {updateMessage && (
              <div style={{ padding: '8px 12px', background: updateMessage.includes('latest') ? '#1f6feb22' : '#2ea04322', borderRadius: 6, fontSize: 13, color: updateMessage.includes('latest') ? '#58a6ff' : '#3fb950' }}>
                {updateMessage}
              </div>
            )}
          </div>
        </div>

        {/* System Info */}
        <div className="settings-card">
          <div className="settings-card-header">{t('settings.systemInfo')}</div>
          <div className="settings-card-body">
            <div className="settings-row">
              <span className="settings-label">{t('settings.lastBoot')}</span>
              <span className="settings-value">{bootTime || t('common.loading')}</span>
            </div>
            <div className="settings-row">
              <span className="settings-label">{t('settings.uptime')}</span>
              <span className="settings-value">{uptimeDuration || t('common.loading')}</span>
            </div>
          </div>
        </div>

        {/* Server Reboot */}
        <div className="settings-card">
          <div className="settings-card-header">{t('settings.serverReboot')}</div>
          <div className="settings-card-body">
            <div className="settings-btn-row">
              <button
                className="svc-cfg-btn primary"
                onClick={() => handleReboot(false)}
                disabled={rebootLoading}
              >
                {rebootLoading ? '...' : t('settings.reboot')}
              </button>
              <button
                className="svc-cfg-btn danger"
                onClick={() => handleReboot(true)}
                disabled={rebootLoading}
              >
                {rebootLoading ? '...' : t('settings.forceReboot')}
              </button>
            </div>
            <div className="settings-hint">
              {t('settings.rebootHint')}
            </div>
          </div>
        </div>

        {/* SSH Authentication */}
        <div className="settings-card" style={{ gridColumn: 'span 2' }}>
          <div className="settings-card-header">{t('settings.sshAuth')}</div>
          <div className="settings-card-body">
            {/* Auth mode toggles */}
            <div className="settings-auth-toggles">
              <div className="settings-row">
                <span className="settings-label">{t('settings.passwordLogin')}</span>
                <button
                  className={`firewall-toggle ${authMode?.password ? 'on' : 'off'} ${authModeSaving ? 'loading' : ''}`}
                  onClick={() => handleToggleAuthMode('password', !authMode?.password)}
                  disabled={authModeSaving || !authMode || (authMode.password && !authMode.pubkey)}
                  title={authMode?.password && !authMode?.pubkey ? 'Cannot disable the last auth method' : ''}
                >
                  <span className="toggle-track"><span className="toggle-thumb" /></span>
                  <span className="toggle-label">{authMode?.password ? t('common.on') : t('common.off')}</span>
                </button>
              </div>
              <div className="settings-row">
                <span className="settings-label">{t('settings.keyLogin')}</span>
                <button
                  className={`firewall-toggle ${authMode?.pubkey ? 'on' : 'off'} ${authModeSaving ? 'loading' : ''}`}
                  onClick={() => handleToggleAuthMode('pubkey', !authMode?.pubkey)}
                  disabled={authModeSaving || !authMode || (!authMode.password && authMode.pubkey)}
                  title={!authMode?.password && authMode?.pubkey ? 'Cannot disable the last auth method' : ''}
                >
                  <span className="toggle-track"><span className="toggle-thumb" /></span>
                  <span className="toggle-label">{authMode?.pubkey ? t('common.on') : t('common.off')}</span>
                </button>
              </div>
            </div>
            {authModeLoading && <div className="settings-muted">{t('settings.loadingAuth')}</div>}

            {/* Key generation */}
            <div className="settings-key-section">
              <div className="settings-section-sub-header">{t('settings.sshKeyManagement')}</div>
              <div className="settings-form-row">
                <select
                  className="settings-select"
                  value={keyAlgorithm}
                  onChange={(e) => { setKeyAlgorithm(e.target.value); setKeyPair(null) }}
                >
                  <option value="ed25519">Ed25519</option>
                  <option value="rsa">RSA 4096</option>
                </select>
                <button
                  className="svc-cfg-btn primary"
                  onClick={handleGenerateKey}
                  disabled={keyGenLoading}
                >
                  {keyGenLoading ? t('settings.generating') : t('settings.generateKey')}
                </button>
              </div>

              {keyError && <div className="settings-error">{keyError}</div>}
              {keyMessage && <div className="settings-success">{keyMessage}</div>}

              {keyPair && (
                <>
                  <div className="settings-pubkey">
                    <label>{t('settings.publicKey')}</label>
                    <textarea
                      className="settings-pubkey-textarea"
                      readOnly
                      value={keyPair.public_key_openssh}
                      rows={3}
                    />
                  </div>
                  <div className="settings-btn-row">
                    <button className="svc-cfg-btn" onClick={handleDownloadKey}>
                      {t('settings.downloadPrivateKey')}
                    </button>
                    <button
                      className="svc-cfg-btn primary"
                      onClick={handleDeployKey}
                      disabled={keyDeployLoading}
                    >
                      {keyDeployLoading ? '...' : t('settings.deployToServer')}
                    </button>
                  </div>
                  <div className="settings-hint settings-hint-warning">
                    {t('settings.keyGenHint')}
                  </div>
                </>
              )}
            </div>
          </div>
        </div>
      </div>

      {/* Reboot Confirm Dialog */}
      {rebootConfirm.show && (
        <div className="fb-dialog-overlay">
          <div className="fb-dialog reboot-confirm-dialog" onClick={(e) => e.stopPropagation()}>
            <button 
              className="modal-close-btn"
              onClick={() => setRebootConfirm({ show: false, force: false })}
              title="关闭"
            >×</button>
            <div className="fb-dialog-title">
              {rebootConfirm.force ? t('settings.forceRebootServer') : t('settings.rebootServer')}
            </div>
            <div className="reboot-confirm-msg">
              {rebootConfirm.force
                ? t('settings.forceRebootConfirmMsg')
                : t('settings.rebootConfirmMsg')}
            </div>
            {rebootConfirm.force && (
              <div className="reboot-confirm-warning">
                <span className="reboot-warning-icon">!</span>
                {t('settings.forceRebootWarning')}
              </div>
            )}
            <div className="fb-dialog-actions">
              <button
                className="fb-dialog-btn"
                onClick={() => setRebootConfirm({ show: false, force: false })}
              >
                {t('common.cancel')}
              </button>
              <button
                className={`fb-dialog-btn ${rebootConfirm.force ? 'danger' : 'primary'}`}
                onClick={execReboot}
              >
                {rebootConfirm.force ? t('settings.forceReboot') : t('settings.reboot')}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Command Execution Floating Panel */}
      {rebootExecPanel.show && (
        <div className="reboot-exec-panel">
          <div className="reboot-exec-header">
            <span className="reboot-exec-title">
              {rebootExecPanel.status === 'running' && (
                <>
                  <span className="reboot-exec-spinner" />
                  {t('settings.executing')}
                </>
              )}
              {rebootExecPanel.status === 'done' && (
                <span style={{ color: '#3fb950' }}>&#10003; {t('settings.completed')}</span>
              )}
              {rebootExecPanel.status === 'error' && (
                <span style={{ color: '#f85149' }}>&#10007; Failed</span>
              )}
            </span>
            <button
              className="reboot-exec-close"
              onClick={() => setRebootExecPanel({ show: false, logs: [], status: 'running' })}
            >
              &#10005;
            </button>
          </div>
          <div className="reboot-exec-log">
            {rebootExecPanel.logs.map((line, i) => (
              <div key={i} className={`reboot-exec-log-line ${
                line.startsWith('[CMD]') ? 'cmd' :
                line.startsWith('[OK]') ? 'ok' :
                line.startsWith('[ERROR]') ? 'error' : ''
              }`}>
                {line}
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  )
}
