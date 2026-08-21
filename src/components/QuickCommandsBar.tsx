import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

interface QuickCommand {
  id: string
  name: string
  command: string
  group?: string
  sort: number
}

interface QuickCommandsBarProps {
  sessionId: string | null
  /** Send a command into the terminal; autoEnter=true runs it, false fills it for review */
  onSendCommand: (cmd: string, autoEnter: boolean) => void
  onShowToast?: (msg: string) => void
}

const STORAGE_KEY = 'leepanel.quickCommands'
const AUTO_ENTER_KEY = 'leepanel.quickCommandsAutoEnter'
const MAX_COMMANDS = 20

// Seed templates merged in on first run; user can freely edit/delete them
const DEFAULT_COMMANDS: { name: string; command: string; group: string }[] = [
  { name: 'htop', command: 'htop', group: 'System' },
  { name: 'Disk usage', command: 'df -h', group: 'System' },
  { name: 'Memory', command: 'free -m', group: 'System' },
  { name: 'Uptime', command: 'uptime', group: 'System' },
  { name: 'Clear screen', command: 'clear', group: 'System' },
  { name: 'List directory', command: 'dir', group: 'System' },
  { name: 'List files', command: 'ls', group: 'System' },
  { name: 'Parent directory', command: 'cd ..', group: 'System' },
]

const newId = () => `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`

function loadCommands(): QuickCommand[] {
  const seed = () => DEFAULT_COMMANDS.map((d, i) => ({ ...d, id: newId(), sort: i }))
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (raw === null) return seed()
    const parsed = JSON.parse(raw)
    if (!Array.isArray(parsed)) return []
    return parsed
      .filter((c): c is QuickCommand => !!c && typeof c.name === 'string' && typeof c.command === 'string')
      .map(c => ({
        id: typeof c.id === 'string' ? c.id : newId(),
        name: c.name,
        command: c.command,
        group: typeof c.group === 'string' && c.group ? c.group : undefined,
        sort: typeof c.sort === 'number' ? c.sort : 0,
      }))
  } catch {
    return seed()
  }
}

export default function QuickCommandsBar({ sessionId, onSendCommand, onShowToast }: QuickCommandsBarProps) {
  const { t } = useTranslation()
  const [commands, setCommands] = useState<QuickCommand[]>(loadCommands)
  // ponytail: default to fill-only (no auto-enter); user opts in via the manage modal
  const [autoEnter, setAutoEnter] = useState(() => {
    try { return localStorage.getItem(AUTO_ENTER_KEY) === '1' } catch { return false }
  })
  const [showManage, setShowManage] = useState(false)
  const [formOpen, setFormOpen] = useState(false)
  const [editingId, setEditingId] = useState<string | null>(null)
  const [formName, setFormName] = useState('')
  const [formCommand, setFormCommand] = useState('')
  const [formGroup, setFormGroup] = useState('')
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null)

  useEffect(() => {
    try { localStorage.setItem(STORAGE_KEY, JSON.stringify(commands)) } catch { /* storage full - ignore */ }
  }, [commands])

  useEffect(() => {
    try { localStorage.setItem(AUTO_ENTER_KEY, autoEnter ? '1' : '0') } catch { /* ignore */ }
  }, [autoEnter])

  const sorted = [...commands].sort((a, b) => a.sort - b.sort)

  const handleChipClick = (cmd: QuickCommand) => {
    if (!sessionId) {
      onShowToast?.(`⚠ ${t('common.connectFirst')}`)
      return
    }
    onSendCommand(cmd.command, autoEnter)
  }

  const openAdd = () => {
    setEditingId(null)
    setFormName('')
    setFormCommand('')
    setFormGroup('')
    setFormOpen(true)
  }

  const openEdit = (c: QuickCommand) => {
    setEditingId(c.id)
    setFormName(c.name)
    setFormCommand(c.command)
    setFormGroup(c.group || '')
    setFormOpen(true)
  }

  const saveForm = () => {
    if (!formName.trim()) { onShowToast?.(`⚠ ${t('quickCommands.nameRequired')}`); return }
    if (!formCommand.trim()) { onShowToast?.(`⚠ ${t('quickCommands.commandRequired')}`); return }
    if (editingId === null && commands.length >= MAX_COMMANDS) {
      onShowToast?.(`⚠ ${t('quickCommands.maxReached', { max: MAX_COMMANDS })}`)
      return
    }
    if (editingId !== null) {
      setCommands(prev => prev.map(c =>
        c.id === editingId
          ? { ...c, name: formName.trim(), command: formCommand.trim(), group: formGroup.trim() || undefined }
          : c
      ))
    } else {
      const maxSort = commands.reduce((m, c) => Math.max(m, c.sort), -1)
      setCommands(prev => [...prev, {
        id: newId(),
        name: formName.trim(),
        command: formCommand.trim(),
        group: formGroup.trim() || undefined,
        sort: maxSort + 1,
      }])
    }
    setFormOpen(false)
  }

  const move = (id: string, dir: -1 | 1) => {
    setCommands(prev => {
      const s = [...prev].sort((a, b) => a.sort - b.sort)
      const idx = s.findIndex(c => c.id === id)
      const target = idx + dir
      if (idx < 0 || target < 0 || target >= s.length) return prev
      const [item] = s.splice(idx, 1)
      s.splice(target, 0, item)
      return s.map((c, i) => ({ ...c, sort: i }))
    })
  }

  const doDelete = (id: string) => {
    setCommands(prev => prev.filter(c => c.id !== id))
    setConfirmDeleteId(null)
  }

  return (
    <>
      <div className="quick-cmds">
        {sorted.length === 0 ? (
          <button type="button" className="quick-cmds-empty" onClick={() => setShowManage(true)}>
            {t('quickCommands.empty')}
          </button>
        ) : (
          <div className="quick-cmds-scroll">
            {sorted.map(c => (
              <button
                key={c.id}
                type="button"
                className="quick-cmd-chip"
                title={c.command}
                onClick={() => handleChipClick(c)}
              >
                {c.name}
              </button>
            ))}
          </div>
        )}
        <button type="button" className="quick-cmds-btn" onClick={() => setShowManage(true)} title={t('quickCommands.manage')}>
          + {t('quickCommands.manage')}
        </button>
      </div>

      {showManage && (
        <div className="modal-overlay" onClick={() => { if (!formOpen) setShowManage(false) }}>
          <div className="modal-content quick-cmds-modal" onClick={e => e.stopPropagation()}>
            <h3>{t('quickCommands.manageTitle')}</h3>

            {formOpen ? (
              <>
                <div className="form-group">
                  <label>{t('quickCommands.name')}</label>
                  <input
                    className="form-input"
                    value={formName}
                    onChange={e => setFormName(e.target.value)}
                    placeholder={t('quickCommands.namePlaceholder')}
                    autoFocus
                  />
                </div>
                <div className="form-group">
                  <label>{t('quickCommands.command')}</label>
                  <input
                    className="form-input"
                    value={formCommand}
                    onChange={e => setFormCommand(e.target.value)}
                    placeholder={t('quickCommands.commandPlaceholder')}
                    spellCheck={false}
                    onKeyDown={e => { if (e.key === 'Enter') saveForm() }}
                  />
                </div>
                <div className="form-group">
                  <label>{t('quickCommands.group')}</label>
                  <input
                    className="form-input"
                    value={formGroup}
                    onChange={e => setFormGroup(e.target.value)}
                    placeholder={t('quickCommands.groupPlaceholder')}
                  />
                </div>
                <div className="modal-actions">
                  <button type="button" className="quick-cmds-btn" onClick={() => setFormOpen(false)}>{t('common.cancel')}</button>
                  <button type="button" className="quick-cmds-btn primary" onClick={saveForm}>{t('common.save')}</button>
                </div>
              </>
            ) : (
              <>
                <div className="quick-cmds-setting">
                  <div className="quick-cmds-setting-label">
                    {t('quickCommands.autoEnter')}
                    <div className="quick-cmds-setting-hint">{t('quickCommands.autoEnterHint')}</div>
                  </div>
                  <button
                    type="button"
                    className={`quick-cmds-toggle ${autoEnter ? 'on' : 'off'}`}
                    role="switch"
                    aria-checked={autoEnter}
                    onClick={() => setAutoEnter(v => !v)}
                    title={t('quickCommands.autoEnter')}
                  >
                    <span className="toggle-track"><span className="toggle-thumb" /></span>
                  </button>
                </div>
                <div className="quick-cmds-list">
                  {sorted.length === 0 && <div className="quick-cmds-list-empty">{t('quickCommands.emptyList')}</div>}
                  {sorted.map(c => (
                    <div key={c.id} className="quick-cmds-item">
                      <div className="quick-cmds-item-info">
                        <div className="quick-cmds-item-name">
                          {c.name}
                          {c.group && <span className="quick-cmds-item-group">{c.group}</span>}
                        </div>
                        <div className="quick-cmds-item-cmd">{c.command}</div>
                      </div>
                      {confirmDeleteId === c.id ? (
                        <div className="quick-cmds-item-confirm">
                          <span>{t('quickCommands.deleteConfirm')}</span>
                          <button type="button" className="quick-cmds-btn danger" onClick={() => doDelete(c.id)}>{t('common.confirm')}</button>
                          <button type="button" className="quick-cmds-btn" onClick={() => setConfirmDeleteId(null)}>{t('common.cancel')}</button>
                        </div>
                      ) : (
                        <div className="quick-cmds-item-actions">
                          <button type="button" className="quick-cmds-icon-btn" title={t('quickCommands.moveUp')} onClick={() => move(c.id, -1)}>↑</button>
                          <button type="button" className="quick-cmds-icon-btn" title={t('quickCommands.moveDown')} onClick={() => move(c.id, 1)}>↓</button>
                          <button type="button" className="quick-cmds-btn" onClick={() => openEdit(c)}>{t('common.edit')}</button>
                          <button type="button" className="quick-cmds-btn danger" onClick={() => setConfirmDeleteId(c.id)}>{t('common.delete')}</button>
                        </div>
                      )}
                    </div>
                  ))}
                </div>
                <div className="modal-actions">
                  <button type="button" className="quick-cmds-btn" onClick={() => setShowManage(false)}>{t('common.close')}</button>
                  <button type="button" className="quick-cmds-btn primary" onClick={openAdd} disabled={commands.length >= MAX_COMMANDS}>{t('quickCommands.add')}</button>
                </div>
              </>
            )}
          </div>
        </div>
      )}
    </>
  )
}
