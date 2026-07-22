import { useState, useEffect } from 'react'
import { getVersion } from '@tauri-apps/api/app'
import { check } from '@tauri-apps/plugin-updater'
import { useTranslation } from 'react-i18next'

const GITHUB_URL = 'https://raw.githubusercontent.com/gna1280072/LeePanel/gh-pages/update.json'

interface Step { text: string; status: 'pending' | 'ok' | 'fail' }

// ponytail: probe endpoint with individual timeout, returns null on failure
async function probeEndpoint(url: string, timeoutMs: number): Promise<Record<string, unknown> | null> {
  try {
    const res = await Promise.race([
      fetch(url),
      new Promise<never>((_, reject) => setTimeout(() => reject(new Error('timeout')), timeoutMs)),
    ])
    if (!res.ok) return null
    return await res.json()
  } catch {
    return null
  }
}

export default function UpdatePanel() {
  const { t } = useTranslation()
  const [appVersion, setAppVersion] = useState('')
  const [checking, setChecking] = useState(false)
  const [message, setMessage] = useState('')
  const [progress, setProgress] = useState<{ pct: number; downloaded: string; total: string } | null>(null)
  const [steps, setSteps] = useState<Step[]>([])

  useEffect(() => {
    getVersion().then(setAppVersion).catch(() => {})
  }, [])

  const handleCheckUpdate = async () => {
    setChecking(true)
    setMessage('')
    setProgress(null)
    setSteps([])
    const addStep = (text: string, status: Step['status'] = 'pending') => setSteps(prev => [...prev, { text, status }])
    const updateLastStep = (status: Step['status']) => setSteps(prev => { const c = [...prev]; c[c.length - 1] = { ...c[c.length - 1], status }; return c })
    try {
      // Step 1: probe GitHub endpoint
      addStep(GITHUB_URL)
      const probeData = await probeEndpoint(GITHUB_URL, 15000)
      if (probeData) updateLastStep('ok')
      else updateLastStep('fail')

      if (!probeData) {
        setMessage(t('settings.updateTimedOut'))
        return
      }

      // Step 2: check version via Tauri updater
      addStep(t('settings.fetchingVersion'))
      const update = await Promise.race([
        check(),
        new Promise<never>((_, reject) => setTimeout(() => reject(new Error('Timeout')), 30000)),
      ])
      updateLastStep('ok')
      if (update?.available) {
        // Extract platform download URL from probe data (already fetched)
        const platformKey = navigator.userAgent.includes('Windows') ? 'windows-x86_64'
          : navigator.userAgent.includes('Mac') ? (navigator.userAgent.includes('ARM') ? 'darwin-aarch64' : 'darwin-x86_64')
          : 'linux-x86_64'
        const dlUrl = ((probeData.platforms as Record<string, { url: string }>)?.[platformKey]?.url) || ''
        const urlLine = dlUrl ? '\n' + dlUrl : ''
        setMessage(t('settings.newVersionFound', { version: update.version }) + urlLine)
        let downloaded = 0
        let total = 0
        const fmt = (b: number) => b >= 1048576 ? (b / 1048576).toFixed(1) + ' MB' : (b / 1024).toFixed(0) + ' KB'
        await update.downloadAndInstall((event) => {
          if (event.event === 'Started' && event.data.contentLength) {
            total = event.data.contentLength
          } else if (event.event === 'Progress') {
            downloaded += event.data.chunkLength
            const pct = total > 0 ? Math.round(downloaded / total * 100) : 0
            setProgress({ pct, downloaded: fmt(downloaded), total: total ? fmt(total) : '' })
            setMessage(t('settings.downloadProgress', { version: update.version, pct, downloaded: fmt(downloaded), total: total ? ' / ' + fmt(total) : '' }) + urlLine)
          }
        })
        setProgress(null)
        const { ask } = await import('@tauri-apps/plugin-dialog')
        const restart = await ask(t('settings.updateReady', { version: update.version }), { title: t('settings.updateReadyTitle'), kind: 'info' })
        if (restart) {
          const { relaunch } = await import('@tauri-apps/plugin-process')
          await relaunch()
        } else {
          setMessage(t('settings.updateInstalled', { version: update.version }))
        }
      } else {
        setMessage(t('settings.latestVersion'))
      }
    } catch (e) {
      const msg = String(e)
      setMessage(msg.includes('Timeout') ? t('settings.updateTimedOut') : t('settings.updateCheckFailed', { error: msg.slice(0, 100) }))
    } finally {
      setChecking(false)
    }
  }

  return (
    <div className="sp-page" style={{ padding: '24px 32px', maxWidth: 600 }}>
      <h2 style={{ margin: '0 0 20px', fontSize: 18, fontWeight: 600, color: '#e6edf3' }}>{t('settings.softwareUpdate')}</h2>

      <div className="settings-card">
        <div className="settings-card-body" style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
          {/* Version info */}
          <div className="settings-row">
            <span className="settings-label">{t('settings.currentVersion')}</span>
            <span className="settings-value" style={{ fontFamily: "'JetBrains Mono', monospace", fontSize: 13, color: '#8b949e' }}>{appVersion || '—'}</span>
          </div>

          {/* Check button */}
          <button
            className="svc-cfg-btn primary"
            style={{ width: '100%' }}
            onClick={handleCheckUpdate}
            disabled={checking}
          >
            {checking ? t('settings.checking') : t('settings.checkUpdates')}
          </button>

          {/* Step log */}
          {steps.length > 0 && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 4, padding: '8px 12px', background: '#161b22', borderRadius: 6, fontSize: 12, fontFamily: "'JetBrains Mono', monospace" }}>
              {steps.map((s, i) => (
                <div key={i} style={{ color: s.status === 'ok' ? '#3fb950' : s.status === 'fail' ? '#f85149' : '#8b949e', wordBreak: 'break-all' }}>
                  {s.status === 'ok' ? '✓' : s.status === 'fail' ? '✗' : '⏳'} {s.text}
                </div>
              ))}
            </div>
          )}

          {/* Progress bar */}
          {progress && (
            <div style={{ width: '100%', height: 6, background: '#21262d', borderRadius: 3, overflow: 'hidden' }}>
              <div style={{ width: `${progress.pct}%`, height: '100%', background: '#2ea043', borderRadius: 3, transition: 'width 0.3s' }} />
            </div>
          )}

          {/* Status message */}
          {message && (
            <div style={{ padding: '8px 12px', background: message.includes('latest') || message.includes('最新') ? '#1f6feb22' : '#2ea04322', borderRadius: 6, fontSize: 13, color: message.includes('latest') || message.includes('最新') ? '#58a6ff' : '#3fb950', whiteSpace: 'pre-line', wordBreak: 'break-all' }}>
              {message}
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
