import { useState, useEffect, useRef, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { useTranslation } from 'react-i18next'
import Dashboard from './panels/Dashboard'
// ponytail: InstallLnmp removed
// import InstallLnmp from './panels/InstallLnmp'
import NginxPanel from './panels/NginxPanel'
// ponytail: PhpPanel not yet wired up
// import PhpPanel from './panels/PhpPanel'
import SitesPanel from './panels/SitesPanel'
import SslPanel from './panels/SslPanel'
import MonitorPanel from './panels/MonitorPanel'
import FirewallPanel from './panels/FirewallPanel'
import SoftwareRepo from './panels/SoftwareRepo'
import ServerSettingsPanel from './panels/ServerSettingsPanel'
import SiteLogsPanel from './panels/SiteLogsPanel'
import BbrPanel from './panels/BbrPanel'
import DatabasePanel from './panels/DatabasePanel'
import RedisPanel from './panels/RedisPanel'
import DockerPanel from './panels/DockerPanel'
import Terminal from './Terminal'
import type { TerminalHandle } from './Terminal'
import FileBrowser, { type FileBrowserHandle } from './FileBrowser'

type PanelSection = 'dashboard' | 'terminal' | 'files' | 'software' | 'nginx' | 'php' | 'sites' | 'logs' | 'ssl' | 'monitor' | 'firewall' | 'bbr' | 'docker' | 'database' | 'redis' | 'settings'

interface AppSettings {
  auto_reconnect: boolean
  reconnect_interval: number
  max_reconnect_attempts: number
  cache_ttl_hours: number
  cache_max_files: number
  cache_enabled: boolean
}

interface ServerPanelProps {
  sessionId: string | null
  connHost?: string
  jumpToPath?: string | null
  setJumpToPath?: (path: string | null) => void
  termRef?: React.RefObject<TerminalHandle | null>
  onStartUpload?: (files: { file: File; fileName: string; remotePath: string }[]) => void
  onUploadComplete?: React.MutableRefObject<(() => void) | null>
  appSettings?: AppSettings
  onToggleAutoReconnect?: () => void
  onUpdateSettings?: (settings: Partial<AppSettings>) => Promise<void>
  onReconnect?: () => void
}

const NAV_ITEMS: { key: PanelSection; labelKey: string; icon: string }[] = [
  { key: 'dashboard', labelKey: 'nav.dashboard', icon: '📊' },
  { key: 'terminal', labelKey: 'nav.terminal', icon: '💻' },
  { key: 'files', labelKey: 'nav.files', icon: '📂' },
  // { key: 'install', labelKey: 'nav.installLnmp', icon: '📦' },
  { key: 'software', labelKey: 'nav.software', icon: '🧩' },
  // { key: 'nginx', labelKey: 'Nginx', icon: '' },
  { key: 'sites', labelKey: 'nav.sites', icon: '🌐' },
  { key: 'database', labelKey: 'nav.database', icon: '🗄' },
  { key: 'redis', labelKey: 'nav.redis', icon: '⚡' },
  // { key: 'php', labelKey: 'PHP', icon: '' },
  { key: 'logs', labelKey: 'nav.logs', icon: '📋' },
  { key: 'ssl', labelKey: 'nav.ssl', icon: '🔒' },
  { key: 'monitor', labelKey: 'nav.monitor', icon: '📈' },
  { key: 'firewall', labelKey: 'nav.firewall', icon: '🧱' },
  { key: 'bbr', labelKey: 'nav.bbr', icon: '🚀' },
    { key: 'docker', labelKey: 'nav.docker', icon: '' },
  { key: 'settings', labelKey: 'nav.settings', icon: '⚙' },
]

export default function ServerPanel({ sessionId, connHost, jumpToPath, setJumpToPath, termRef, onStartUpload, onUploadComplete, appSettings, onToggleAutoReconnect, onUpdateSettings, onReconnect }: ServerPanelProps) {
  const { t } = useTranslation()
  const [activeSection, setActiveSectionRaw] = useState<PanelSection>('dashboard')
  const cdHereRef = useRef<string | null>(null)
  const fileBrowserRef = useRef<FileBrowserHandle | null>(null)

  // Load last panel from SQLite
  useEffect(() => {
    invoke<string>('ui_state_get', { key: 'lastPanel' })
      .then(v => { if (v && NAV_ITEMS.some(s => s.key === v)) setActiveSectionRaw(v as PanelSection) })
      .catch(() => {})
  }, [])

  const setActiveSection = (key: PanelSection) => {
    setActiveSectionRaw(key)
    invoke('ui_state_set', { key: 'lastPanel', value: key }).catch(() => {})
  }

  const handleNavigate = (section: string) => {
    setActiveSection(section as PanelSection)
  }

  // ponytail: removed auto-switch to terminal on connection - let user choose where to go

  // Clear jumpToPath after FileBrowser consumes it
  useEffect(() => {
    if (jumpToPath && activeSection === 'files') {
      const timer = setTimeout(() => setJumpToPath?.(null), 100)
      return () => clearTimeout(timer)
    }
  }, [jumpToPath, activeSection]) // eslint-disable-line

  // Handle cd-here from FileBrowser
  useEffect(() => {
    if (activeSection === 'terminal' && cdHereRef.current) {
      const path = cdHereRef.current
      cdHereRef.current = null
      setTimeout(() => termRef?.current?.sendCommand(`cd '${path}'`), 200)
    }
  }, [activeSection]) // eslint-disable-line

  const handleInternalOpenFolder = (path: string) => {
    setJumpToPath?.(path)
    setActiveSection('files')
  }

  const handleCdHere = (path: string) => {
    cdHereRef.current = path
    setActiveSection('terminal')
  }

  // Handle upload complete - refresh current directory
  const handleUploadComplete = useCallback(() => {
    if (fileBrowserRef.current && activeSection === 'files') {
      fileBrowserRef.current.refreshCurrentDirectory()
    }
  }, [activeSection])

  useEffect(() => {
    if (onUploadComplete) onUploadComplete.current = handleUploadComplete
  }, [onUploadComplete, handleUploadComplete])

  const renderContent = () => {
    switch (activeSection) {
      case 'dashboard':
        return <Dashboard sessionId={sessionId} onNavigate={handleNavigate} />
      // case 'install':
      //   return <InstallLnmp sessionId={sessionId} onInstallationComplete={onReconnect} />
      case 'nginx':
        return <NginxPanel sessionId={sessionId} />
      // case 'php':
      //   return <PhpPanel sessionId={sessionId} />
      case 'logs':
        return <SiteLogsPanel sessionId={sessionId} />
      case 'ssl':
        return <SslPanel sessionId={sessionId} />
      case 'monitor':
        return <MonitorPanel sessionId={sessionId} />
      case 'firewall':
        return <FirewallPanel sessionId={sessionId} />
      case 'software':
        return <SoftwareRepo sessionId={sessionId} onDisconnect={onReconnect} />
      case 'bbr':
        return <BbrPanel sessionId={sessionId} />
      case 'database':
        return <DatabasePanel sessionId={sessionId} onNavigateToSoftware={() => setActiveSection('software')} />
      case 'redis':
        return <RedisPanel sessionId={sessionId} onNavigateToSoftware={() => setActiveSection('software')} />
      case 'docker':
        return <DockerPanel sessionId={sessionId} />
      case 'settings':
        return <ServerSettingsPanel sessionId={sessionId} onNavigate={handleNavigate} appSettings={appSettings} onToggleAutoReconnect={onToggleAutoReconnect} onUpdateSettings={onUpdateSettings} />
      default:
        return null
    }
  }

  return (
    <div className="server-panel">
      <nav className="sp-nav">
        {NAV_ITEMS.map(item => (
          <button
            key={item.key}
            className={`sp-nav-item ${activeSection === item.key ? 'active' : ''}`}
            onClick={() => setActiveSection(item.key)}
            disabled={!sessionId && item.key !== 'dashboard'}
          >
            <span className="sp-nav-icon">{item.icon}</span>
            <span className="sp-nav-label">{t(item.labelKey)}</span>
          </button>
        ))}
      </nav>
      <div className="sp-content">
        {/* Terminal always mounted to preserve SSH session */}
        <div style={{ display: activeSection === 'terminal' ? 'block' : 'none', height: '100%' }}>
          <Terminal ref={termRef} sessionId={sessionId} isActive={activeSection === 'terminal'} />
        </div>
        {/* Files always mounted to preserve state and avoid reload flash */}
        <div key={sessionId} style={{ display: activeSection === 'files' ? 'block' : 'none', height: '100%' }}>
          <FileBrowser sessionId={sessionId} connHost={connHost} jumpToPath={jumpToPath} ref={fileBrowserRef} onTerminalCommand={termRef?.current ? (cmd: string) => termRef.current?.sendCommand(cmd) : undefined} onCdHere={handleCdHere} onStartUpload={onStartUpload} />
        </div>
        {/* Sites always mounted to preserve list state */}
        <div style={{ display: activeSection === 'sites' ? 'block' : 'none', height: '100%' }}>
          <SitesPanel sessionId={sessionId} onOpenFolder={handleInternalOpenFolder} />
        </div>
        {activeSection !== 'terminal' && activeSection !== 'files' && activeSection !== 'sites' && renderContent()}
      </div>
    </div>
  )
}
