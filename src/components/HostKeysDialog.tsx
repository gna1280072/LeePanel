import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { useTranslation } from 'react-i18next'

interface KnownHost {
  host: string
  key_type: string
  fingerprint: string
  first_seen: number
  last_seen: number
}

interface Props {
  open: boolean
  onClose: () => void
}

/** Global dialog to view/delete trusted SSH host fingerprints (TOFU known_hosts). */
export default function HostKeysDialog({ open, onClose }: Props) {
  const { t } = useTranslation()
  const [items, setItems] = useState<KnownHost[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')

  const load = async () => {
    setLoading(true)
    setError('')
    try {
      setItems(await invoke<KnownHost[]>('known_hosts_list'))
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    if (open) load()
  }, [open])

  const remove = async (host: string, keyType: string) => {
    if (!window.confirm(t('hostKey.deleteConfirm'))) return
    try {
      await invoke('known_hosts_delete', { host, keyType })
      await load()
    } catch (e) {
      setError(String(e))
    }
  }

  if (!open) return null

  return (
    <div className="error-dialog-overlay" onClick={onClose}>
      <div className="error-dialog hostkeys-dialog" onClick={(e) => e.stopPropagation()}>
        <button className="error-dialog-close" onClick={onClose}>×</button>
        <div className="error-dialog-icon">🔐</div>
        <div className="error-dialog-title">{t('hostKey.manageTitle')}</div>
        {error && <div className="settings-error">{error}</div>}
        <div className="hostkeys-list">
          {loading && <div className="hostkeys-empty">…</div>}
          {!loading && items.length === 0 && <div className="hostkeys-empty">{t('hostKey.manageEmpty')}</div>}
          {!loading && items.map((it) => (
            <div className="hostkeys-row" key={`${it.host}__${it.key_type}`}>
              <div className="hostkeys-main">
                <div className="hostkeys-host">
                  {it.host}
                  <span className="hostkeys-alg">{it.key_type}</span>
                </div>
                <div className="hostkeys-fp"><code>SHA256:{it.fingerprint}</code></div>
              </div>
              <button
                className="hostkeys-delete"
                onClick={() => remove(it.host, it.key_type)}
                title={t('common.delete')}
              >🗑</button>
            </div>
          ))}
        </div>
        <button className="error-dialog-btn secondary" onClick={onClose}>{t('common.close')}</button>
      </div>
    </div>
  )
}
