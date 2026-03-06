import React, { useState, useRef, useEffect } from 'react'
import { Globe, ChevronDown, Check } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { LANGUAGE_CONFIG, SupportedLanguage, SUPPORTED_LANGUAGES } from '../i18n'

interface LanguageSelectorProps {
  variant?: 'navbar' | 'dropdown' | 'compact'
  className?: string
}

const LanguageSelector: React.FC<LanguageSelectorProps> = ({ 
  variant = 'navbar',
  className = '' 
}) => {
  const { t, i18n } = useTranslation()
  const language = (SUPPORTED_LANGUAGES.includes(i18n.language as SupportedLanguage) 
    ? i18n.language 
    : 'en') as SupportedLanguage
  const [isOpen, setIsOpen] = useState(false)
  const dropdownRef = useRef<HTMLDivElement>(null)

  const changeLanguage = (lang: SupportedLanguage) => {
    i18n.changeLanguage(lang)
  }

  // Close dropdown when clicking outside
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (dropdownRef.current && !dropdownRef.current.contains(event.target as Node)) {
        setIsOpen(false)
      }
    }
    document.addEventListener('mousedown', handleClickOutside)
    return () => document.removeEventListener('mousedown', handleClickOutside)
  }, [])

  const currentLang = LANGUAGE_CONFIG[language] || LANGUAGE_CONFIG.en

  // Compact variant - for inline use
  if (variant === 'compact') {
    return (
      <div ref={dropdownRef} className={`relative ${className}`}>
        <button
          onClick={() => setIsOpen(!isOpen)}
          className="flex items-center gap-1.5 px-2.5 py-1.5 text-xs bg-gray-100 border border-gray-200 rounded-lg hover:border-gray-300 text-gray-600 hover:text-gray-900 transition-colors"
        >
          <span className="text-sm">{currentLang.flag}</span>
          <span className="font-medium">{currentLang.nativeLabel}</span>
        </button>
        
        {isOpen && (
          <div className="absolute top-full right-0 mt-1 w-36 bg-white border border-gray-200 rounded-xl shadow-lg z-50 overflow-hidden">
            {SUPPORTED_LANGUAGES.map(langCode => {
              const option = LANGUAGE_CONFIG[langCode]
              return (
                <button
                  key={langCode}
                  onClick={() => {
                    changeLanguage(langCode)
                    setIsOpen(false)
                  }}
                  className={`w-full flex items-center justify-between px-3 py-2.5 text-xs hover:bg-gray-50 transition-colors ${
                    language === langCode ? 'text-blue-600 bg-blue-50' : 'text-gray-600'
                  }`}
                >
                  <div className="flex items-center gap-2">
                    <span className="text-base">{option.flag}</span>
                    <span className="font-medium">{option.nativeLabel}</span>
                  </div>
                  {language === langCode && <Check className="w-3.5 h-3.5" />}
                </button>
              )
            })}
          </div>
        )}
      </div>
    )
  }

  // Dropdown variant - for forms/settings
  if (variant === 'dropdown') {
    return (
      <div ref={dropdownRef} className={`relative ${className}`}>
        <button
          onClick={() => setIsOpen(!isOpen)}
          className="flex items-center gap-2 px-3 py-1.5 text-xs bg-gray-100 border border-gray-200 rounded-lg hover:border-blue-300 text-gray-600 hover:text-gray-900 transition-all"
        >
          <span className="text-base">{currentLang.flag}</span>
          <span className="font-medium">{currentLang.nativeLabel}</span>
          <ChevronDown className={`w-3 h-3 transition-transform ${isOpen ? 'rotate-180' : ''}`} />
        </button>
        
        {isOpen && (
          <div className="absolute top-full right-0 mt-2 w-48 bg-white border border-gray-200 rounded-xl shadow-xl z-50 overflow-hidden">
            <div className="px-3 py-2 border-b border-gray-100 bg-gray-50">
              <span className="text-[10px] text-gray-500 uppercase tracking-wider font-semibold">
                {t('nav.language')}
              </span>
            </div>
            {SUPPORTED_LANGUAGES.map(langCode => {
              const option = LANGUAGE_CONFIG[langCode]
              return (
                <button
                  key={langCode}
                  onClick={() => {
                    changeLanguage(langCode)
                    setIsOpen(false)
                  }}
                  className={`w-full flex items-center justify-between px-3 py-3 text-xs hover:bg-gray-50 transition-colors ${
                    language === langCode ? 'text-blue-600 bg-blue-50' : 'text-gray-600'
                  }`}
                >
                  <div className="flex items-center gap-2.5">
                    <span className="text-lg">{option.flag}</span>
                    <div className="flex flex-col items-start">
                      <span className="font-medium">{option.nativeLabel}</span>
                      <span className="text-[10px] text-gray-400">{option.label}</span>
                    </div>
                  </div>
                  {language === langCode && <Check className="w-4 h-4" />}
                </button>
              )
            })}
          </div>
        )}
      </div>
    )
  }

  // Navbar variant (default) - for navigation bar
  return (
    <div ref={dropdownRef} className={`relative ${className}`}>
      <button
        onClick={() => setIsOpen(!isOpen)}
        className="flex items-center gap-2 px-3 py-2 text-sm bg-white border border-gray-200 rounded-lg hover:border-blue-300 text-gray-700 hover:text-gray-900 transition-all shadow-sm"
      >
        <span className="text-base">{currentLang.flag}</span>
        <span className="font-medium">{currentLang.nativeLabel}</span>
        <ChevronDown className={`w-4 h-4 transition-transform ${isOpen ? 'rotate-180' : ''}`} />
      </button>
      
      {isOpen && (
        <div className="absolute top-full right-0 mt-2 w-52 bg-white border border-gray-200 rounded-xl shadow-2xl z-50 overflow-hidden">
          <div className="px-4 py-3 border-b border-gray-100 bg-gray-50">
            <div className="flex items-center gap-2">
              <Globe className="w-4 h-4 text-blue-500" />
              <span className="text-xs font-bold text-gray-700 uppercase tracking-wider">
                {t('nav.language')}
              </span>
            </div>
          </div>
          <div className="py-1">
            {SUPPORTED_LANGUAGES.map(langCode => {
              const option = LANGUAGE_CONFIG[langCode]
              return (
                <button
                  key={langCode}
                  onClick={() => {
                    changeLanguage(langCode)
                    setIsOpen(false)
                  }}
                  className={`w-full flex items-center justify-between px-4 py-3 text-sm hover:bg-gray-50 transition-colors ${
                    language === langCode ? 'text-blue-600 bg-blue-50' : 'text-gray-600'
                  }`}
                >
                  <div className="flex items-center gap-3">
                    <span className="text-xl">{option.flag}</span>
                    <div className="flex flex-col items-start gap-0.5">
                      <span className="font-medium">{option.nativeLabel}</span>
                      <span className="text-[10px] text-gray-400">{option.label}</span>
                    </div>
                  </div>
                  {language === langCode && (
                    <Check className="w-4 h-4" />
                  )}
                </button>
              )
            })}
          </div>
        </div>
      )}
    </div>
  )
}

export default LanguageSelector
