import { useState, useEffect, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/core'

interface BbrStatus {
  enabled: boolean
  congestion_control: string
  qdisc: string
}

interface BbrPanelProps {
  sessionId: string | null
}

export default function BbrPanel({ sessionId }: BbrPanelProps) {
  const [bbrStatus, setBbrStatus] = useState<BbrStatus | null>(null)
  const [bbrLoading, setBbrLoading] = useState(false)
  const [bbrSaving, setBbrSaving] = useState(false)
  const [bbrError, setBbrError] = useState('')
  const [bbrSuccess, setBbrSuccess] = useState('')

  const fetchBbrStatus = useCallback(async () => {
    if (!sessionId) return
    setBbrLoading(true)
    try {
      const status = await invoke<BbrStatus>('server_get_bbr_status', { sessionId })
      setBbrStatus(status)
    } catch {
      setBbrStatus(null)
    } finally {
      setBbrLoading(false)
    }
  }, [sessionId])

  useEffect(() => { fetchBbrStatus() }, [fetchBbrStatus])

  const handleToggle = async () => {
    if (!sessionId || !bbrStatus) return
    const enable = !bbrStatus.enabled
    setBbrSaving(true)
    setBbrError('')
    setBbrSuccess('')
    try {
      const result = await invoke<string>('server_set_bbr_status', { sessionId, enable })
      setBbrSuccess(result)
      await fetchBbrStatus()
    } catch (e) {
      setBbrError(String(e))
      await fetchBbrStatus()
    } finally {
      setBbrSaving(false)
    }
  }

  if (!sessionId) return <div className="sp-empty">Connect to a server first</div>

  return (
    <div className="settings-panel">
      <h2 className="settings-panel-title">BBR Acceleration</h2>

      <div className="settings-section">
        <div className="settings-section-header">TCP Congestion Control</div>
        <div className="settings-section-body">
          <div className="settings-row">
            <span className="settings-label">
              BBR Congestion Control Algorithm
              {bbrStatus && (
                <span className="settings-label-detail">
                  Current algorithm: <code>{bbrStatus.congestion_control}</code> / Queue discipline: <code>{bbrStatus.qdisc}</code>
                </span>
              )}
            </span>
            <div className="settings-row-right">
              {bbrLoading && <span className="settings-muted">Loading...</span>}
              {bbrStatus && (
                <button
                  className={`firewall-toggle ${bbrStatus.enabled ? 'on' : 'off'} ${bbrSaving ? 'loading' : ''}`}
                  onClick={handleToggle}
                  disabled={bbrSaving}
                >
                  <span className="toggle-track">
                    <span className="toggle-thumb" />
                  </span>
                  <span className="toggle-label">{bbrStatus.enabled ? 'ON' : 'OFF'}</span>
                </button>
              )}
            </div>
          </div>
          {bbrError && <div className="settings-error">{bbrError}</div>}
          {bbrSuccess && <div className="settings-success">{bbrSuccess}</div>}
          <div className="settings-hint">
            BBR is a TCP congestion control algorithm developed by Google that improves network throughput and reduces latency. Requires Linux kernel 4.9 or higher.
          </div>
        </div>
      </div>
    </div>
  )
}
