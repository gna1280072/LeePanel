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

const KEY_TYPES = [
  'ssh-ed25519',
  'ssh-rsa',
  'ecdsa-sha2-nistp256',
  'ecdsa-sha2-nistp384',
  'ecdsa-sha2-nistp521',
]

/** Global dialog to view/delete/import trusted SSH host fingerprints (TOFU known_hosts). */
export default function HostKeysDialog({ open, onClose }: Props) {
  const { t } = useTranslation()
  const [items, setItems] = useState<KnownHost[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')
  const [importing, setImporting] = useState(false)
  const [showAdd, setShowAdd] = useState(false)
  const [addHost, setAddHost] = useState('')
  const [addKeyType, setAddKeyType] = useState(KEY_TYPES[0])
  const [addFingerprint, setAddFingerprint] = useState('')
  const [addError, setAddError] = useState('')

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

  const importFromSsh = async () => {
    setImporting(true)
    setError('')
    try {
      const count = await invoke<number>('known_hosts_import_from_ssh')
      setError(t('hostKey.importedCount', { count: String(count) }))
      await load()
    } catch (e) {
      setError(t('hostKey.importFailed', { error: String(e) }))
    } finally {
      setImporting(false)
    }
  }

  const add = async () => {
    setAddError('')
    const host = addHost.trim()
    const fp = addFingerprint.trim()
    if (!host || !fp) {
      setAddError(t('hostKey.invalidInput'))
      return
    }
    try {
      await invoke('known_hosts_add', { host, keyType: addKeyType, fingerprint: fp })
      setAddHost('')
      setAddFingerprint('')
      setShowAdd(false)
      await load()
    } catch (e) {
      setAddError(String(e))
    }
  }

  if (!open) return null

  return (
    <div className="error-dialog-overlay" onClick={onClose}>
      <div className="error-dialog hostkeys-dialog" onClick={(e) => e.stopPropagation()}>
        <button className="error-dialog-close" onClick={onClose}>×</button>
        <div className="error-dialog-icon">🔐</div>
        <div className="error-dialog-title">{t('hostKey.manageTitle')}</div>
        {error && <div className="hostkeys-msg">{error}</div>}
        <div className="hostkeys-toolbar">
          <button className="hostkeys-tool-btn" onClick={importFromSsh} disabled={importing}>
            📥 {importing ? '…' : t('hostKey.importFromSsh')}
          </button>
          <button className="hostkeys-tool-btn" onClick={() => setShowAdd(v => !v)}>
            ➕ {t('hostKey.manualAdd')}
          </button>
        </div>
        {showAdd && (
          <div className="hostkeys-add">
            <div className="hostkeys-add-row">
              <input
                className="hostkeys-input"
                placeholder={t('hostKey.hostPlaceholder')}
                value={addHost}
                onChange={(e) => setAddHost(e.target.value)}
              />
              <select className="hostkeys-select" value={addKeyType} onChange={(e) => setAddKeyType(e.target.value)}>
                {KEY_TYPES.map((kt) => <option key={kt} value={kt}>{kt}</option>)}
              </select>
            </div>
            <input
              className="hostkeys-input"
              placeholder={t('hostKey.fingerprintPlaceholder')}
              value={addFingerprint}
              onChange={(e) => setAddFingerprint(e.target.value)}
            />
            <div className="hostkeys-hint">{t('hostKey.fingerprintHint')}</div>
            {addError && <div className="settings-error">{addError}</div>}
            <button className="error-dialog-btn" onClick={add}>{t('hostKey.add')}</button>
          </div>
        )}
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
              >{t('common.delete')}</button>
            </div>
          ))}
        </div>
        <button className="error-dialog-btn secondary" onClick={onClose}>{t('common.close')}</button>
      </div>
    </div>
  )
}
