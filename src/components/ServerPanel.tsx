import { useState, useEffect, useRef, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { useTranslation } from 'react-i18next'
import { open } from '@tauri-apps/plugin-shell'
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
import UpdatePanel from './panels/UpdatePanel'
import SiteLogsPanel from './panels/SiteLogsPanel'
import BbrPanel from './panels/BbrPanel'
import DatabasePanel from './panels/DatabasePanel'
import RedisPanel from './panels/RedisPanel'
import DockerPanel from './panels/DockerPanel'
import Terminal from './Terminal'
import type { TerminalHandle } from './Terminal'
import FileBrowser, { type FileBrowserHandle } from './FileBrowser'

type PanelSection = 'dashboard' | 'terminal' | 'files' | 'software' | 'nginx' | 'php' | 'sites' | 'logs' | 'ssl' | 'monitor' | 'firewall' | 'bbr' | 'docker' | 'database' | 'redis' | 'update' | 'settings' | 'discussions'

interface AppSettings {
  auto_reconnect: boolean
  reconnect_interval: number
  max_reconnect_attempts: number
  close_tab_on_disconnect: boolean
  cache_ttl_hours: number
  cache_max_files: number
  cache_enabled: boolean
  command_timeout_minutes: number
  upload_workers: number
}

interface ServerPanelProps {
  sessionId: string | null
  connHost?: string
  connUsername?: string
  initialSection?: string
  jumpToPath?: string | null
  setJumpToPath?: (path: string | null) => void
  termRef?: React.RefObject<TerminalHandle | null>
  onStartUpload?: (files: { file: File; fileName: string; remotePath: string }[]) => void
  onUploadComplete?: React.MutableRefObject<(() => void) | null>
  appSettings?: AppSettings
  onToggleAutoReconnect?: () => void
  onUpdateSettings?: (settings: Partial<AppSettings>) => Promise<void>
  onShowToast?: (msg: string) => void
}

const NAV_ITEMS: { key: PanelSection; labelKey: string; icon: string }[] = [
  { key: 'dashboard', labelKey: 'nav.dashboard', icon: '📊' },
  { key: 'terminal', labelKey: 'nav.terminal', icon: '💻' },
  { key: 'files', labelKey: 'nav.files', icon: '📂' },
  // { key: 'install', labelKey: 'nav.installLnmp', icon: '📦' },
  { key: 'software', labelKey: 'nav.software', icon: '🧩' },
  // { key: 'nginx', labelKey: 'Nginx', icon: '' },
  { key: 'sites', labelKey: 'nav.sites', icon: '🌐' },
  { key: 'ssl', labelKey: 'nav.ssl', icon: '🔒' },
  { key: 'docker', labelKey: 'nav.docker', icon: '🐳' },
  { key: 'database', labelKey: 'nav.database', icon: '🗄' },
  { key: 'redis', labelKey: 'nav.redis', icon: '⚡' },
  // { key: 'php', labelKey: 'PHP', icon: '' },
  { key: 'logs', labelKey: 'nav.logs', icon: '📋' },
  { key: 'monitor', labelKey: 'nav.monitor', icon: '📈' },
  { key: 'firewall', labelKey: 'nav.firewall', icon: '🧱' },
  { key: 'bbr', labelKey: 'nav.bbr', icon: '🚀' },
  { key: 'update', labelKey: 'nav.update', icon: '🔄' },
  { key: 'settings', labelKey: 'nav.settings', icon: '⚙' },
  { key: 'discussions', labelKey: 'nav.discussions', icon: '💬' },
]

export default function ServerPanel({ sessionId, connHost, connUsername, initialSection = 'dashboard', jumpToPath, setJumpToPath, termRef, onStartUpload, onUploadComplete, appSettings, onToggleAutoReconnect, onUpdateSettings, onShowToast }: ServerPanelProps) {
  const { t } = useTranslation()
  const [activeSection, setActiveSectionRaw] = useState<PanelSection>((initialSection && NAV_ITEMS.some(s => s.key === initialSection) ? initialSection : 'dashboard') as PanelSection)
  const cdHereRef = useRef<string | null>(null)
  const fileBrowserRef = useRef<FileBrowserHandle | null>(null)

  // ponytail: per-server panel memory — key = lastPanel_${user}@${host}
  const panelKey = connHost && connUsername ? `lastPanel_${connUsername}@${connHost}` : ''

  // Sync activeSection when initialSection changes (redundant with key remount, kept as safety net)
  useEffect(() => {
    if (initialSection && NAV_ITEMS.some(s => s.key === initialSection)) {
      setActiveSectionRaw(initialSection as PanelSection)
    }
  }, [initialSection])

  const setActiveSection = (key: PanelSection) => {
    setActiveSectionRaw(key)
    if (panelKey) invoke('ui_state_set', { key: panelKey, value: key }).catch(() => {})
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

  // ponytail: auto-focus FileBrowser on tab switch so keyboard shortcuts work immediately
  useEffect(() => {
    if (activeSection === 'files' && fileBrowserRef.current) {
      fileBrowserRef.current.focus()
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
      // case 'software': removed - always mounted below
      case 'bbr':
        return <BbrPanel sessionId={sessionId} />
      case 'database':
        return <DatabasePanel sessionId={sessionId} onNavigateToSoftware={() => setActiveSection('software')} />
      case 'redis':
        return <RedisPanel sessionId={sessionId} onNavigateToSoftware={() => setActiveSection('software')} />
      case 'docker':
        return <DockerPanel sessionId={sessionId} onNavigateToSoftware={() => setActiveSection('software')} />
      case 'settings':
        return <ServerSettingsPanel sessionId={sessionId} appSettings={appSettings} onToggleAutoReconnect={onToggleAutoReconnect} onUpdateSettings={onUpdateSettings} />
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
            onClick={() => {
              if (item.key === 'discussions') { open('https://github.com/gna1280072/LeePanel/discussions'); return }
              // ponytail: no session → toast hint instead of disabling nav items
              if (!sessionId) { onShowToast?.(`⚠ ${t('common.connectFirst')}`); return }
              setActiveSection(item.key)
            }}
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
        <div style={{ display: activeSection === 'files' ? 'block' : 'none', height: '100%' }}>
          <FileBrowser sessionId={sessionId} connHost={connHost} jumpToPath={jumpToPath} ref={fileBrowserRef} onTerminalCommand={termRef?.current ? (cmd: string) => termRef.current?.sendCommand(cmd) : undefined} onCdHere={handleCdHere} onStartUpload={onStartUpload} onNavigateToSoftware={() => setActiveSection('software')} />
        </div>
        {/* Sites always mounted to preserve list state */}
        <div style={{ display: activeSection === 'sites' ? 'block' : 'none', height: '100%' }}>
          <SitesPanel sessionId={sessionId} onOpenFolder={handleInternalOpenFolder} />
        </div>
        {/* Software always mounted to preserve install progress state */}
        <div style={{ display: activeSection === 'software' ? 'block' : 'none', height: '100%' }}>
          <SoftwareRepo sessionId={sessionId} />
        </div>
        {/* Update always mounted to preserve update state */}
        <div style={{ display: activeSection === 'update' ? 'block' : 'none', height: '100%' }}>
          <UpdatePanel />
        </div>
        {activeSection !== 'terminal' && activeSection !== 'files' && activeSection !== 'sites' && activeSection !== 'software' && activeSection !== 'update' && renderContent()}
      </div>
    </div>
  )
}
