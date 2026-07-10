import { useState, useEffect, useRef } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useTranslation } from 'react-i18next'

interface SoftwareInfo {
  name: string
  display_name: string
  category: string
  installed: boolean
  version: string
  service_name: string
  running: boolean
}

interface SoftwareRepoProps {
  sessionId: string | null
  onDisconnect?: () => void
}

type PanelState = 'loading' | 'ready' | 'error' | 'running'
type LogStatus = 'running' | 'done' | 'error'

interface ConfirmAction {
  software: SoftwareInfo
  action: 'install' | 'uninstall'
}

export default function SoftwareRepo({ sessionId, onDisconnect }: SoftwareRepoProps) {
  const { t } = useTranslation()
  const [state, setState] = useState<PanelState>('loading')
  const [software, setSoftware] = useState<SoftwareInfo[]>([])
  const [error, setError] = useState('')
  const [confirmAction, setConfirmAction] = useState<ConfirmAction | null>(null)
  const [logs, setLogs] = useState<string[]>([])
  const [logStatus, setLogStatus] = useState<LogStatus | null>(null)
  const [actionLabel, setActionLabel] = useState('')
  const logEndRef = useRef<HTMLDivElement>(null)

  // Config editor state
  const [configEditorOpen, setConfigEditorOpen] = useState(false)
  const [configEditorContent, setConfigEditorContent] = useState('')
  const [configEditorLoading, setConfigEditorLoading] = useState(false)
  const [configEditorSaving, setConfigEditorSaving] = useState(false)
  const [configEditorTitle, setConfigEditorTitle] = useState('')
  const [configEditorMaximized, setConfigEditorMaximized] = useState(false)
  const [configEditorPath, setConfigEditorPath] = useState('')

  // ponytail: all version selectors removed — system package manager handles versions

  // PHP version selection modal state
  const [phpVersionModalOpen, setPhpVersionModalOpen] = useState(false)
  const [availableVersions, setAvailableVersions] = useState<string[]>([])
  const [selectedVersion, setSelectedVersion] = useState('')
  const [versionsLoading, setVersionsLoading] = useState(false)
  const [versionsError, setVersionsError] = useState('')

  // Package sources management state
  const [sourcesModalOpen, setSourcesModalOpen] = useState(false)
  const [removableSources, setRemovableSources] = useState<string[]>([])
  const [selectedSources, setSelectedSources] = useState<string[]>([])
  const [sourcesLoading, setSourcesLoading] = useState(false)
  const [sourcesError, setSourcesError] = useState('')
  const [cleaningSources, setCleaningSources] = useState(false)
  const [cleanLogs, setCleanLogs] = useState<string[]>([])
  const [cleanLogStatus, setCleanLogStatus] = useState<'running' | 'done' | 'error' | null>(null)

  const loadSoftware = async () => {
    if (!sessionId) return
    setState('loading')
    setError('')
    try {
      const list = await invoke<SoftwareInfo[]>('server_get_software_list', { sessionId })
      setSoftware(list)
      setState('ready')
    } catch (e) {
      setError(String(e))
      setState('error')
    }
  }

  useEffect(() => { loadSoftware() }, [sessionId])

  // Listen for progress events
  useEffect(() => {
    if (!sessionId) return
    const unlisten = listen<{ sessionId: string; line: string; status: string }>(
      'software-action-progress',
      (event) => {
        if (event.payload.sessionId !== sessionId) return
        setLogs(prev => [...prev, event.payload.line])
        if (event.payload.status === 'done') {
          setLogStatus('done')
        } else if (event.payload.status === 'error') {
          setLogStatus('error')
        }
      }
    )
    return () => { unlisten.then(fn => fn()) }
  }, [sessionId])

  // Listen for sources action progress events
  useEffect(() => {
    if (!sessionId) return
    const unlisten = listen<{ sessionId: string; line: string; status: string }>(
      'sources-action-progress',
      (event) => {
        if (event.payload.sessionId !== sessionId) return
        setCleanLogs(prev => [...prev, event.payload.line])
        if (event.payload.status === 'done') {
          setCleanLogStatus('done')
        } else if (event.payload.status === 'error') {
          setCleanLogStatus('error')
        }
      }
    )
    return () => { unlisten.then(fn => fn()) }
  }, [sessionId])

  useEffect(() => {
    logEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [logs])

  const handleAction = async (sw: SoftwareInfo, action: 'install' | 'uninstall') => {
    if (!sessionId) return
    let options = ''
    // ponytail: options always empty — system package manager picks the version
    setState('running')
    setLogs([`${action === 'install' ? 'Installing' : 'Uninstalling'} ${sw.display_name}...`])
    setLogStatus('running')
    setActionLabel(`${action === 'install' ? 'Installing' : 'Uninstalling'} ${sw.display_name}`)

    try {
      await invoke('server_software_action', {
        sessionId,
        software: sw.name,
        action,
        options,
      })
      setLogStatus('done')
      // Disconnect after nginx install to refresh environment
      if (action === 'install' && (sw.name === 'nginx' || sw.name === 'mysql')) {
        onDisconnect?.()
      }
    } catch (e) {
      const msg = String(e)
      setLogs(prev => [...prev, msg.length > 300 ? msg.slice(0, 300) + '...' : msg])
      setLogStatus('error')
    }
  }

  const handleServiceAction = async (sw: SoftwareInfo, action: string) => {
    if (!sessionId || !sw.service_name) return
    try {
      await invoke('server_service_action', { sessionId, service: sw.service_name, action })
      setTimeout(loadSoftware, 1000)
    } catch (e) {
      setError(`${action} failed: ${e}`)
    }
  }

  const handlePHPInstallClick = async () => {
    if (!sessionId) return
    setVersionsLoading(true)
    setVersionsError('')
    setPhpVersionModalOpen(true)
    try {
      const versions = await invoke<string[]>('server_get_available_php_versions', { sessionId })
      setAvailableVersions(versions)
      if (versions.length === 0) {
        setVersionsError(t('software.noVersionsAvailable'))
      } else {
        setSelectedVersion(versions[versions.length - 1]) // default to latest
      }
    } catch (e) {
      setVersionsError(`${t('software.queryFailed')}: ${String(e)}`)
    } finally {
      setVersionsLoading(false)
    }
  }

  // Load removable sources
  const handleManageSourcesClick = async () => {
    if (!sessionId) return
    setSourcesLoading(true)
    setSourcesError('')
    setSourcesModalOpen(true)
    try {
      const sources = await invoke<string[]>('server_get_removable_sources', { sessionId })
      setRemovableSources(sources)
      setSelectedSources([])
    } catch (e) {
      setSourcesError(`${t('software.queryFailed')}: ${String(e)}`)
    } finally {
      setSourcesLoading(false)
    }
  }

  // Remove selected sources
  const handleRemoveSelectedSources = async () => {
    if (!sessionId || selectedSources.length === 0) return
    try {
      await invoke<string>('server_remove_sources', {
        sessionId,
        sourceNames: selectedSources,
      })
      setSourcesModalOpen(false)
      setTimeout(() => handleManageSourcesClick(), 500)
    } catch (e) {
      setSourcesError(`${t('software.queryFailed')}: ${String(e)}`)
    }
  }

  // Clean and update sources
  const handleCleanAndUpdateSources = async () => {
    if (!sessionId) return
    setCleaningSources(true)
    setCleanLogs([t('software.cleaningSources')])
    setCleanLogStatus('running')
    try {
      await invoke('server_clean_and_update_sources', { sessionId })
      setCleanLogStatus('done')
    } catch (e) {
      const msg = String(e)
      setCleanLogs(prev => [...prev, msg.length > 300 ? msg.slice(0, 300) + '...' : msg])
      setCleanLogStatus('error')
    }
  }

  const handleConfirmPHPInstall = async () => {
    if (!sessionId || !selectedVersion) return
    setPhpVersionModalOpen(false)
    setState('running')
    setLogs([`${t('software.installingPHPVersion', { version: selectedVersion })}`])
    setLogStatus('running')
    setActionLabel(`${t('software.installingPHPVersion', { version: selectedVersion })}`)
    try {
      await invoke('server_software_action', {
        sessionId,
        software: 'php',
        action: 'install',
        options: selectedVersion,
      })
      setLogStatus('done')
    } catch (e) {
      const msg = String(e)
      setLogs(prev => [...prev, msg.length > 300 ? msg.slice(0, 300) + '...' : msg])
      setLogStatus('error')
    }
  }

  const getConfigPath = (sw: SoftwareInfo): string => {
    // ponytail: config file paths for each software
    switch (sw.name) {
      case 'nginx': return '/etc/nginx/nginx.conf'
      case 'mysql': return '/etc/mysql/my.cnf'
      case 'postgresql': return `/etc/postgresql/${sw.version}/main/postgresql.conf`
      case 'redis': return '/etc/redis/redis.conf'
      case 'memcached': return '/etc/memcached.conf'
      case 'docker': return '/etc/docker/daemon.json'
      default:
        if (sw.name.startsWith('php')) {
          const ver = sw.name.replace('php', '')
          return `/etc/php/${ver}/fpm/pool.d/www.conf`
        }
        if (sw.name.startsWith('apache')) return '/etc/apache2/apache2.conf'
        return ''
    }
  }

  const handleEditConfig = async (sw: SoftwareInfo) => {
    if (!sessionId) return
    const configPath = getConfigPath(sw)
    if (!configPath) return
    setConfigEditorPath(configPath)
    setConfigEditorTitle(sw.display_name)
    setConfigEditorOpen(true)
    setConfigEditorLoading(true)
    try {
      const text = await invoke<string>('server_read_remote_file', {
        sessionId,
        path: configPath,
      })
      setConfigEditorContent(text)
    } catch (e) {
      setError(String(e))
      setConfigEditorOpen(false)
    } finally {
      setConfigEditorLoading(false)
    }
  }

  const handleSaveConfig = async () => {
    if (!sessionId) return
    setConfigEditorSaving(true)
    try {
      await invoke('server_write_remote_file', {
        sessionId,
        path: configEditorPath,
        content: configEditorContent,
      })
      setConfigEditorOpen(false)
      // Reload software list to reflect changes
      setTimeout(loadSoftware, 500)
    } catch (e) {
      setError(String(e))
    } finally {
      setConfigEditorSaving(false)
    }
  }

  if (!sessionId) {
    return <div className="sp-empty">{t('software.connectToManage')}</div>
  }

  const categories = [
    { key: 'web', label: t('software.webServer') },
    // ponytail: database category restored (only MySQL/MariaDB removed, Redis/PostgreSQL/Memcached kept)
    { key: 'database', label: t('software.database') },
    { key: 'runtime', label: t('software.runtime') },
    { key: 'container', label: t('software.container') },
  ]

  return (
    <div className="sw-panel">
      <div className="sw-header">
        <h2>{t('software.title')}</h2>
        <button className="sp-refresh-btn" onClick={loadSoftware} disabled={state === 'loading' || state === 'running'}>
          {state === 'loading' ? t('common.loading') : t('common.refresh')}
        </button>
      </div>

      {error && (
        <div className="svc-error">{error}</div>
      )}

      {state === 'loading' && software.length === 0 && (
        <div className="sp-loading">{t('software.checkingStatus')}</div>
      )}

      {state === 'error' && software.length === 0 && (
        <div className="sp-error">
          <p>Failed to load: {error}</p>
          <button className="sp-retry-btn" onClick={loadSoftware}>Retry</button>
        </div>
      )}

      {/* Running state - show logs */}
      {state === 'running' && (
        <div className="sw-running">
          <div className="sw-running-header">
            <div className={`sw-running-status ${logStatus}`}>
              {logStatus === 'running' && <div className="sw-spinner" />}
              {logStatus === 'done' && <span className="sw-done-icon">✓</span>}
              {logStatus === 'error' && <span className="sw-error-icon">✗</span>}
              <span>{actionLabel}</span>
            </div>
          </div>
          <div className="sw-log-box">
            {logs.map((line, i) => (
              <div key={i} className={`sw-log-line ${line.includes('ERROR') || line.includes('failed') ? 'error' : ''}`}>
                {line}
              </div>
            ))}
            <div ref={logEndRef} />
          </div>
          {logStatus === 'done' && (
            <button className="sw-action-btn primary" onClick={() => { setState('ready'); loadSoftware() }}>
              {t('software.doneRefresh')}
            </button>
          )}
          {logStatus === 'error' && (
            <button className="sw-action-btn" onClick={() => { setState('ready'); loadSoftware() }}>
              {t('common.close')}
            </button>
          )}
        </div>
      )}

      {/* Package Sources Management Card */}
      {(state === 'ready' || state === 'error') && (
        <div className="sw-sources-card">
          <div className="sw-sources-header">
            <h3>{t('software.packageSourcesTitle')}</h3>
          </div>
          <div className="sw-sources-actions">
            <button
              className="sw-action-btn small primary"
              onClick={handleManageSourcesClick}
              disabled={cleaningSources}
            >
              {t('software.manageSources')}
            </button>
            <button
              className="sw-action-btn small primary"
              onClick={handleCleanAndUpdateSources}
              disabled={cleaningSources}
            >
              {cleaningSources ? t('software.updatingSources') : t('software.cleanSources')}
            </button>
          </div>
        </div>
      )}

      {/* Cleaning Sources Progress */}
      {cleaningSources && (
        <div className="sw-running" style={{ marginTop: '16px', marginBottom: '16px' }}>
          <div className="sw-running-header">
            <div className={`sw-running-status ${cleanLogStatus}`}>
              {cleanLogStatus === 'running' && <div className="sw-spinner" />}
              {cleanLogStatus === 'done' && <span className="sw-done-icon">✓</span>}
              {cleanLogStatus === 'error' && <span className="sw-error-icon">✗</span>}
              <span>{t('software.cleaningSources')}</span>
            </div>
          </div>
          <div className="sw-log-box">
            {cleanLogs.map((line, i) => (
              <div key={i} className={`sw-log-line ${line.includes('ERROR') || line.includes('failed') ? 'error' : ''}`}>
                {line}
              </div>
            ))}
            <div ref={logEndRef} />
          </div>
          {(cleanLogStatus === 'done' || cleanLogStatus === 'error') && (
            <button
              className="sw-action-btn"
              onClick={() => { setCleaningSources(false); setCleanLogs([]); setCleanLogStatus(null); }}
            >
              {t('common.close')}
            </button>
          )}
        </div>
      )}

      {/* Software grid */}
      {(state === 'ready' || state === 'error') && software.length > 0 && (
        <div className="sw-categories">
          {categories.map(cat => {
            const items = software.filter(s => s.category === cat.key)
            if (items.length === 0 && cat.key !== 'web') return null
            return (
              <div key={cat.key} className="sw-category">
                <div className="sw-category-title">{cat.label}</div>
                <div className="sw-grid">
                  {items.map(sw => (
                    <div key={sw.name} className={`sw-card ${sw.installed ? 'installed' : ''}`}>
                      <div className="sw-card-header">
                        <span className="sw-card-name">{sw.display_name}</span>
                        <span className={`sw-status-dot ${sw.running ? 'running' : sw.installed ? 'stopped' : ''}`} />
                      </div>

                      <div className="sw-card-info">
                        {sw.installed ? (
                          <>
                            <span className="sw-version">{sw.version || t('software.installed')}</span>
                            <span className={`sw-state-label ${sw.running ? 'running' : 'stopped'}`}>
                              {sw.running ? t('software.runningLabel') : sw.service_name ? t('software.stoppedLabel') : t('software.installedLabel')}
                            </span>
                          </>
                        ) : (
                          <span className="sw-not-installed">{t('software.notInstalledLabel')}</span>
                        )}
                      </div>

                      <div className="sw-card-actions">
                        {sw.installed ? (
                          <>
                            {sw.service_name && (
                              <>
                                <button
                                  className="sw-action-btn small"
                                  onClick={() => handleServiceAction(sw, 'start')}
                                  disabled={sw.running}
                                >{t('common.start')}</button>
                                <button
                                  className="sw-action-btn small"
                                  onClick={() => handleServiceAction(sw, 'stop')}
                                  disabled={!sw.running}
                                >{t('common.stop')}</button>
                                <button
                                  className="sw-action-btn small"
                                  onClick={() => handleServiceAction(sw, 'restart')}
                                  disabled={!sw.running}
                                >{t('common.restart')}</button>
                              </>
                            )}
                            {getConfigPath(sw) && (
                              <button
                                className="sw-action-btn small"
                                onClick={() => handleEditConfig(sw)}
                              >{t('software.configLabel')}</button>
                            )}
                            <button
                              className="sw-action-btn small danger"
                              onClick={() => setConfirmAction({ software: sw, action: 'uninstall' })}
                            >{t('common.uninstall')}</button>
                          </>
                        ) : (
                          sw.name === 'php' ? (
                            <button
                              className="sw-action-btn small primary"
                              onClick={() => handlePHPInstallClick()}
                              disabled={versionsLoading}
                            >{versionsLoading && phpVersionModalOpen ? t('software.queryingVersions') : t('common.install')}</button>
                          ) : (
                            <button
                              className="sw-action-btn small primary"
                              onClick={() => setConfirmAction({ software: sw, action: 'install' })}
                            >{t('common.install')}</button>
                          )
                        )}
                      </div>
                    </div>
                  ))}
                  {/* ponytail: Install PHP Version card removed */}
                </div>
              </div>
            )
          })}
        </div>
      )}

      {/* Confirm dialog */}
      {confirmAction && (
        <div className="sw-confirm-overlay" onClick={() => setConfirmAction(null)}>
          <div className="sw-confirm-dialog" onClick={e => e.stopPropagation()}>
            <div className="sw-confirm-title">
              {confirmAction.action === 'install' ? t('software.installTitle', { name: confirmAction.software.display_name }) : t('software.uninstallTitle', { name: confirmAction.software.display_name })}
            </div>



            {confirmAction.action === 'uninstall' && (
              <div className="sw-confirm-warning">
                This will remove {confirmAction.software.display_name} and its configuration from the server.
              </div>
            )}

            <div className="sw-confirm-actions">
              <button className="sw-action-btn" onClick={() => setConfirmAction(null)}>{t('common.cancel')}</button>
              <button
                className={`sw-action-btn ${confirmAction.action === 'uninstall' ? 'danger' : 'primary'}`}
                onClick={() => {
                  handleAction(confirmAction.software, confirmAction.action)
                  setConfirmAction(null)
                }}
              >
                {confirmAction.action === 'install' ? t('common.install') : t('common.uninstall')}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* PHP Version Selection Modal */}
      {phpVersionModalOpen && (
        <div className="sw-confirm-overlay" onClick={() => setPhpVersionModalOpen(false)}>
          <div className="sw-confirm-dialog" onClick={e => e.stopPropagation()}>
            <div className="sw-confirm-title">{t('software.selectPHPVersion')}</div>

            {versionsLoading ? (
              <div style={{ padding: '16px', textAlign: 'center' }}>{t('software.queryingVersions')}</div>
            ) : versionsError ? (
              <div className="sw-confirm-warning" style={{ color: '#e74c3c' }}>{versionsError}</div>
            ) : (
              <>
                <div style={{ marginBottom: '16px' }}>
                  <label style={{ display: 'block', marginBottom: '8px', fontWeight: 500 }}>
                    {t('software.availableVersionsLabel')}:
                  </label>
                  <select
                    value={selectedVersion}
                    onChange={(e) => setSelectedVersion(e.target.value)}
                    style={{
                      width: '100%',
                      padding: '8px 12px',
                      borderRadius: '4px',
                      border: '1px solid #ccc',
                      fontSize: '14px',
                      background: 'var(--bg-secondary, #fff)',
                      color: 'var(--text-primary, #000)',
                    }}
                  >
                    {availableVersions.map(ver => (
                      <option key={ver} value={ver}>
                        PHP {ver}
                      </option>
                    ))}
                  </select>
                </div>
                <div className="sw-confirm-warning">
                  {t('software.installPHPVersionWarning', { version: selectedVersion })}
                </div>
              </>
            )}

            <div className="sw-confirm-actions">
              <button
                className="sw-action-btn"
                onClick={() => setPhpVersionModalOpen(false)}
                disabled={versionsLoading}
              >{t('common.cancel')}</button>
              <button
                className="sw-action-btn primary"
                onClick={handleConfirmPHPInstall}
                disabled={versionsLoading || !!versionsError || !selectedVersion}
              >{t('software.installPHPVersion', { version: selectedVersion })}</button>
            </div>
          </div>
        </div>
      )}

      {/* Manage Sources Modal */}
      {sourcesModalOpen && (
        <div className="sw-confirm-overlay" onClick={() => setSourcesModalOpen(false)}>
          <div className="sw-confirm-dialog" onClick={e => e.stopPropagation()}>
            <div className="sw-confirm-title">{t('software.removableSources')}</div>

            {sourcesLoading ? (
              <div style={{ padding: '16px', textAlign: 'center' }}>{t('software.loadingSources')}</div>
            ) : sourcesError ? (
              <div className="sw-confirm-warning" style={{ color: '#e74c3c' }}>{sourcesError}</div>
            ) : removableSources.length === 0 ? (
              <div style={{ padding: '16px', textAlign: 'center' }}>{t('software.noOldSources')}</div>
            ) : (
              <>
                <div style={{ marginBottom: '16px', maxHeight: '300px', overflowY: 'auto' }}>
                  {removableSources.map(source => (
                    <label
                      key={source}
                      style={{
                        display: 'flex',
                        alignItems: 'center',
                        padding: '8px',
                        borderBottom: '1px solid #eee',
                        cursor: 'pointer',
                      }}
                    >
                      <input
                        type="checkbox"
                        checked={selectedSources.includes(source)}
                        onChange={(e) => {
                          if (e.target.checked) {
                            setSelectedSources([...selectedSources, source])
                          } else {
                            setSelectedSources(selectedSources.filter(s => s !== source))
                          }
                        }}
                        style={{ marginRight: '8px' }}
                      />
                      <code style={{ fontSize: '13px' }}>{source}</code>
                    </label>
                  ))}
                </div>

                <div className="sw-confirm-warning">
                  {t('software.removeSourcesWarning')}
                </div>
              </>
            )}

            <div className="sw-confirm-actions">
              <button
                className="sw-action-btn"
                onClick={() => setSourcesModalOpen(false)}
                disabled={sourcesLoading}
              >
                {t('common.cancel')}
              </button>
              <button
                className="sw-action-btn danger"
                onClick={handleRemoveSelectedSources}
                disabled={sourcesLoading || selectedSources.length === 0}
              >
                {t('software.removeSelected')} ({selectedSources.length})
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Apache Config Editor */}
      {configEditorOpen && (
        <div className="fb-dialog-overlay" style={{ zIndex: 1100 }} onClick={() => setConfigEditorOpen(false)}>
          <div
            className={`config-editor-dialog${configEditorMaximized ? ' maximized' : ''}`}
            onClick={(e) => e.stopPropagation()}
          >
            <div className="config-editor-header">
              <span className="config-editor-title">{configEditorTitle} Config — {configEditorPath}</span>
              <div className="config-editor-header-btns">
                <button
                  className="config-editor-maximize"
                  onClick={() => setConfigEditorMaximized(!configEditorMaximized)}
                  title={configEditorMaximized ? t('files.restore') : t('files.maximize')}
                >
                  {configEditorMaximized ? '❐' : '▢'}
                </button>
                <button className="config-editor-close" onClick={() => setConfigEditorOpen(false)}>×</button>
              </div>
            </div>
            {configEditorLoading ? (
              <div className="config-editor-loading">Loading...</div>
            ) : (
              <textarea
                className="config-editor-textarea"
                value={configEditorContent}
                onChange={(e) => setConfigEditorContent(e.target.value)}
                spellCheck={false}
              />
            )}
            <div className="config-editor-footer">
              <button className="fb-dialog-btn" onClick={() => setConfigEditorOpen(false)} disabled={configEditorSaving}>{t('common.cancel')}</button>
              <button
                className="fb-dialog-btn primary"
                disabled={configEditorLoading || configEditorSaving}
                onClick={handleSaveConfig}
              >
                {configEditorSaving ? t('common.saving') : t('common.save')}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
