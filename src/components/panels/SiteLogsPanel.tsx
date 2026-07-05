import { useState, useEffect, useCallback, useRef } from 'react'
import { invoke } from '@tauri-apps/api/core'

interface SiteInfo {
  domain: string
  root: string
  config_path: string
  ssl: boolean
  php_version: string
  enabled: boolean
  created_at: number
}

interface SiteLogInfo {
  path: string
  log_type: string
  size: number
}

interface SiteLogsPanelProps {
  sessionId: string | null
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`
}

export default function SiteLogsPanel({ sessionId }: SiteLogsPanelProps) {
  const [sites, setSites] = useState<SiteInfo[]>([])
  const [sitesLoading, setSitesLoading] = useState(true)
  const [selectedSite, setSelectedSite] = useState<string>('')
  const [logs, setLogs] = useState<SiteLogInfo[]>([])
  const [logsLoading, setLogsLoading] = useState(false)
  const [selectedLog, setSelectedLog] = useState<SiteLogInfo | null>(null)
  const [logContent, setLogContent] = useState('')
  const [logLoading, setLogLoading] = useState(false)
  const [logLines, setLogLines] = useState(1000)
  const [dateFrom, setDateFrom] = useState('')
  const [dateTo, setDateTo] = useState('')
  const [searchTerm, setSearchTerm] = useState('')
  const [error, setError] = useState('')
  const logRef = useRef<HTMLPreElement>(null)
  const readInProgress = useRef(false)
  const pendingRead = useRef(false)

  const fetchSites = useCallback(async () => {
    if (!sessionId) return
    setSitesLoading(true)
    try {
      const list = await invoke<SiteInfo[]>('server_list_sites', { sessionId })
      list.sort((a, b) => b.created_at - a.created_at)
      setSites(list)
      setSelectedSite(prev => prev || (list.length > 0 ? list[0].domain : ''))
    } catch (e) {
      setError(String(e))
    } finally {
      setSitesLoading(false)
    }
  }, [sessionId])

  const fetchLogs = useCallback(async () => {
    if (!sessionId || !selectedSite) return
    setLogsLoading(true)
    setLogs([])
    setSelectedLog(null)
    setLogContent('')
    try {
      const list = await invoke<SiteLogInfo[]>('server_get_site_logs', { sessionId, domain: selectedSite })
      setLogs(list)
      if (list.length > 0) {
        setSelectedLog(list[0])
      }
    } catch (e) {
      setError(String(e))
    } finally {
      setLogsLoading(false)
    }
  }, [sessionId, selectedSite])

  const doReadLog = useCallback(async () => {
    if (!sessionId || !selectedLog) return
    readInProgress.current = true
    setLogLoading(true)
    setError('')
    try {
      const content = await invoke<string>('server_read_site_log', {
        sessionId,
        logPath: selectedLog.path,
        lines: logLines,
        dateFrom: dateFrom || null,
        dateTo: dateTo || null,
      })
      setLogContent(content)
    } catch (e) {
      setError(String(e))
    } finally {
      setLogLoading(false)
      readInProgress.current = false
    }
  }, [sessionId, selectedLog?.path, logLines, dateFrom, dateTo])

  // Queue-based read: if busy, mark pending and wait for current to finish
  const readLog = useCallback(async () => {
    if (readInProgress.current) {
      pendingRead.current = true
      return
    }
    await doReadLog()
    // Process any pending read that arrived during the current one
    while (pendingRead.current) {
      pendingRead.current = false
      await doReadLog()
    }
  }, [doReadLog])

  useEffect(() => { fetchSites() }, [fetchSites])
  useEffect(() => { fetchLogs() }, [fetchLogs])
  useEffect(() => { readLog() }, [readLog])

  // Auto-scroll to bottom when content loads
  useEffect(() => {
    if (logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight
    }
  }, [logContent])

  const filteredContent = searchTerm
    ? logContent.split('\n').filter(line => line.toLowerCase().includes(searchTerm.toLowerCase())).join('\n')
    : logContent

  if (!sessionId) return <div className="sp-empty">Connect to a server first</div>

  return (
    <div className="site-logs-panel">
      <div className="site-logs-header">
        <div className="site-logs-toolbar">
          <select
            className="site-logs-select"
            value={selectedSite}
            onChange={(e) => setSelectedSite(e.target.value)}
            disabled={sitesLoading}
          >
            {sitesLoading && <option value="">Loading sites...</option>}
            {sites.length === 0 && !sitesLoading && <option value="">No sites found</option>}
            {sites.map(s => (
              <option key={s.domain} value={s.domain}>{s.domain}</option>
            ))}
          </select>

          {logs.length > 0 && (
            <select
              className="site-logs-select"
              value={selectedLog?.path || ''}
              onChange={(e) => {
                const log = logs.find(l => l.path === e.target.value)
                setSelectedLog(log || null)
              }}
            >
              {logs.map(l => (
                <option key={l.path} value={l.path}>
                  {l.log_type === 'access' ? '📗 Access' : '📕 Error'} — {l.path} ({formatSize(l.size)})
                </option>
              ))}
            </select>
          )}

          <input
            className="site-logs-lines-input"
            type="number"
            min={10}
            max={10000}
            value={logLines}
            onChange={(e) => setLogLines(Math.max(10, Math.min(10000, Number(e.target.value))))}
            title="Number of lines"
          />

          <input
            className="site-logs-date-input"
            type="datetime-local"
            value={dateFrom}
            onChange={(e) => setDateFrom(e.target.value)}
            title="From date/time"
          />
          <span className="site-logs-date-sep">~</span>
          <input
            className="site-logs-date-input"
            type="datetime-local"
            value={dateTo}
            onChange={(e) => setDateTo(e.target.value)}
            title="To date/time"
          />
          {(dateFrom || dateTo) && (
            <button className="svc-cfg-btn" onClick={() => { setDateFrom(''); setDateTo('') }} title="Clear date range">
              ✕
            </button>
          )}

          <button className="svc-cfg-btn" onClick={readLog} disabled={!selectedLog}>
            🔄 Refresh
          </button>
        </div>

        <div className="site-logs-search">
          <input
            className="site-logs-search-input"
            placeholder="Search in log..."
            value={searchTerm}
            onChange={(e) => setSearchTerm(e.target.value)}
          />
          {searchTerm && (
            <span className="site-logs-search-count">
              {filteredContent.split('\n').filter(Boolean).length} matches
            </span>
          )}
        </div>
      </div>

      {error && <div className="settings-error">{error}</div>}

      {logsLoading && <div className="site-logs-status">Loading log files...</div>}
      {!logsLoading && selectedSite && logs.length === 0 && (
        <div className="site-logs-status">No log files found for {selectedSite}</div>
      )}

      <pre ref={logRef} className="site-logs-content">
        {logLoading ? 'Loading log content...' : filteredContent || 'Select a log file to view'}
      </pre>
    </div>
  )
}
