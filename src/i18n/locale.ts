// Map a system locale (navigator.language) to an app-supported language code.
// Returns null when the system language is English or unsupported (keep 'en' default).

// App-supported languages other than en/zh-CN/zh-TW (which are handled explicitly)
const SUPPORTED: readonly string[] = ['ja', 'fr', 'de', 'ru', 'ar', 'pt', 'ko']

export function systemToAppLocale(systemLang: string): string | null {
  const lower = systemLang.toLowerCase()
  if (lower.startsWith('zh')) {
    // Traditional: TW/HK/MO regions or explicit Hant script
    if (lower.includes('tw') || lower.includes('hk') || lower.includes('mo') || lower.includes('hant')) {
      return 'zh-TW'
    }
    return 'zh-CN'
  }
  for (const code of SUPPORTED) {
    if (lower.startsWith(code)) return code
  }
  return null
}
