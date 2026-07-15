import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { useTranslation } from 'react-i18next'

interface OsInfo {
  distro: string
  version: string
  codename: string
  family: string
  kernel: string
  arch: string
  hostname: string
}

interface DiskInfo {
  filesystem: string
  size: string
  used: string
  available: string
  use_percent: string
  mount: string
}

interface SystemInfo {
  os: OsInfo
  uptime: string
  load_avg: string
  cpu_model: string
  cpu_cores: number
  cpu_percent?: number
  mem_total_mb: number
  mem_used_mb: number
  mem_free_mb: number
  swap_total_mb: number
  swap_used_mb: number
  disks: DiskInfo[]
}

interface ServiceStatus {
  name: string
  active: boolean
  status_text: string
  version: string
}

interface DashboardProps {
  sessionId: string | null
  onNavigate?: (section: string) => void
}

function formatMb(mb: number): string {
  if (mb >= 1024) return `${(mb / 1024).toFixed(1)} GB`
  return `${mb} MB`
}

function percentBar(used: number, total: number): { percent: number; color: string } {
  if (total === 0) return { percent: 0, color: '#3fb950' }
  const pct = Math.round((used / total) * 100)
  const color = pct > 90 ? '#f85149' : pct > 70 ? '#d29922' : '#3fb950'
  return { percent: pct, color }
}

export default function Dashboard({ sessionId, onNavigate }: DashboardProps) {
  const { t } = useTranslation()
  const [sysInfo, setSysInfo] = useState<SystemInfo | null>(null)
  const [services, setServices] = useState<ServiceStatus[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')

  const fetchData = async () => {
    if (!sessionId) return
    setLoading(true)
    setError('')
    try {
      const [info, svcs] = await Promise.all([
        invoke<SystemInfo>('server_get_system_info', { sessionId }),
        invoke<ServiceStatus[]>('server_get_service_statuses', { sessionId }),
      ])
      setSysInfo(info)
      setServices(svcs)
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    fetchData()
    // Auto-refresh every 30s only when tab is visible
    let interval: number | undefined
    
    const handleVisibilityChange = () => {
      if (document.hidden) {
        // Tab hidden - clear interval to stop polling
        if (interval) clearInterval(interval)
        interval = undefined
      } else {
        // Tab visible - refresh immediately and restart polling
        fetchData()
        interval = setInterval(fetchData, 30000)
      }
    }
    
    // Start polling
    interval = setInterval(fetchData, 30000)
    document.addEventListener('visibilitychange', handleVisibilityChange)
    
    return () => {
      if (interval) clearInterval(interval)
      document.removeEventListener('visibilitychange', handleVisibilityChange)
    }
  }, [sessionId])

  if (!sessionId) {
    return <div className="sp-empty">{t('dashboard.connectToView')}</div>
  }

  if (loading && !sysInfo) {
    return <div className="sp-loading">{t('dashboard.loadingSystem')}</div>
  }

  if (error && !sysInfo) {
    return (
      <div className="sp-error">
        <p>{t('common.failedToLoad', { error })}</p>
        <button className="sp-retry-btn" onClick={fetchData}>{t('common.retry')}</button>
      </div>
    )
  }

  const mem = sysInfo ? percentBar(sysInfo.mem_used_mb, sysInfo.mem_total_mb) : null
  const swap = sysInfo ? percentBar(sysInfo.swap_used_mb, sysInfo.swap_total_mb) : null
  const cpu = sysInfo && sysInfo.cpu_percent !== undefined ? percentBar(sysInfo.cpu_percent, 100) : null

  // Deduplicate MySQL/MariaDB services
  const displayServices = services.filter(s => {
    if (s.name === 'mysql' && services.some(x => x.name === 'mysqld' && x.active)) return false
    return true
  })

  const serviceLabel = (name: string) => {
    const map: Record<string, string> = {
      nginx: 'Nginx',
      mysqld: 'MySQL',
      mariadb: 'MariaDB',
      mysql: 'MySQL',
      'php-fpm': 'PHP-FPM',
    }
    return map[name] || name
  }

  return (
    <div className="sp-dashboard">
      {/* Header */}
      <div className="sp-dash-header">
        <div className="sp-dash-title">
          <h2>Dashboard</h2>
          {sysInfo && (
            <span className="sp-dash-host">{sysInfo.os.hostname}</span>
          )}
          <p className="sp-dash-welcome">{t('dashboard.welcome')}</p>
        </div>
        <button className="sp-refresh-btn" onClick={fetchData} disabled={loading}>
          {loading ? t('common.refreshing') : t('common.refresh')}
        </button>
      </div>

      {/* System Info Card */}
      {sysInfo && (
        <div className="sp-card">
          <div className="sp-card-title">{t('dashboard.system')}</div>
          <div className="sp-info-grid">
            <div className="sp-info-item">
              <span className="sp-info-label">{t('dashboard.os')}</span>
              <span className="sp-info-value">{sysInfo.os.distro} {sysInfo.os.version}</span>
            </div>
            <div className="sp-info-item">
              <span className="sp-info-label">{t('dashboard.kernel')}</span>
              <span className="sp-info-value">{sysInfo.os.kernel}</span>
            </div>
            <div className="sp-info-item">
              <span className="sp-info-label">{t('dashboard.architecture')}</span>
              <span className="sp-info-value">{sysInfo.os.arch}</span>
            </div>
            <div className="sp-info-item">
              <span className="sp-info-label">{t('dashboard.hostname')}</span>
              <span className="sp-info-value">{sysInfo.os.hostname}</span>
            </div>
            <div className="sp-info-item">
              <span className="sp-info-label">{t('dashboard.uptime')}</span>
              <span className="sp-info-value">{sysInfo.uptime}</span>
            </div>
            <div className="sp-info-item">
              <span className="sp-info-label">{t('dashboard.loadAverage')}</span>
              <span className="sp-info-value">{sysInfo.load_avg}</span>
            </div>
          </div>
        </div>
      )}

      {/* Resources Card */}
      {sysInfo && (
        <div className="sp-card">
          <div className="sp-card-title">{t('dashboard.resources')}</div>
          <div className="sp-resource-list">
            {/* CPU */}
            {cpu && (
              <div className="sp-resource-item">
                <div className="sp-resource-header">
                  <span>{t('dashboard.cpu')}</span>
                  <span>{sysInfo.cpu_percent}% - {sysInfo.cpu_model} ({sysInfo.cpu_cores} {t('dashboard.cores')})</span>
                </div>
                <div className="sp-progress-track">
                  <div className="sp-progress-fill" style={{ width: `${cpu.percent}%`, background: cpu.color }} />
                </div>
              </div>
            )}
            {/* Memory */}
            {mem && (
              <div className="sp-resource-item">
                <div className="sp-resource-header">
                  <span>{t('dashboard.memory')}</span>
                  <span>{formatMb(sysInfo.mem_used_mb)} / {formatMb(sysInfo.mem_total_mb)} ({mem.percent}%)</span>
                </div>
                <div className="sp-progress-track">
                  <div className="sp-progress-fill" style={{ width: `${mem.percent}%`, background: mem.color }} />
                </div>
              </div>
            )}
            {/* Swap */}
            {swap && sysInfo.swap_total_mb > 0 && (
              <div className="sp-resource-item">
                <div className="sp-resource-header">
                  <span>{t('dashboard.swap')}</span>
                  <span>{formatMb(sysInfo.swap_used_mb)} / {formatMb(sysInfo.swap_total_mb)} ({swap.percent}%)</span>
                </div>
                <div className="sp-progress-track">
                  <div className="sp-progress-fill" style={{ width: `${swap.percent}%`, background: swap.color }} />
                </div>
              </div>
            )}
            {/* Disks */}
            {sysInfo.disks.map((d, i) => {
              const pct = parseInt(d.use_percent) || 0
              const color = pct > 90 ? '#f85149' : pct > 70 ? '#d29922' : '#3fb950'
              return (
                <div className="sp-resource-item" key={i}>
                  <div className="sp-resource-header">
                    <span>{t('dashboard.disk')} {d.mount}</span>
                    <span>{d.used} / {d.size} ({d.use_percent})</span>
                  </div>
                  <div className="sp-progress-track">
                    <div className="sp-progress-fill" style={{ width: `${pct}%`, background: color }} />
                  </div>
                </div>
              )
            })}
          </div>
        </div>
      )}

      {/* Services Card */}
      <div className="sp-card">
        <div className="sp-card-title">{t('dashboard.services')}</div>
        {displayServices.length === 0 ? (
          <div className="sp-services-empty">
            <p>{t('dashboard.noLnmp')}</p>
            {onNavigate && (
              <button className="install-nav-btn" onClick={() => onNavigate('install')}>
                {t('dashboard.installLnmp')}
              </button>
            )}
          </div>
        ) : (
          <div className="sp-service-grid">
            {displayServices.map(svc => (
              <div className="sp-service-card" key={svc.name}>
                <div className="sp-service-status">
                  <span className={`sp-status-dot ${svc.active ? 'active' : 'inactive'}`} />
                  <span className="sp-service-name">{serviceLabel(svc.name)}</span>
                </div>
                <div className="sp-service-meta">
                  {svc.version && <span className="sp-service-version">v{svc.version}</span>}
                  <span className={`sp-service-state ${svc.active ? 'running' : 'stopped'}`}>
                    {svc.active ? t('common.running') : t('common.stopped')}
                  </span>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
