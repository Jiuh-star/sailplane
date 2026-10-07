/**
 * Internationalization. Messages live in `src/locales/<tag>.ts`. The active
 * locale persists to `localStorage` and defaults to the browser's preference.
 * The build bundles all messages, so a locale switch never hits the network.
 */

import { createI18n } from 'vue-i18n'

import en from '@/locales/en/index'
import zhCN from '@/locales/zh-CN/index'

export const SUPPORTED_LOCALES = [
  { tag: 'en', label: 'English' },
  { tag: 'zh-CN', label: '简体中文' },
] as const

export type LocaleTag = (typeof SUPPORTED_LOCALES)[number]['tag']

const STORAGE_KEY = 'sailplane:locale'
const FALLBACK: LocaleTag = 'en'

function isSupported(tag: string): tag is LocaleTag {
  return SUPPORTED_LOCALES.some((locale) => locale.tag === tag)
}

/** Best guess for a first visit: the browser's language, then English. */
function detectLocale(): LocaleTag {
  for (const candidate of navigator.languages ?? [navigator.language]) {
    if (isSupported(candidate)) return candidate
    // Every `zh-*` tag lands on the simplified translation. There is no
    // traditional translation to prefer yet.
    const base = candidate.split('-')[0]
    if (base === 'zh') return 'zh-CN'
  }
  return FALLBACK
}

function storedLocale(): LocaleTag | null {
  const stored = localStorage.getItem(STORAGE_KEY)
  return stored && isSupported(stored) ? stored : null
}

export const i18n = createI18n({
  legacy: false,
  globalInjection: true,
  locale: storedLocale() ?? detectLocale(),
  fallbackLocale: FALLBACK,
  messages: { en, 'zh-CN': zhCN },
})

export function currentLocale(): LocaleTag {
  return i18n.global.locale.value as LocaleTag
}

export function setLocale(tag: LocaleTag) {
  i18n.global.locale.value = tag
  localStorage.setItem(STORAGE_KEY, tag)
  document.documentElement.lang = tag
}

/** Applies the persisted locale's `lang` attribute on boot. */
export function applyStoredLocale() {
  document.documentElement.lang = currentLocale()
}

/** The translator for use outside components, such as stores and helpers. */
export function t(key: string, params?: Record<string, unknown>): string {
  return params
    ? i18n.global.t(key, params)
    : i18n.global.t(key)
}
