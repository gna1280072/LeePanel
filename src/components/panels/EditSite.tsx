import { useState, useEffect, useRef } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { useTranslation } from 'react-i18next'

interface SiteInfo {
  domain: string
  domains: string
  root: string
  config_path: string
  ssl: boolean
  ssl_cert_path: string | null
  ssl_key_path: string | null
  php_version: string
  running_dir: string
  open_basedir: boolean
  enabled: boolean
  index_files: string
  proxy_target: string
  hotlink_enabled: boolean
  hotlink_extensions: string
  hotlink_allowed_domains: string
  hotlink_response: string
  hotlink_allow_empty_referer: boolean
  created_at: number
}

const REWRITE_TEMPLATES: Record<string, string> = {
  WordPress: `location / {
    try_files $uri $uri/ /index.php?$args;
}

location ~ \\.php$ {
    include snippets/fastcgi-php.conf;
    fastcgi_pass unix:/run/php/php-fpm.sock;
    fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;
    include fastcgi_params;
}`,
  Laravel: `location / {
    try_files $uri $uri/ /index.php?$query_string;
}`,
  ThinkPHP: `location / {
    if (!-e $request_filename) {
        rewrite ^(.*)$ /index.php?s=/$1 last;
    }
}`,
  'ThinkPHP 6': `location / {
    try_files $uri $uri/ /index.php$is_args$args;
}`,
  CodeIgniter: `location / {
    try_files $uri $uri/ /index.php?$query_string;
}`,
  Symfony: `location / {
    try_files $uri /index.php$is_args$args;
}`,
  Yii2: `location / {
    try_files $uri $uri/ /index.php$is_args$args;
}`,
  Typecho: `location / {
    if (!-e $request_filename) {
        rewrite ^(.*)$ /index.php$1 last;
    }
}`,
  Discuz: `location / {
    rewrite ^([^\\.]*)/([^/]+)-([0-9]+)-([0-9]+)-([0-9]+)-([0-9]+)-([0-9]+)\\.html$ $1/$2.php?mod=forumdisplay&fid=$3&page=$4 last;
    rewrite ^([^\\.]*)/([^/]+)-([0-9]+)-([0-9]+)\\.html$ $1/$2.php?mod=viewthread&tid=$3&extra=page%3D$4 last;
    rewrite ^([^\\.]*)/([^/]+)\\.html$ $1/$2.php?rewrite=$2 last;
}`,
  'Next.js (proxy)': `location / {
    proxy_pass http://127.0.0.1:3000;
    proxy_http_version 1.1;
    proxy_set_header Upgrade $http_upgrade;
    proxy_set_header Connection 'upgrade';
    proxy_set_header Host $host;
    proxy_cache_bypass $http_upgrade;
}`,
  'Vue/React SPA': `location / {
    try_files $uri $uri/ /index.html;
}`,
}

export default function EditSite({
  sessionId,
  site,
  onBack,
  onSaved,
  onError,
}: {
  sessionId: string
  site: SiteInfo
  onBack: () => void
  onSaved: () => void
  onError: (msg: string) => void
}) {
  // Convert space-separated domains to newline-separated for textarea display
  const { t } = useTranslation()
  const [domains, setDomains] = useState(site.domains.split(' ').filter(Boolean).join('\n'))
  const [root, setRoot] = useState(site.root)
  const [phpVersion, setPhpVersion] = useState(site.php_version || '')
  const [phpVersions, setPhpVersions] = useState<string[]>([])
  const [indexFiles, setIndexFiles] = useState('index.php\nindex.html\nindex.htm')
  const [rewriteRules, setRewriteRules] = useState('')
  const [saving, setSaving] = useState(false)
  const [runningDir, setRunningDir] = useState(site.running_dir || '/')
  const [openBasedir, setOpenBasedir] = useState(site.open_basedir ?? true)
  const [subdirs, setSubdirs] = useState<string[]>([])
  const autoDetectRef = useRef(true)

  // Resizable config editor popup
  const [configEditorOpen, setConfigEditorOpen] = useState(false)
  const [configEditorContent, setConfigEditorContent] = useState('')
  const [configEditorLoading, setConfigEditorLoading] = useState(false)
  const [configEditorSaving, setConfigEditorSaving] = useState(false)
  const [configEditorMaximized, setConfigEditorMaximized] = useState(false)

  // Hotlink tab - initialize from site data
  const [hotlinkEnabled, setHotlinkEnabled] = useState(site.hotlink_enabled || false)
  const [hotlinkExtensions, setHotlinkExtensions] = useState(
    site.hotlink_extensions || 'jpg,jpeg,gif,png,js,css'
  )
  const [hotlinkDomains, setHotlinkDomains] = useState(
    site.hotlink_allowed_domains || site.domain
  )
  const [hotlinkResponse, setHotlinkResponse] = useState(
    site.hotlink_response || '403'
  )
  const [hotlinkAllowEmpty, setHotlinkAllowEmpty] = useState(
    site.hotlink_allow_empty_referer || false
  )

  // Reverse Proxy tab
  const [proxyEnabled, setProxyEnabled] = useState(false)
  const [proxyPath, setProxyPath] = useState('/')
  const [proxyTarget, setProxyTarget] = useState('http://127.0.0.1:3000')
  const [proxyWebsocket, setProxyWebsocket] = useState(false)
  const [proxyPreserveHost, setProxyPreserveHost] = useState(true)

  useEffect(() => {
    invoke<string[]>('server_list_php_versions', { sessionId })
      .then(versions => {
        console.log('PHP versions loaded:', versions)
        setPhpVersions(versions)
      })
      .catch(err => {
        console.error('Failed to load PHP versions:', err)
        onError(`Failed to load PHP versions: ${err}`)
      })
    // Load existing config to populate indexFiles and rewriteRules
    invoke<string>('server_read_remote_file', { sessionId, path: site.config_path })
      .then(text => {
        const indexMatch = text.match(/^\s*index\s+([^;]+);/m)
        if (indexMatch) setIndexFiles(indexMatch[1].trim().split(/\s+/).join('\n'))
        const rewriteMatch = text.match(/# Rewrite rules\n([\s\S]*?)\n\s*location ~ \.php/)
        if (rewriteMatch) setRewriteRules(rewriteMatch[1].replace(/^    /gm, '').trim())
        // Detect existing reverse proxy config
        const proxyMatch = text.match(/# Reverse Proxy Start\s*\n\s*location\s+(\S+)\s*\{\s*\n\s*proxy_pass\s+(\S+);/)
        if (proxyMatch) {
          setProxyEnabled(true)
          setProxyPath(proxyMatch[1])
          setProxyTarget(proxyMatch[2])
          setProxyWebsocket(text.includes('proxy_set_header Upgrade'))
          setProxyPreserveHost(text.includes('proxy_set_header Host $host'))
        }
      })
      .catch(() => {})
  }, [sessionId])

  // Load subdirectories when root changes or dialog opens
  useEffect(() => {
    if (!root.trim()) { setSubdirs([]); return }
    invoke<string[]>('server_list_subdirs', { sessionId, path: root.trim() })
      .then(dirs => {
        setSubdirs(dirs)
        // Auto-detect common framework dirs when running_dir is default "/"
        if (autoDetectRef.current && (site.running_dir === '/' || !site.running_dir)) {
          const common = ['public', 'www', 'web', 'html']
          const match = common.find(d => dirs.includes(d))
          if (match) setRunningDir(`/${match}`)
          autoDetectRef.current = false
        }
      })
      .catch(() => setSubdirs([]))
  }, [sessionId, root])

  const handleSaveAll = async () => {
    if (!domains.trim()) {
      onError('Domain name cannot be empty')
      return
    }
    
    // Convert formats
    const domainsStr = domains.trim().split('\n').map(d => d.trim()).filter(Boolean).join(' ')
    const indexStr = indexFiles.trim().split('\n').map(f => f.trim()).filter(Boolean).join(' ')
    const allowedDomainsStr = hotlinkDomains.trim()
    
    setSaving(true)
    try {
      await invoke<string>('server_update_site_full', {
        sessionId,
        oldDomain: site.domain,
        newDomains: domainsStr,
        newRoot: root.trim(),
        newPhpVersion: phpVersion,
        indexFiles: indexStr,
        rewriteRules,
        configPath: site.config_path,
        runningDir: runningDir.trim() || '/',
        openBasedir,
        hotlinkEnabled,
        hotlinkExtensions: hotlinkExtensions.trim(),
        hotlinkAllowedDomains: allowedDomainsStr,
        hotlinkResponse: hotlinkResponse.trim(),
        hotlinkAllowEmptyReferer: hotlinkAllowEmpty,
        proxyEnabled,
        proxyPath: proxyPath.trim() || '/',
        proxyTarget: proxyTarget.trim(),
        proxyWebsocket,
        proxyPreserveHost,
      })
      onSaved()
    } catch (e) {
      onError(String(e))
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="sites-panel edit-site-page">
      {/* Header */}
      <div className="edit-site-header">
        <button className="back-btn" onClick={onBack}>← {t('common.back')}</button>
        <h2>{t('sites.editSite', { domain: site.domain })}</h2>
      </div>

      {/* Content - Single Page with Cards */}
      <div className="edit-content">
        {/* Card 1: Basic Settings */}
        <div className="sp-card">
          <div className="sp-card-title">{t('sites.basicSettings')}</div>
          
          <div className="edit-field">
            <label>{t('sites.domains')} <span className="edit-hint">({t('sites.domainsHint')})</span></label>
            <textarea className="edit-textarea" rows={3} value={domains} onChange={(e) => setDomains(e.target.value)} />
          </div>

          <div className="edit-field">
            <label>{t('sites.webRoot')}</label>
            <input type="text" className="create-input" value={root} onChange={(e) => setRoot(e.target.value)} />
          </div>

          <div className="edit-field">
            <label>{t('sites.defaultHomepage')} <span className="edit-hint">({t('sites.homepageHint')})</span></label>
            <textarea className="edit-textarea" rows={3} value={indexFiles} onChange={(e) => setIndexFiles(e.target.value)} />
          </div>

          <div className="edit-field">
            <label>{t('sites.phpVersion')}</label>
            <select className="create-select" value={phpVersion} onChange={(e) => setPhpVersion(e.target.value)}>
              <option value="">{t('sites.noneStatic')}</option>
              {phpVersions.map(v => (
                <option key={v} value={v}>PHP {v}</option>
              ))}
            </select>
          </div>

          <div className="edit-field">
            <label>{t('sites.configPath')}</label>
            <div style={{ display: 'flex', gap: 6 }}>
              <input type="text" className="create-input mono" value={site.config_path} disabled style={{ flex: 1 }} />
              <button
                className="svc-cfg-btn"
                style={{ whiteSpace: 'nowrap', fontSize: 12, padding: '4px 10px' }}
                onClick={async () => {
                  setConfigEditorOpen(true)
                  setConfigEditorLoading(true)
                  try {
                    const text = await invoke<string>('server_read_remote_file', {
                      sessionId,
                      path: site.config_path,
                    })
                    setConfigEditorContent(text)
                  } catch (e) {
                    onError(String(e))
                    setConfigEditorOpen(false)
                  } finally {
                    setConfigEditorLoading(false)
                  }
                }}
              >
                {t('sites.editConfig')}
              </button>
            </div>
          </div>
        </div>

        {/* Card 2: Directory Settings */}
        <div className="sp-card">
          <div className="sp-card-title">{t('sites.directorySettings')}</div>
          
          <div className="edit-field">
            <label>{t('sites.runningDirectory')}</label>
            <select className="create-select" value={runningDir} onChange={(e) => setRunningDir(e.target.value)}>
              <option value="/">/</option>
              {subdirs.map(d => (
                <option key={d} value={`/${d}`}>/{d}</option>
              ))}
            </select>
            <div className="edit-hint" style={{ marginTop: 4 }}>{t('sites.runningDirHint')}</div>
          </div>

          <div className="edit-field" style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
            <div>
              <label style={{ marginBottom: 0 }}>{t('sites.antiCrossSite')}</label>
              <div className="edit-hint" style={{ marginTop: 2 }}>{t('sites.antiCrossSiteHint')}</div>
            </div>
            <button
              className={`firewall-toggle ${openBasedir ? 'on' : 'off'}`}
              onClick={() => setOpenBasedir(!openBasedir)}
              type="button"
            >
              <div className="toggle-track"><div className="toggle-thumb" /></div>
              <span className="toggle-label">{openBasedir ? t('common.on') : t('common.off')}</span>
            </button>
          </div>
        </div>

        {/* Card 3: Rewrite Rules */}
        <div className="sp-card">
          <div className="sp-card-title">{t('sites.rewriteRules')}</div>
          
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 8 }}>
            <label style={{ marginBottom: 0 }}>{t('sites.nginxLocationBlocks')} <span className="edit-hint">({t('sites.orRewriteDirectives')})</span></label>
            <select
              className="create-select"
              style={{ width: 'auto', minWidth: 140 }}
              value=""
              onChange={(e) => {
                const tpl = REWRITE_TEMPLATES[e.target.value]
                if (tpl) setRewriteRules(tpl)
              }}
            >
              <option value="">{t('sites.selectTemplate')}</option>
              {Object.keys(REWRITE_TEMPLATES).map(k => (
                <option key={k} value={k}>{k}</option>
              ))}
            </select>
          </div>
          <textarea
            className="edit-textarea mono"
            rows={12}
            placeholder={`# Example:\nlocation /api/ {\n    proxy_pass http://127.0.0.1:3000;\n}\n\nlocation /old-path {\n    rewrite ^/old-path(.*)$ /new-path$1 permanent;\n}`}
            value={rewriteRules}
            onChange={(e) => setRewriteRules(e.target.value)}
          />
        </div>

        {/* Card 4: Hotlink Protection */}
        <div className="sp-card">
          <div className="sp-card-title">{t('sites.hotlinkProtection')}</div>
          
          <div className="edit-field">
            <label>{t('sites.urlSuffixes')} <span className="edit-hint">({t('sites.urlSuffixesHint')})</span></label>
            <input
              type="text"
              className="create-input"
              placeholder="jpg,jpeg,gif,png,js,css"
              value={hotlinkExtensions}
              onChange={(e) => setHotlinkExtensions(e.target.value)}
            />
          </div>

          <div className="edit-field">
            <label>{t('sites.allowedDomains')} <span className="edit-hint">({t('sites.allowedDomainsHint')})</span></label>
            <textarea
              className="edit-textarea"
              rows={3}
              placeholder="example.com&#10;www.example.com"
              value={hotlinkDomains}
              onChange={(e) => setHotlinkDomains(e.target.value)}
            />
          </div>

          <div className="edit-field">
            <label>{t('sites.responseResource')} <span className="edit-hint">({t('sites.responseHint')})</span></label>
            <input
              type="text"
              className="create-input"
              placeholder="404"
              value={hotlinkResponse}
              onChange={(e) => setHotlinkResponse(e.target.value)}
            />
          </div>

          <label className="create-checkbox">
            <input type="checkbox" checked={hotlinkEnabled} onChange={(e) => setHotlinkEnabled(e.target.checked)} />
            <span>{t('sites.enableHotlink')}</span>
          </label>

          <label className="create-checkbox">
            <input type="checkbox" checked={hotlinkAllowEmpty} onChange={(e) => setHotlinkAllowEmpty(e.target.checked)} />
            <span>{t('sites.allowEmptyReferer')}</span>
          </label>
        </div>

        {/* Card 5: Reverse Proxy */}
        <div className="sp-card">
          <div className="sp-card-title">{t('sites.reverseProxy')}</div>
          
          <div className="edit-field" style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
            <div>
              <label style={{ marginBottom: 0 }}>{t('sites.enableReverseProxy')}</label>
              <div className="edit-hint" style={{ marginTop: 2 }}>{t('sites.enableReverseProxyHint')}</div>
            </div>
            <button
              className={`firewall-toggle ${proxyEnabled ? 'on' : 'off'}`}
              onClick={() => setProxyEnabled(!proxyEnabled)}
              type="button"
            >
              <div className="toggle-track"><div className="toggle-thumb" /></div>
              <span className="toggle-label">{proxyEnabled ? t('common.on') : t('common.off')}</span>
            </button>
          </div>

          <div className="edit-field">
            <label>{t('sites.proxyPath')} <span className="edit-hint">({t('sites.proxyPathHint')})</span></label>
            <input type="text" className="create-input" value={proxyPath} onChange={(e) => setProxyPath(e.target.value)} placeholder="/" />
          </div>

          <div className="edit-field">
            <label>{t('sites.targetUrl')} <span className="edit-hint">({t('sites.targetUrlHint')})</span></label>
            <input type="text" className="create-input" value={proxyTarget} onChange={(e) => setProxyTarget(e.target.value)} placeholder="http://127.0.0.1:3000" />
          </div>

          <label className="create-checkbox">
            <input type="checkbox" checked={proxyWebsocket} onChange={(e) => setProxyWebsocket(e.target.checked)} />
            <span>{t('sites.websocketSupport')}</span>
          </label>

          <label className="create-checkbox">
            <input type="checkbox" checked={proxyPreserveHost} onChange={(e) => setProxyPreserveHost(e.target.checked)} />
            <span>{t('sites.preserveHost')}</span>
          </label>
        </div>
      </div>

      {/* Footer actions */}
      <div className="edit-footer">
        <button className="fb-dialog-btn" onClick={onBack} disabled={saving}>
          {t('common.cancel')}
        </button>
        <button className="fb-dialog-btn primary" onClick={handleSaveAll} disabled={saving}>
          {saving ? t('common.saving') : t('common.save')}
        </button>
      </div>

      {/* Resizable config editor overlay */}
      {configEditorOpen && (
        <div className="fb-dialog-overlay" style={{ zIndex: 1100 }}>
          <div
            className={`config-editor-dialog${configEditorMaximized ? ' maximized' : ''}`}
            onClick={(e) => e.stopPropagation()}
          >
            <div className="config-editor-header">
              <span className="config-editor-title">{t('sites.nginxConfig')} — {site.config_path}</span>
              <div className="config-editor-header-btns">
                <button
                  className="config-editor-maximize"
                  onClick={() => setConfigEditorMaximized(!configEditorMaximized)}
                  title={configEditorMaximized ? t('files.restore') : t('files.maximize')}
                >
                  {configEditorMaximized ? '' : '▢'}
                </button>
                <button className="config-editor-close" onClick={() => setConfigEditorOpen(false)}>×</button>
              </div>
            </div>
            {configEditorLoading ? (
              <div className="config-editor-loading">{t('common.loading')}</div>
            ) : (
              <textarea
                className="config-editor-textarea"
                value={configEditorContent}
                onChange={(e) => setConfigEditorContent(e.target.value)}
                spellCheck={false}
              />
            )}
            <div className="config-editor-footer">
              <button className="fb-dialog-btn" onClick={() => setConfigEditorOpen(false)}>{t('common.cancel')}</button>
              <button
                className="fb-dialog-btn primary"
                disabled={configEditorLoading || configEditorSaving}
                onClick={async () => {
                  setConfigEditorSaving(true)
                  try {
                    await invoke<string>('server_save_site_config', {
                      sessionId,
                      configPath: site.config_path,
                      configContent: configEditorContent,
                    })
                    setConfigEditorOpen(false)
                    onSaved()
                  } catch (e) {
                    onError(String(e))
                  } finally {
                    setConfigEditorSaving(false)
                  }
                }}
              >
                {configEditorSaving ? t('common.saving') : t('common.save')}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
