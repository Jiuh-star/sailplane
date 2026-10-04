/** Formatting helpers shared across views. */

import { currentLocale } from '@/i18n'

// Intl defaults to the browser language, not the UI language. A Chinese browser
// on the English UI would show Chinese dates. Formatters are keyed by locale so
// a language switch rebuilds them.
const dateFormatters = new Map<string, Intl.DateTimeFormat>()
const relativeFormatters = new Map<string, Intl.RelativeTimeFormat>()

function dateFormatter(): Intl.DateTimeFormat {
  const tag = currentLocale()
  let formatter = dateFormatters.get(tag)
  if (!formatter) {
    formatter = new Intl.DateTimeFormat(tag, { dateStyle: 'medium', timeStyle: 'short' })
    dateFormatters.set(tag, formatter)
  }
  return formatter
}

function relativeFormatter(): Intl.RelativeTimeFormat {
  const tag = currentLocale()
  let formatter = relativeFormatters.get(tag)
  if (!formatter) {
    formatter = new Intl.RelativeTimeFormat(tag, { numeric: 'auto' })
    relativeFormatters.set(tag, formatter)
  }
  return formatter
}

/** Formats an RFC 3339 timestamp, or returns a placeholder for empty values. */
export function formatDate(input: string | null | undefined, fallback = '—'): string {
  if (!input) return fallback
  const date = new Date(input)
  if (Number.isNaN(date.getTime())) return fallback
  // Headscale uses Go's zero time for "never".
  if (date.getFullYear() <= 1) return fallback
  return dateFormatter().format(date)
}

/** Formats a coarse relative time, such as "3 minutes ago". */
export function formatRelative(input: string | null | undefined, fallback = '—'): string {
  if (!input) return fallback
  const date = new Date(input)
  if (Number.isNaN(date.getTime()) || date.getFullYear() <= 1) return fallback

  const seconds = Math.round((date.getTime() - Date.now()) / 1000)
  const formatter = relativeFormatter()

  const units: [Intl.RelativeTimeFormatUnit, number][] = [
    ['second', 60],
    ['minute', 60],
    ['hour', 24],
    ['day', 7],
    ['week', 4.348],
    ['month', 12],
    ['year', Number.POSITIVE_INFINITY],
  ]

  let value = seconds
  for (const [unit, limit] of units) {
    if (Math.abs(value) < limit) {
      return formatter.format(Math.round(value), unit)
    }
    value /= limit
  }
  return formatter.format(Math.round(value), 'year')
}

/** Returns true when a Headscale timestamp means "never expires". */
export function isNever(input: string | null | undefined): boolean {
  if (!input) return true
  const date = new Date(input)
  return Number.isNaN(date.getTime()) || date.getFullYear() <= 1
}

/**
 * Copies text to the clipboard and reports success.
 *
 * `navigator.clipboard` needs a secure context, and a tailnet URL over plain
 * HTTP is not one. The legacy selection-based copy is the fallback.
 */
export async function copyText(value: string): Promise<boolean> {
  if (window.isSecureContext && navigator.clipboard) {
    try {
      await navigator.clipboard.writeText(value)
      return true
    } catch {
      // Permission denied or the document is not focused; try the fallback.
    }
  }

  return legacyCopy(value)
}

function legacyCopy(value: string): boolean {
  const area = document.createElement('textarea')
  area.value = value
  // Keep it in the layout but out of sight: `display: none` and
  // `visibility: hidden` both prevent selection.
  area.setAttribute('readonly', '')
  area.style.position = 'fixed'
  area.style.top = '0'
  area.style.left = '0'
  area.style.width = '2em'
  area.style.height = '2em'
  area.style.padding = '0'
  area.style.border = 'none'
  area.style.outline = 'none'
  area.style.boxShadow = 'none'
  area.style.background = 'transparent'
  area.style.opacity = '0'

  const selection = document.getSelection()
  const previous = selection && selection.rangeCount > 0 ? selection.getRangeAt(0) : null

  document.body.appendChild(area)
  area.select()
  area.setSelectionRange(0, value.length)

  let ok = false
  try {
    ok = document.execCommand('copy')
  } catch {
    ok = false
  }

  document.body.removeChild(area)
  if (previous && selection) {
    selection.removeAllRanges()
    selection.addRange(previous)
  }

  return ok
}

/** Returns a stable colour for an avatar fallback, derived from the label. */
export function avatarHue(label: string): number {
  let hash = 0
  for (let index = 0; index < label.length; index += 1) {
    hash = (hash * 31 + label.charCodeAt(index)) % 360
  }
  return hash
}

export function initials(label: string): string {
  const parts = label.trim().split(/\s+/).filter(Boolean)
  if (parts.length === 0) return '?'
  if (parts.length === 1) return parts[0]!.slice(0, 2).toUpperCase()
  return `${parts[0]![0]}${parts[parts.length - 1]![0]}`.toUpperCase()
}

/** Formats a byte count. */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  const units = ['KB', 'MB', 'GB', 'TB']
  let value = bytes / 1024
  let index = 0
  while (value >= 1024 && index < units.length - 1) {
    value /= 1024
    index += 1
  }
  return `${value.toFixed(1)} ${units[index]}`
}

/** Splits comma- or space-separated user input into a list. */
export function parseList(input: string): string[] {
  return input
    .split(/[,\s]+/)
    .map((item) => item.trim())
    .filter(Boolean)
}
