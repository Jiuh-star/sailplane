/**
 * Resolves server-sent machine status badges. A key unknown to the bundle falls
 * back to the English label the server sends.
 */
import type { StatusTag } from './api'

type Translate = (key: string, params?: Record<string, unknown>) => string
type HasKey = (key: string) => boolean

export function statusTagLabel(tag: StatusTag, t: Translate, te: HasKey): string {
  const key = `machine.statusTags.${tag.key}`
  if (!tag.key || !te(key)) return tag.label
  return t(key, { route: tag.subject ?? '' })
}
