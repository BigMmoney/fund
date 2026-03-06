import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import LanguageDetector from 'i18next-browser-languagedetector'

import en from './locales/en.json'
import zh from './locales/zh.json'
import de from './locales/de.json'
import fr from './locales/fr.json'

// Supported languages
export const SUPPORTED_LANGUAGES = ['en', 'zh', 'de', 'fr'] as const
export type SupportedLanguage = typeof SUPPORTED_LANGUAGES[number]

export const LANGUAGE_CONFIG: Record<SupportedLanguage, { label: string; nativeLabel: string; flag: string }> = {
  en: { label: 'English', nativeLabel: 'English', flag: '🇺🇸' },
  zh: { label: 'Chinese', nativeLabel: '中文', flag: '🇨🇳' },
  de: { label: 'German', nativeLabel: 'Deutsch', flag: '🇩🇪' },
  fr: { label: 'French', nativeLabel: 'Français', flag: '🇫🇷' },
}

i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources: {
      en: { translation: en },
      zh: { translation: zh },
      de: { translation: de },
      fr: { translation: fr },
    },
    fallbackLng: 'en',
    debug: false,
    interpolation: {
      escapeValue: false, // React already escapes
    },
    detection: {
      order: ['localStorage', 'navigator'],
      caches: ['localStorage'],
    },
  })

export default i18n
