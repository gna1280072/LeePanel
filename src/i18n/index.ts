import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import en from './en.json'
import zhCN from './zh-CN.json'
import zhTW from './zh-TW.json'
import ja from './ja.json'
import fr from './fr.json'
import de from './de.json'
import ru from './ru.json'
import ar from './ar.json'
import pt from './pt.json'
import ko from './ko.json'

i18n.use(initReactI18next).init({
  resources: {
    en: { translation: en },
    'zh-CN': { translation: zhCN },
    'zh-TW': { translation: zhTW },
    'ja': { translation: ja },
    'fr': { translation: fr },
    'de': { translation: de },
    'ru': { translation: ru },
    'ar': { translation: ar },
    'pt': { translation: pt },
    'ko': { translation: ko },
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
