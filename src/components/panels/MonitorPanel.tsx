import { useState, useEffect, useRef, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { useTranslation } from 'react-i18next'

interface MonitorData {
  cpu_percent: number
  mem_total_mb: number
  mem_used_mb: number
  swap_total_mb: number
  swap_used_mb: number
  load_avg: string
  net_rx: string
  net_tx: string
  disk_read: string
  disk_write: string
  top_processes: ProcessInfo[]
  uptime: string
}

interface ProcessInfo {
  pid: string
  user: string
  cpu: string
  mem: string
  command: string
}

interface MonitorPanelProps {
  sessionId: string | null
}

const REFRESH_INTERVALS = [3, 5, 10, 30]

export default function MonitorPanel({ sessionId }: MonitorPanelProps) {
  const { t } = useTranslation()
  const [data, setData] = useState<MonitorData | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [interval, setInterval_] = useState(5)
  const [autoRefresh, setAutoRefresh] = useState(true)
  const [history, setHistory] = useState<{ cpu: number; mem: number; ts: number }[]>([])
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null)

  const fetchData = useCallback(async () => {
    if (!sessionId) return
    setLoading(true)
    setError('')
    try {
      const result = await invoke<MonitorData>('server_get_monitor_data', { sessionId })
      setData(result)
      // Add to history (keep last 60 points)
      setHistory((prev) => {
        const next = [...prev, {
          cpu: result.cpu_percent,
          mem: result.mem_total_mb > 0 ? Math.round((result.mem_used_mb / result.mem_total_mb) * 100) : 0,
          ts: Date.now(),
        }]
        return next.length > 60 ? next.slice(-60) : next
      })
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }, [sessionId])

  useEffect(() => {
    fetchData()
  }, [fetchData])

  useEffect(() => {
    if (intervalRef.current) clearInterval(intervalRef.current)
    if (autoRefresh && sessionId) {
      intervalRef.current = setInterval(fetchData, interval * 1000)
    }
    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current)
    }
  }, [autoRefresh, interval, fetchData, sessionId])

  if (!sessionId) return <div className="sp-empty">{t('common.connectFirst')}</div>

  const memPercent = data ? (data.mem_total_mb > 0 ? Math.round((data.mem_used_mb / data.mem_total_mb) * 100) : 0) : 0
  const swapPercent = data ? (data.swap_total_mb > 0 ? Math.round((data.swap_used_mb / data.swap_total_mb) * 100) : 0) : 0

  return (
    <div className="monitor-panel">
      <div className="monitor-header">
        <h2>{t('monitor.title')}</h2>
        <div className="monitor-controls">
          {data && <span className="monitor-uptime">{data.uptime}</span>}
          <label className="monitor-auto">
            <input type="checkbox" checked={autoRefresh} onChange={(e) => setAutoRefresh(e.target.checked)} />
            {t('monitor.auto')}
          </label>
          <select
            className="monitor-interval"
            value={interval}
            onChange={(e) => setInterval_(Number(e.target.value))}
          >
            {REFRESH_INTERVALS.map((s) => (
              <option key={s} value={s}>{s}s</option>
            ))}
          </select>
          <button className="svc-cfg-btn" onClick={fetchData} disabled={loading}>
            {loading ? '...' : t('common.refresh')}
          </button>
        </div>
      </div>

      {error && <div className="svc-error">{error}</div>}

      {data && (
        <>
          {/* Resource Gauges */}
          <div className="monitor-gauges">
            <GaugeCard label={t('monitor.cpu')} percent={data.cpu_percent} color={gaugeColor(data.cpu_percent)} />
            <GaugeCard label={t('monitor.memory')} percent={memPercent} detail={`${formatMb(data.mem_used_mb)} / ${formatMb(data.mem_total_mb)}`} color={gaugeColor(memPercent)} />
            {data.swap_total_mb > 0 && (
              <GaugeCard label={t('monitor.swap')} percent={swapPercent} detail={`${formatMb(data.swap_used_mb)} / ${formatMb(data.swap_total_mb)}`} color={gaugeColor(swapPercent)} />
            )}
            <InfoCard label={t('monitor.loadAverage')} value={data.load_avg} />
            <InfoCard label={t('monitor.network')} value={`↓ ${data.net_rx} / ↑ ${data.net_tx}`} />
            <InfoCard label={t('monitor.diskIO')} value={`R ${data.disk_read} / W ${data.disk_write}`} />
          </div>

          {/* Mini History Chart */}
          {history.length > 2 && (
            <div className="monitor-chart-card">
              <div className="monitor-chart-title">{t('monitor.cpuMemHistory', { count: history.length })}</div>
              <MiniChart data={history} />
            </div>
          )}

          {/* Top Processes */}
          <div className="monitor-processes-card">
            <div className="monitor-card-title">{t('monitor.topProcesses')}</div>
            <table className="monitor-proc-table">
              <thead>
                <tr>
                  <th>{t('monitor.pid')}</th>
                  <th>{t('monitor.user')}</th>
                  <th>{t('monitor.cpuPercent')}</th>
                  <th>{t('monitor.memPercent')}</th>
                  <th>{t('monitor.command')}</th>
                </tr>
              </thead>
              <tbody>
                {data.top_processes.map((proc, i) => (
                  <tr key={i}>
                    <td className="mono">{proc.pid}</td>
                    <td>{proc.user}</td>
                    <td className="mono">{proc.cpu}</td>
                    <td className="mono">{proc.mem}</td>
                    <td className="mono cmd">{proc.command}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </>
      )}

      {loading && !data && <div className="sp-loading">{t('monitor.loadingData')}</div>}
    </div>
  )
}

function gaugeColor(percent: number): string {
  if (percent > 90) return '#f85149'
  if (percent > 70) return '#d29922'
  return '#3fb950'
}

function formatMb(mb: number): string {
  if (mb >= 1024) return `${(mb / 1024).toFixed(1)} GB`
  return `${mb} MB`
}

function GaugeCard({ label, percent, detail, color }: { label: string; percent: number; detail?: string; color: string }) {
  return (
    <div className="monitor-gauge-card">
      <div className="monitor-gauge-label">{label}</div>
      <div className="monitor-gauge-value" style={{ color }}>{percent}%</div>
      <div className="sp-progress-track">
        <div className="sp-progress-fill" style={{ width: `${percent}%`, background: color }} />
      </div>
      {detail && <div className="monitor-gauge-detail">{detail}</div>}
    </div>
  )
}

function InfoCard({ label, value }: { label: string; value: string }) {
  return (
    <div className="monitor-info-card">
      <div className="monitor-info-label">{label}</div>
      <div className="monitor-info-value">{value}</div>
    </div>
  )
}

function MiniChart({ data }: { data: { cpu: number; mem: number; ts: number }[] }) {
  const width = 600
  const height = 80
  const padding = 4

  const stepX = data.length > 1 ? (width - padding * 2) / (data.length - 1) : 0

  const cpuPath = data
    .map((d, i) => `${i === 0 ? 'M' : 'L'}${padding + i * stepX},${height - padding - (d.cpu / 100) * (height - padding * 2)}`)
    .join(' ')

  const memPath = data
    .map((d, i) => `${i === 0 ? 'M' : 'L'}${padding + i * stepX},${height - padding - (d.mem / 100) * (height - padding * 2)}`)
    .join(' ')

  return (
    <svg viewBox={`0 0 ${width} ${height}`} className="monitor-chart-svg">
      {/* Grid lines */}
      <line x1={padding} y1={height / 2} x2={width - padding} y2={height / 2} stroke="#21262d" strokeWidth="0.5" />
      <line x1={padding} y1={padding} x2={padding} y2={height - padding} stroke="#21262d" strokeWidth="0.5" />
      {/* CPU line */}
      <path d={cpuPath} fill="none" stroke="#58a6ff" strokeWidth="1.5" />
      {/* Memory line */}
      <path d={memPath} fill="none" stroke="#3fb950" strokeWidth="1.5" />
    </svg>
  )
}
