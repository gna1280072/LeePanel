import { useState, useEffect, useCallback, useRef } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useTranslation } from 'react-i18next'
import EditSite from './EditSite'

interface SiteInfo {
  domain: string
  domains: string
  root: string
  config_path: string
  ssl: boolean
  ssl_cert_path: string | null
  ssl_key_path: string | null
  php_version: string
  running_dir: string
  open_basedir: boolean
  enabled: boolean
  index_files: string
  proxy_target: string
  hotlink_enabled: boolean
  hotlink_extensions: string
  hotlink_allowed_domains: string
  hotlink_response: string
  hotlink_allow_empty_referer: boolean
  created_at: number
}

interface SitesPanelProps {
  sessionId: string | null
  onOpenFolder?: (path: string) => void
  onNavigateToInstall?: () => void
}

type View = 'list' | 'create' | 'edit' | 'progress'

export default function SitesPanel({ sessionId, onOpenFolder, onNavigateToInstall }: SitesPanelProps) {
  const { t } = useTranslation()
  const [view, setView] = useState<View>('list')
  const [sites, setSites] = useState<SiteInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [msg, setMsg] = useState('')

  // Search
  const [searchQuery, setSearchQuery] = useState('')

  // Delete dialog
  const [deleteTarget, setDeleteTarget] = useState<SiteInfo | null>(null)
  const [removeFiles, setRemoveFiles] = useState(false)
  const [deleting, setDeleting] = useState(false)
  const [confirmDomain, setConfirmDomain] = useState('')

  // Edit target
  const [editTarget, setEditTarget] = useState<SiteInfo | null>(null)

  // Progress logs for site creation (shared across views)
  const [progressLogs, setProgressLogs] = useState<string[]>([])

  const openEdit = (site: SiteInfo) => {
    setEditTarget(site)
    setView('edit')
  }

  // Toggle toast notification
  const [toast, setToast] = useState<{ type: 'success' | 'error'; text: string } | null>(null)

  const fetchSites = useCallback(async () => {
    if (!sessionId) return
    setLoading(true)
    setError('')
    try {
      const list = await invoke<SiteInfo[]>('server_list_sites', { sessionId })
      // Sort by creation time descending (newest first), based on config file mtime
      list.sort((a, b) => b.created_at - a.created_at)
      setSites(list)
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }, [sessionId])

  useEffect(() => { fetchSites() }, [fetchSites])

  // Listen for site creation progress events (always active)
  useEffect(() => {
    if (!sessionId) return
    
    const unlisten = listen<{ sessionId: string; domain: string; line: string; status: string }>(
      'site-create-progress',
      (event) => {
        if (event.payload.sessionId === sessionId) {
          setProgressLogs(prev => [...prev, event.payload.line])
        }
      }
    )
    
    return () => {
      unlisten.then(unsub => unsub())
    }
  }, [sessionId])

  // Listen for SSL installation events to refresh site list
  useEffect(() => {
    if (!sessionId) return
    
    const unlisten = listen<{ sessionId: string; domain: string }>(
      'ssl-installed',
      (event) => {
        if (event.payload.sessionId === sessionId) {
          // Refresh the site list to update SSL status
          fetchSites()
        }
      }
    )
    
    return () => {
      unlisten.then(unsub => unsub())
    }
  }, [sessionId, fetchSites])

  const handleDelete = async () => {
    if (!deleteTarget || !sessionId) return
    
    // Validate domain input (case-insensitive)
    if (confirmDomain.trim().toLowerCase() !== deleteTarget.domain.toLowerCase()) {
      setError('输入的域名与目标域名不匹配')
      setTimeout(() => setError(''), 3000)
      return
    }
    
    setDeleting(true)
    try {
      await invoke('server_delete_site', {
        sessionId,
        domain: deleteTarget.domain,
        removeFiles,
      })
      setMsg(`Site ${deleteTarget.domain} deleted`)
      setDeleteTarget(null)
      setConfirmDomain('')
      fetchSites()
    } catch (e) {
      setError(`Delete failed: ${e}`)
    } finally {
      setDeleting(false)
    }
  }

  const handleToggle = async (site: SiteInfo, enable: boolean) => {
    if (!sessionId) return
    try {
      await invoke<string>('server_toggle_site', {
        sessionId,
        configPath: site.config_path,
        domain: site.domain,
        enable,
      })
      setToast({ type: 'success', text: `${site.domain} ${enable ? 'started' : 'stopped'} successfully` })
      setTimeout(() => setToast(null), 2500)
      fetchSites()
    } catch (e) {
      setToast({ type: 'error', text: `${enable ? 'Start' : 'Stop'} failed: ${e}` })
      setTimeout(() => setToast(null), 3500)
    }
  }

  if (!sessionId) return <div className="sp-empty">{t('common.connectFirst')}</div>

  return (
    <div className="sites-panel">
      <div className="sites-header">
        <h2>{t('sites.title')}</h2>
        {view !== 'create' && (
          <button
            className="svc-cfg-btn primary"
            onClick={() => setView('create')}
          >
            {t('sites.newSite')}
          </button>
        )}
        <div className="sites-header-actions">
          <input
            type="text"
            className="sites-search"
            placeholder={t('sites.searchDomain')}
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
          />
          {msg && <span className="sites-msg">{msg}</span>}
          <button
            className="svc-cfg-btn"
            onClick={fetchSites}
            disabled={loading}
          >
            {loading ? t('common.loading') : t('common.refresh')}
          </button>
        </div>
      </div>

      {error && <div className="svc-error">{error}</div>}

      {/* Edit page */}
      {view === 'edit' && editTarget ? (
        <EditSite
          sessionId={sessionId!}
          site={editTarget}
          onBack={() => { setView('list'); setEditTarget(null) }}
          onSaved={() => { setView('list'); setEditTarget(null); setMsg('Site updated'); fetchSites() }}
          onError={setError}
        />
      ) : view === 'create' ? (
        <CreateSiteForm
          sessionId={sessionId}
          onError={setError}
          onNavigateToInstall={onNavigateToInstall}
          onViewProgress={() => {
            setProgressLogs([])
            setView('progress')
          }}
          onCancel={() => setView('list')}
        />
      ) : view === 'progress' ? (
        <CreateSiteProgress
          logs={progressLogs}
          onBack={() => { setView('list'); setProgressLogs([]); fetchSites() }}
        />
      ) : (
        <>
          {loading && sites.length === 0 ? (
            <div className="svc-loading">{t('sites.loadingSites')}</div>
          ) : sites.length === 0 ? (
            <div className="sites-empty">
              <div className="sites-empty-icon">🌐</div>
              <p>{t('sites.noSites')}</p>
              <button className="svc-cfg-btn primary" onClick={() => setView('create')}>
                {t('sites.createFirst')}
              </button>
            </div>
          ) : (
            <>
              {searchQuery && (
                <div style={{ color: '#ef4444', marginBottom: '12px', fontSize: '14px' }}>
                  {t('sites.searchResultsHint')}
                </div>
              )}
              <div className="sites-grid">
              {sites
                .filter(s => !searchQuery || s.domain.toLowerCase().includes(searchQuery.toLowerCase()))
                .map((site) => (
                <div className={`site-card ${site.enabled ? 'running' : 'stopped'}`} key={site.config_path}>
                  <div className="site-card-header">
                    <div className="site-domain">
                      <span
                        className="site-domain-text"
                        style={{ cursor: 'pointer', textDecoration: 'underline', textDecorationStyle: 'dashed' }}
                        onClick={() => openEdit(site)}
                        title="Click to edit site"
                      >
                        {site.domain}
                      </span>
                      {site.ssl && <span className="site-ssl-badge">SSL</span>}
                    </div>
                  </div>
                  <div className="site-card-body">
                    <div className="site-info-row">
                      <span
                        className="site-info-value mono site-root-link"
                        onClick={() => onOpenFolder?.(site.root)}
                        title="Open in File Browser"
                      >{site.root}</span>
                    </div>
                    {site.php_version && (
                      <div className="site-info-row">
                        <span className="site-info-value">PHP {site.php_version}</span>
                      </div>
                    )}
                  </div>
                  <div className="site-card-actions">
                    <button
                      className="svc-cfg-btn"
                      style={!site.enabled ? { background: '#16a34a', color: '#fff', border: '1px solid #15803d' } : {}}
                      onClick={() => handleToggle(site, !site.enabled)}
                    >
                      {site.enabled ? t('common.stop') : t('common.start')}
                    </button>
                    <button className="svc-cfg-btn" style={{ background: '#16a34a', color: '#fff', border: '1px solid #15803d' }} onClick={() => openEdit(site)}>
                      {t('common.edit')}
                    </button>
                  </div>
                  {/* Delete button - positioned at bottom right */}
                  <button
                    className="site-delete-btn"
                    onClick={() => { setDeleteTarget(site); setRemoveFiles(false) }}
                    title="Delete site"
                  >
                    🗑️
                  </button>
                </div>
              ))}
            </div>
            </>
          )}
        </>
      )}

      {/* Delete confirmation dialog */}
      {deleteTarget && (
        <div className="fb-dialog-overlay">
          <div className="fb-dialog fb-delete-site-dialog" onClick={(e) => e.stopPropagation()}>
            <button 
              className="modal-close-btn"
              onClick={() => {
                setDeleteTarget(null)
                setConfirmDomain('')
              }}
              title="关闭"
            >×</button>
            
            {/* Warning header */}
            <div className="delete-warning-header">
              <span className="warning-icon">️</span>
              <div className="warning-title">
                <h3>{t('sites.deleteSiteTitle', { domain: deleteTarget.domain })}</h3>
                <p>{t('sites.deleteWarning')}</p>
              </div>
            </div>
            
            {/* Info box */}
            <div className="delete-info-box">
              {t('sites.deleteInfo', { domain: deleteTarget.domain })}
            </div>
            
            {/* Domain confirmation input */}
            <div className="confirm-input-section">
              <label className="confirm-label">
                {t('sites.typeDomainConfirm')}
              </label>
              <input
                type="text"
                value={confirmDomain}
                onChange={(e) => setConfirmDomain(e.target.value)}
                placeholder={`Enter: ${deleteTarget.domain}`}
                className="confirm-domain-input"
                autoFocus
              />
              {confirmDomain.trim() && confirmDomain.trim().toLowerCase() !== deleteTarget.domain.toLowerCase() && (
                <div className="input-error-msg">
                  ⚠️ {t('sites.domainMismatch')}
                </div>
              )}
            </div>
            
            {/* Delete files option */}
            <label className="site-delete-files-option enhanced">
              <input
                type="checkbox"
                checked={removeFiles}
                onChange={(e) => setRemoveFiles(e.target.checked)}
              />
              <div className="checkbox-content">
                <span className="checkbox-text">{t('sites.alsoDeleteFiles')}</span>
                <code className="path-code">{deleteTarget.root}</code>
              </div>
            </label>
            
            {/* Action buttons */}
            <div className="fb-dialog-actions">
              <button 
                className="fb-dialog-btn cancel-btn"
                onClick={() => {
                  setDeleteTarget(null)
                  setConfirmDomain('')
                }} 
                disabled={deleting}
              >
                {t('common.cancel')}
              </button>
              <button 
                className="fb-dialog-btn danger delete-btn"
                onClick={handleDelete} 
                disabled={deleting || !confirmDomain.trim() || confirmDomain.trim().toLowerCase() !== deleteTarget.domain.toLowerCase()}
              >
                {deleting ? t('common.deleting') : t('common.delete')}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Toggle toast notification */}
      {toast && (
        <div className={`toast-notification toast-${toast.type}`} onClick={() => setToast(null)}>
          <span className="toast-icon">{toast.type === 'success' ? '✓' : '✕'}</span>
          <span className="toast-text">{toast.text}</span>
        </div>
      )}
    </div>
  )
}

interface CreateSiteFormProps {
  sessionId: string
  onError: (msg: string) => void
  onNavigateToInstall?: () => void
  onViewProgress?: () => void
  onCancel?: () => void
}

function CreateSiteForm({
  sessionId,
  onError,
  onNavigateToInstall,
  onViewProgress,
  onCancel,
}: CreateSiteFormProps) {
  const { t } = useTranslation()
  const [domain, setDomain] = useState('')
  const [root, setRoot] = useState('')
  const [phpVersion, setPhpVersion] = useState('')
  const [phpVersions, setPhpVersions] = useState<string[]>([])
  const [useSsl, setUseSsl] = useState(false)
  const [creating, setCreating] = useState(false)

  // Nginx install prompt dialog (unused, kept for future use)
  const [_showNginxPrompt, _setShowNginxPrompt] = useState(false)
  const [createDb, setCreateDb] = useState(false)
  const [dbName, setDbName] = useState('')
  const [dbUser, setDbUser] = useState('')
  const [dbPass, setDbPass] = useState('')

  const domainToIdent = (d: string) => d.replace(/[.-]/g, '_')

  useEffect(() => {
    invoke<string[]>('server_list_php_versions', { sessionId }).then(setPhpVersions).catch(() => {})
  }, [sessionId])

  useEffect(() => {
    if (domain) {
      if (!root || root === `/www/wwwroot/${domain}`) {
        setRoot(`/www/wwwroot/${domain}`)
      }
      const ident = domainToIdent(domain)
      setDbName(ident)
      setDbUser(ident)
    }
  }, [domain])

  const handleCreate = async () => {
    if (!domain.trim()) {
      onError('Please enter a domain name')
      return
    }
    setCreating(true)
    try {
      // Switch to progress view before starting creation
      onViewProgress?.()
      
      const result = await invoke<string>('server_create_site', {
        sessionId,
        domain: domain.trim(),
        root: root.trim() || `/www/wwwroot/${domain.trim()}`,
        phpVersion,
        runningDir: '/',
        openBasedir: true,
        useSsl,
        createDb,
        dbName: createDb ? dbName.trim() : '',
        dbUser: createDb ? dbUser.trim() : '',
        dbPass: createDb ? dbPass : '',
      })
      // Show result (might be partial success with SSL warning)
      if (result.includes('but SSL') || result.includes('but database')) {
        onError(result)
      }
      // Don't auto-jump, stay on progress page for user to review
    } catch (e) {
      const errMsg = String(e)
      // Check if error is about nginx not installed
      if (errMsg.toLowerCase().includes('nginx') && errMsg.toLowerCase().includes('install')) {
        _setShowNginxPrompt(true)
        // Auto-navigate to Install LNMP after 5 seconds
        setTimeout(() => {
          _setShowNginxPrompt(false)
          onNavigateToInstall?.()
        }, 5000)
      } else {
        onError(errMsg)
      }
    } finally {
      setCreating(false)
    }
  }

  const generatePassword = () => {
    const chars = 'ABCDEFGHJKLMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz23456789!@#$%&*'
    let pw = ''
    const arr = new Uint32Array(16)
    crypto.getRandomValues(arr)
    for (let i = 0; i < 16; i++) pw += chars[arr[i] % chars.length]
    setDbPass(pw)
  }

  return (
    <div className="create-site-form">
      <div className="create-site-title">{t('sites.createNewSite')}</div>

      <div className="create-field">
        <label>{t('sites.domainName')}</label>
        <input
          type="text"
          className="create-input"
          placeholder="example.com"
          value={domain}
          onChange={(e) => {
            setDomain(e.target.value)
            if (e.target.value) {
              setRoot(`/www/wwwroot/${e.target.value}`)
              const ident = e.target.value.replace(/[.-]/g, '_')
              setDbName(ident)
              setDbUser(ident)
            }
          }}
        />
      </div>

      <div className="create-field">
        <label>{t('sites.webRoot')}</label>
        <input
          type="text"
          className="create-input"
          placeholder={`/www/wwwroot/${domain || 'example.com'}`}
          value={root}
          onChange={(e) => setRoot(e.target.value)}
        />
      </div>

      <div className="create-field">
        <label>{t('sites.phpVersion')}</label>
        <select
          className="create-select"
          value={phpVersion}
          onChange={(e) => setPhpVersion(e.target.value)}
        >
          <option value="">{t('sites.noneStatic')}</option>
          {phpVersions.map(v => (
            <option key={v} value={v}>PHP {v}</option>
          ))}
        </select>
      </div>

      <label className="create-checkbox">
        <input
          type="checkbox"
          checked={useSsl}
          onChange={(e) => setUseSsl(e.target.checked)}
        />
        <span>{t('sites.enableSsl')}</span>
      </label>

      <label className="create-checkbox">
        <input
          type="checkbox"
          checked={createDb}
          onChange={(e) => setCreateDb(e.target.checked)}
        />
        <span>{t('sites.createMysqlDb')}</span>
      </label>

      {createDb && (
        <div className="create-db-fields">
          <div className="create-field">
            <label>{t('sites.dbName')}</label>
            <input
              type="text"
              className="create-input"
              placeholder="database_name"
              value={dbName}
              onChange={(e) => setDbName(e.target.value)}
            />
          </div>
          <div className="create-field">
            <label>{t('sites.dbUser')}</label>
            <input
              type="text"
              className="create-input"
              placeholder="db_user"
              value={dbUser}
              onChange={(e) => setDbUser(e.target.value)}
            />
          </div>
          <div className="create-field">
            <label>{t('sites.dbPassword')}</label>
            <div className="create-input-row">
              <input
                type="text"
                className="create-input"
                placeholder="password"
                value={dbPass}
                onChange={(e) => setDbPass(e.target.value)}
              />
              <button className="create-gen-btn" type="button" onClick={generatePassword} title="Generate random password">
                &#x21bb;
              </button>
            </div>
          </div>
        </div>
      )}

      <div style={{ display: 'flex', gap: '12px' }}>
        <button
          className="install-btn"
          onClick={handleCreate}
          disabled={creating || !domain.trim()}
        >
          {creating ? t('sites.creating') : t('sites.createSite')}
        </button>
        <button
          className="install-btn secondary"
          onClick={() => onCancel?.()}
        >
          {t('common.cancel')}
        </button>
      </div>
    </div>
  )
}

// Progress display page (similar to SoftwareRepo)
function CreateSiteProgress({
  logs,
  onBack,
}: {
  logs: string[]
  onBack: () => void
}) {
  const { t } = useTranslation()
  const logEndRef = useRef<HTMLDivElement>(null)
  
  useEffect(() => {
    logEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [logs])

  // Show back button only when creation is complete or has error
  const hasError = logs.some(line => line.includes('ERROR') || line.includes('failed'))
  const isComplete = logs.length > 0 && !hasError && logs.some(line => line.toLowerCase().includes('successfully') || line.toLowerCase().includes('completed'))
  const showBackButton = isComplete || hasError

  // Check if a line should be displayed in red (error/warning messages)
  const isErrorLine = (line: string) => {
    return line.includes('ERROR') || 
           line.includes('failed') || 
           line.includes('NOT installed') || 
           line.includes('Install it first')
  }

  return (
    <div className="sw-running">
      <div className="sw-running-header">
        <h2 style={{ margin: 0, fontSize: '18px', fontWeight: 600 }}>{t('sites.siteCreationProgress')}</h2>
      </div>
      <div className="sw-log-box">
        {logs.length === 0 ? (
          <div className="sw-log-line">{t('sites.startingCreation')}</div>
        ) : (
          logs.map((line, i) => (
            <div key={i} className={`sw-log-line ${isErrorLine(line) ? 'error' : ''}`}>
              {line}
            </div>
          ))
        )}
        <div ref={logEndRef} />
      </div>
      {showBackButton && (
        <button className="sw-action-btn primary" onClick={onBack}>
          {t('sites.backToList')}
        </button>
      )}
    </div>
  )
}
