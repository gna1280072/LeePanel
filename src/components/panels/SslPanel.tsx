import { useState, useEffect, useCallback, useRef } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen, emit } from '@tauri-apps/api/event'
import { useTranslation } from 'react-i18next'

interface SiteInfo {
  domain: string
  root: string
  config_path: string
  ssl: boolean
  php_version: string
  enabled: boolean
  created_at: number
}

interface SslPanelProps {
  sessionId: string | null
}

interface SslLog {
  domain: string
  lines: string[]
  status: 'idle' | 'installing' | 'done' | 'error'
}

export default function SslPanel({ sessionId }: SslPanelProps) {
  const { t } = useTranslation()
  const [sites, setSites] = useState<SiteInfo[]>([])
  const [loading, setLoading] = useState(false)
  const [logs, setLogs] = useState<Record<string, SslLog>>({})
  const logEndRef = useRef<HTMLDivElement>(null)

  const fetchSites = useCallback(async () => {
    if (!sessionId) return
    setLoading(true)
    try {
      const result = await invoke<SiteInfo[]>('server_list_sites', { sessionId })
      setSites(result)
    } catch {
      // ignore
    } finally {
      setLoading(false)
    }
  }, [sessionId])

  useEffect(() => { fetchSites() }, [fetchSites])

  // Listen for ssl-install-progress events
  useEffect(() => {
    if (!sessionId) return
    const unlisten = listen<{ sessionId: string; domain: string; line: string; status: string }>(
      'ssl-install-progress',
      (event) => {
        if (event.payload.sessionId !== sessionId) return
        const { domain, line, status } = event.payload
        setLogs(prev => {
          const entry = prev[domain] || { domain, lines: [], status: 'idle' }
          return {
            ...prev,
            [domain]: {
              ...entry,
              lines: [...entry.lines, line],
              status: status as SslLog['status'],
            }
          }
        })
      }
    )
    return () => { unlisten.then(fn => fn()) }
  }, [sessionId])

  // Listen for ssl-installed events to refresh site list
  useEffect(() => {
    if (!sessionId) return
    const unlisten = listen<{ sessionId: string; domain: string }>(
      'ssl-installed',
      (event) => {
        if (event.payload.sessionId === sessionId) {
          fetchSites()
        }
      }
    )
    return () => { unlisten.then(fn => fn()) }
  }, [sessionId, fetchSites])

  // Auto-scroll log to bottom
  useEffect(() => {
    logEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [logs])

  const handleInstall = async (domain: string) => {
    if (!sessionId) return
    setLogs(prev => ({
      ...prev,
      [domain]: { domain, lines: [t('ssl.startingSetup', { domain })], status: 'installing' }
    }))
    try {
      await invoke<string>('server_setup_ssl', { sessionId, domain })
      // Notify other panels to refresh their site lists
      await emit('ssl-installed', { sessionId, domain })
    } catch (e) {
      setLogs(prev => {
        const entry = prev[domain] || { domain, lines: [], status: 'idle' }
        return {
          ...prev,
          [domain]: {
            ...entry,
            lines: [...entry.lines, String(e)],
            status: 'error',
          }
        }
      })
    }
  }

  if (!sessionId) {
    return <div className="sp-empty">{t('ssl.connectToManage')}</div>
  }

  // Find domain with active log
  const activeLog = Object.values(logs).find(l => l.status === 'installing')
    || Object.values(logs).find(l => l.status !== 'idle')

  return (
    <div className="ssl-panel">
      <div className="sp-section-header">
        <h3>{t('ssl.title')}</h3>
        <button className="svc-cfg-btn" onClick={fetchSites} disabled={loading}>
          {loading ? t('common.loading') : t('common.refresh')}
        </button>
      </div>

      {sites.length === 0 && !loading && (
        <div className="sp-empty">{t('ssl.noSites')}</div>
      )}

      <div className="sites-grid">
        {sites.map(site => (
          <div key={site.domain} className={`site-card ${site.ssl ? 'running' : ''}`}>
            <div className="site-card-header">
              <div className="site-domain">
                <span className="site-domain-text">{site.domain}</span>
                {site.ssl ? (
                  <span className="site-ssl-badge">SSL</span>
                ) : (
                  <span className="site-ssl-badge" style={{ background: 'rgba(248, 139, 56, 0.1)', color: '#f0883e', border: '1px solid rgba(248, 139, 56, 0.3)' }}>{t('ssl.noSsl')}</span>
                )}
              </div>
            </div>
            <div className="site-card-body">
              <div className="site-info-row">
                <span className="site-info-value mono site-root-link">{site.root}</span>
              </div>
            </div>
            {!site.ssl && (
              <div className="site-card-actions">
                <button
                  className="svc-cfg-btn"
                  style={{ background: '#16a34a', color: '#fff', border: '1px solid #15803d' }}
                  onClick={() => handleInstall(site.domain)}
                  disabled={logs[site.domain]?.status === 'installing'}
                >
                  {logs[site.domain]?.status === 'installing' ? t('ssl.installing') : t('ssl.freeInstall')}
                </button>
              </div>
            )}
          </div>
        ))}
      </div>

      {activeLog && (
        <div className="ssl-log-section">
          <div className="ssl-log-header">
            <span>{activeLog.domain}</span>
            {activeLog.status === 'installing' && <div className="install-spinner" />}
            {activeLog.status === 'done' && <span style={{ color: '#3fb950' }}>Done</span>}
            {activeLog.status === 'error' && <span style={{ color: '#f85149' }}>Error</span>}
          </div>
          <div className="install-log">
            {activeLog.lines.map((line, i) => (
              <div
                className={`install-log-line${activeLog.status === 'error' && i === activeLog.lines.length - 1 ? ' error' : ''}`}
                key={i}
              >
                {line}
              </div>
            ))}
            <div ref={logEndRef} />
          </div>
        </div>
      )}
    </div>
  )
}
