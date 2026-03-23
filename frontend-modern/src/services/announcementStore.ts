export interface ExchangeAnnouncement {
  text: string
  enabled: boolean
  updatedAt: string
  updatedBy?: string
}

const STORAGE_KEY = 'exchange:announcement'
const EVENT_NAME = 'exchange-announcement:update'

const defaultAnnouncement: ExchangeAnnouncement = {
  text: '欢迎来到 Pre Trading Exchange，当前为本地演示环境；公告可在管理后台直接修改并实时推送到顶部滚动条。',
  enabled: true,
  updatedAt: new Date().toISOString(),
  updatedBy: 'system',
}

function canUseBrowser() {
  return typeof window !== 'undefined' && typeof window.localStorage !== 'undefined'
}

export function readAnnouncement(): ExchangeAnnouncement {
  if (!canUseBrowser()) return defaultAnnouncement
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY)
    if (!raw) return defaultAnnouncement
    const parsed = JSON.parse(raw) as Partial<ExchangeAnnouncement>
    if (typeof parsed.text !== 'string') return defaultAnnouncement
    return {
      text: parsed.text,
      enabled: parsed.enabled !== false,
      updatedAt: typeof parsed.updatedAt === 'string' ? parsed.updatedAt : defaultAnnouncement.updatedAt,
      updatedBy: typeof parsed.updatedBy === 'string' ? parsed.updatedBy : defaultAnnouncement.updatedBy,
    }
  } catch {
    return defaultAnnouncement
  }
}

export function writeAnnouncement(next: ExchangeAnnouncement) {
  if (!canUseBrowser()) return
  window.localStorage.setItem(STORAGE_KEY, JSON.stringify(next))
  window.dispatchEvent(new CustomEvent<ExchangeAnnouncement>(EVENT_NAME, { detail: next }))
}

export function clearAnnouncement() {
  writeAnnouncement(defaultAnnouncement)
}

export function subscribeAnnouncement(listener: (value: ExchangeAnnouncement) => void) {
  if (!canUseBrowser()) return () => {}

  const onCustom = (event: Event) => {
    const custom = event as CustomEvent<ExchangeAnnouncement>
    if (custom.detail) listener(custom.detail)
  }

  const onStorage = (event: StorageEvent) => {
    if (event.key !== STORAGE_KEY) return
    listener(readAnnouncement())
  }

  window.addEventListener(EVENT_NAME, onCustom as EventListener)
  window.addEventListener('storage', onStorage)

  return () => {
    window.removeEventListener(EVENT_NAME, onCustom as EventListener)
    window.removeEventListener('storage', onStorage)
  }
}

