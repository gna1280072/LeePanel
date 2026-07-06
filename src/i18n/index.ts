import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import en from './en.json'
import zhCN from './zh-CN.json'

i18n.use(initReactI18next).init({
  resources: {
    en: { translation: en },
    'zh-CN': { translation: zhCN },
  },
  lng: 'en',
  fallbackLng: 'en',
  interpolation: { escapeValue: false },
})

// ponytail: async load persisted language from SQLite, switch if different
invoke<string>('ui_state_get', { key: 'language' })
  .then(lang => { if (lang && lang !== 'en') i18n.changeLanguage(lang) })
  .catch(() => {})

export default i18n
