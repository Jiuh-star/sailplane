/** Dark/light/system theme handling, persisted to localStorage. */

import { computed, ref } from 'vue'

export type ColorScheme = 'light' | 'dark' | 'system'

const STORAGE_KEY = 'sailplane:color-scheme'
const scheme = ref<ColorScheme>('system')

const media = typeof window !== 'undefined' ? window.matchMedia('(prefers-color-scheme: dark)') : null

function resolved(): 'light' | 'dark' {
  if (scheme.value === 'system') return media?.matches ? 'dark' : 'light'
  return scheme.value
}

/** Applies the stored preference to `<html class="dark">`. */
export function applyStoredTheme() {
  const stored = localStorage.getItem(STORAGE_KEY) as ColorScheme | null
  if (stored === 'light' || stored === 'dark' || stored === 'system') {
    scheme.value = stored
  }
  document.documentElement.classList.toggle('dark', resolved() === 'dark')

  media?.addEventListener('change', () => {
    if (scheme.value === 'system') {
      document.documentElement.classList.toggle('dark', resolved() === 'dark')
    }
  })
}

export function useTheme() {
  function setScheme(next: ColorScheme) {
    scheme.value = next
    localStorage.setItem(STORAGE_KEY, next)
    document.documentElement.classList.toggle('dark', resolved() === 'dark')
  }

  return {
    scheme: computed(() => scheme.value),
    resolved: computed(resolved),
    setScheme,
  }
}
