import { describe, it, expect } from 'vitest'
import { readdirSync, readFileSync } from 'fs'
import { join } from 'path'
import { systemToAppLocale } from '../src/i18n/locale'

const I18N_DIR = join(__dirname, '..', 'src', 'i18n')

// Collect all keys from a nested object (dot-separated paths)
function collectKeys(obj: Record<string, unknown>, prefix = ''): string[] {
  const keys: string[] = []
  for (const [k, v] of Object.entries(obj)) {
    const fullKey = prefix ? `${prefix}.${k}` : k
    if (v && typeof v === 'object' && !Array.isArray(v)) {
      keys.push(...collectKeys(v as Record<string, unknown>, fullKey))
    } else {
      keys.push(fullKey)
    }
  }
  return keys.sort()
}

describe('i18n key consistency', () => {
  const enPath = join(I18N_DIR, 'en.json')
  const enJson = JSON.parse(readFileSync(enPath, 'utf-8'))
  const enKeys = collectKeys(enJson)

  const languageFiles = readdirSync(I18N_DIR)
    .filter(f => f.endsWith('.json') && f !== 'en.json')

  it('en.json has keys', () => {
    expect(enKeys.length).toBeGreaterThan(0)
  })

  for (const file of languageFiles) {
    const lang = file.replace('.json', '')
    const langPath = join(I18N_DIR, file)
    const langJson = JSON.parse(readFileSync(langPath, 'utf-8'))
    const langKeys = collectKeys(langJson)

    it(`${lang}: no missing keys (vs en.json)`, () => {
      const missing = enKeys.filter(k => !langKeys.includes(k))
      if (missing.length > 0) {
        throw new Error(`Missing ${missing.length} key(s) in ${file}: ${missing.slice(0, 10).join(', ')}${missing.length > 10 ? '...' : ''}`)
      }
    })

    it(`${lang}: no extra keys (vs en.json)`, () => {
      const extra = langKeys.filter(k => !enKeys.includes(k))
      if (extra.length > 0) {
        throw new Error(`Extra ${extra.length} key(s) in ${file}: ${extra.slice(0, 10).join(', ')}${extra.length > 10 ? '...' : ''}`)
      }
    })
  }
})

describe('systemToAppLocale', () => {
  it('maps Chinese locales to simplified/traditional', () => {
    expect(systemToAppLocale('zh-CN')).toBe('zh-CN')
    expect(systemToAppLocale('zh')).toBe('zh-CN')
    expect(systemToAppLocale('zh-Hans')).toBe('zh-CN')
    expect(systemToAppLocale('zh-TW')).toBe('zh-TW')
    expect(systemToAppLocale('zh-HK')).toBe('zh-TW')
    expect(systemToAppLocale('zh-Hant')).toBe('zh-TW')
  })

  it('maps other supported locales to their base code', () => {
    expect(systemToAppLocale('ja-JP')).toBe('ja')
    expect(systemToAppLocale('fr-FR')).toBe('fr')
    expect(systemToAppLocale('de-DE')).toBe('de')
    expect(systemToAppLocale('ru-RU')).toBe('ru')
    expect(systemToAppLocale('ar-EG')).toBe('ar')
    expect(systemToAppLocale('pt-BR')).toBe('pt')
    expect(systemToAppLocale('ko-KR')).toBe('ko')
  })

  it('returns null for English and unsupported locales (keep en default)', () => {
    expect(systemToAppLocale('en-US')).toBeNull()
    expect(systemToAppLocale('en-GB')).toBeNull()
    expect(systemToAppLocale('es-ES')).toBeNull()
    expect(systemToAppLocale('it-IT')).toBeNull()
  })
})
