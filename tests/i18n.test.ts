import { describe, it, expect } from 'vitest'
import { readdirSync, readFileSync } from 'fs'
import { join } from 'path'

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
