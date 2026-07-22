import { useState, useEffect } from 'react'
import { getVersion } from '@tauri-apps/api/app'
import { invoke } from '@tauri-apps/api/core'
import { check } from '@tauri-apps/plugin-updater'
import { open } from '@tauri-apps/plugin-shell'
import { useTranslation } from 'react-i18next'

interface Step { text: string; status: 'pending' | 'ok' | 'fail' }

export default function UpdatePanel() {
  const { t } = useTranslation()
  const [appVersion, setAppVersion] = useState('')
  const [checking, setChecking] = useState(false)
  const [message, setMessage] = useState('')
  const [progress, setProgress] = useState<{ pct: number; downloaded: string; total: string } | null>(null)
  const [steps, setSteps] = useState<Step[]>([])
  const [timedOut, setTimedOut] = useState(false)

  useEffect(() => {
    getVersion().then(setAppVersion).catch(() => {})
  }, [])

  const handleCheckUpdate = async () => {
    setChecking(true)
    setMessage('')
    setProgress(null)
    setSteps([])
    setTimedOut(false)
    const addStep = (text: string, status: Step['status'] = 'pending') => setSteps(prev => [...prev, { text, status }])
    const updateLastStep = (status: Step['status']) => setSteps(prev => { const c = [...prev]; c[c.length - 1] = { ...c[c.length - 1], status }; return c })
    try {
      // Attempt 1: check with proxy (10s timeout)
      addStep(t('settings.fetchingVersion'))
      let update = await Promise.race([
        check(),
        new Promise<never>((_, reject) => setTimeout(() => reject(new Error('Timeout')), 10000)),
      ]).catch(async () => {
        // Attempt 2: clear proxy and retry (10s timeout)
        updateLastStep('fail')
        addStep(t('settings.clearingProxy'))
        await invoke('clear_proxy_env')
        updateLastStep('ok')
        return await Promise.race([
          check(),
          new Promise<never>((_, reject) => setTimeout(() => reject(new Error('Timeout')), 10000)),
        ])
      })
      updateLastStep('ok')
      if (update?.available) {
        setMessage(t('settings.newVersionFound', { version: update.version }))
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
            setMessage(t('settings.downloadProgress', { version: update.version, pct, downloaded: fmt(downloaded), total: total ? ' / ' + fmt(total) : '' }))
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
      setMessage(msg.includes('Timeout') ? t('settings.updateTimedOut') : t('settings.updateCheckFailed', { error: msg }))
      if (msg.includes('Timeout')) setTimedOut(true)
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
              {timedOut && (() => {
                const text = t('settings.updateManualDownload')
                const url = 'https://www.LeePanel.com'
                const idx = text.indexOf(url)
                return (
                  <div style={{ marginTop: 4 }}>
                    {idx >= 0 ? <>{text.slice(0, idx)}<a href="#" onClick={(e) => { e.preventDefault(); open(url) }} style={{ color: '#58a6ff' }}>{url}</a>{text.slice(idx + url.length)}</> : text}
                  </div>
                )
              })()}
            </div>
          )}
        </div>
      </div>

      <div style={{ marginTop: 16, fontSize: 13, color: '#8b949e', lineHeight: 1.8 }}>
        <div style={{ fontWeight: 600, color: '#e6edf3', marginBottom: 4 }}>{t('settings.updateFailedHint')}</div>
        <div>1、{t('settings.updateFailedProxyHint')}</div>
        <div>2、{t('settings.updateFailedManualHint')} <a href="#" onClick={(e) => { e.preventDefault(); open('https://www.LeePanel.com') }} style={{ color: '#58a6ff' }}>https://www.LeePanel.com</a></div>
      </div>
    </div>
  )
}
