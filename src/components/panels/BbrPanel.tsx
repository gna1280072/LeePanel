import { useState, useEffect, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { useTranslation } from 'react-i18next'

interface BbrStatus {
  enabled: boolean
  congestion_control: string
  qdisc: string
}

interface BbrPanelProps {
  sessionId: string | null
}

export default function BbrPanel({ sessionId }: BbrPanelProps) {
  const { t } = useTranslation()
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

  if (!sessionId) return <div className="sp-empty">{t('common.connectFirst')}</div>

  return (
    <div className="settings-panel">
      <h2 className="settings-panel-title">{t('bbr.title')}</h2>

      <div className="settings-section">
        <div className="settings-section-header">{t('bbr.tcpCongestion')}</div>
        <div className="settings-section-body">
          <div className="settings-row">
            <span className="settings-label">
              {t('bbr.bbrAlgorithm')}
              {bbrStatus && (
                <span className="settings-label-detail">
                  {t('bbr.currentAlgorithm', { algorithm: bbrStatus.congestion_control, qdisc: bbrStatus.qdisc })}
                </span>
              )}
            </span>
            <div className="settings-row-right">
              {bbrLoading && <span className="settings-muted">{t('common.loading')}</span>}
              {bbrStatus && (
                <button
                  className={`firewall-toggle ${bbrStatus.enabled ? 'on' : 'off'} ${bbrSaving ? 'loading' : ''}`}
                  onClick={handleToggle}
                  disabled={bbrSaving}
                >
                  <span className="toggle-track">
                    <span className="toggle-thumb" />
                  </span>
                  <span className="toggle-label">{bbrStatus.enabled ? t('common.on') : t('common.off')}</span>
                </button>
              )}
            </div>
          </div>
          {bbrError && <div className="settings-error">{bbrError}</div>}
          {bbrSuccess && <div className="settings-success">{bbrSuccess}</div>}
          <div className="settings-hint">
            {t('bbr.bbrHint')}
          </div>
        </div>
      </div>
    </div>
  )
}
