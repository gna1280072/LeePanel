import { useCallback, useEffect, useState } from 'react'
import { invoke } from '../sudoPrompt'
import { useTranslation } from 'react-i18next'

interface AuditEntry {
  id: number
  ts: number
  server_host: string
  server_username: string
  op: string
  command: string
  result: string
  detail: string
}

const OP_LABELS: Record<string, string> = {
  port_kill: 'port_kill',
  service_action: 'service_action',
  firewall_add: 'firewall_add',
  firewall_remove: 'firewall_remove',
  firewall_toggle: 'firewall_toggle',
  docker_container_action: 'docker_container_action',
  docker_container_remove: 'docker_container_remove',
  docker_image_remove: 'docker_image_remove',
  site_toggle: 'site_toggle',
  site_delete: 'site_delete',
  server_reboot: 'server_reboot',
  software_action: 'software_action',
  custom_software_action: 'custom_software_action',
  tunnel_create: 'tunnel_create',
  tunnel_close: 'tunnel_close',
  tunnel_delete: 'tunnel_delete',
  tunnel_restore: 'tunnel_restore',
}

function formatTime(ts: number): string {
  const d = new Date(ts * 1000)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
}

interface AuditLogDialogProps {
  open: boolean
  onClose: () => void
}

export default function AuditLogDialog({ open, onClose }: AuditLogDialogProps) {
  const { t } = useTranslation()
  const [entries, setEntries] = useState<AuditEntry[]>([])
  const [loading, setLoading] = useState(false)
  const [confirmClear, setConfirmClear] = useState(false)

  const fetchEntries = useCallback(async () => {
    setLoading(true)
    try {
      const list = await invoke<AuditEntry[]>('audit_list', { limit: 100 })
      setEntries(list)
    } catch {
      setEntries([])
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    if (open) {
      setConfirmClear(false)
      fetchEntries()
    }
  }, [open, fetchEntries])

  if (!open) return null

  const handleClear = async () => {
    if (!confirmClear) { setConfirmClear(true); return }
    try {
      await invoke('audit_clear')
      setConfirmClear(false)
      setEntries([])
    } catch { /* ignore */ }
  }

  return (
    <div className="sidebar-confirm-overlay" onClick={onClose}>
      <div className="audit-dialog" onClick={(e) => e.stopPropagation()}>
        <div className="sidebar-edit-header">
          <div className="sidebar-confirm-title">{t('audit.title')}</div>
          <button className="sidebar-edit-close" onClick={onClose}>×</button>
        </div>
        <div className="audit-body">
          {loading && <div className="audit-empty">{t('common.loading')}</div>}
          {!loading && entries.length === 0 && (
            <div className="audit-empty">{t('audit.empty')}</div>
          )}
          {!loading && entries.length > 0 && (
            <div className="audit-list">
              {entries.map(e => (
                <div key={e.id} className={`audit-item ${e.result === 'error' ? 'error' : ''}`}>
                  <div className="audit-item-head">
                    <span className={`audit-result ${e.result}`}>{e.result === 'error' ? t('audit.error') : t('audit.success')}</span>
                    <span className="audit-op">{t(`audit.op.${OP_LABELS[e.op] || e.op}`)}</span>
                    <span className="audit-time">{formatTime(e.ts)}</span>
                  </div>
                  <div className="audit-item-cmd">
                    <span className="audit-server">{e.server_username}@{e.server_host}</span>
                    <code>{e.command}</code>
                  </div>
                  {e.detail && <div className="audit-item-detail">{e.detail}</div>}
                </div>
              ))}
            </div>
          )}
        </div>
        <div className="sidebar-confirm-actions">
          <button className="sidebar-confirm-btn danger" onClick={handleClear} disabled={entries.length === 0}>
            {confirmClear ? t('audit.clearConfirm') : t('audit.clear')}
          </button>
          <button className="sidebar-confirm-btn primary" onClick={onClose}>{t('common.close')}</button>
        </div>
      </div>
    </div>
  )
}
