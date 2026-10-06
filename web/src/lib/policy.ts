/**
 * Normalisers for the optional policy sections shared by the ACL editor and the
 * topology policy panel: `autoApprovers`, `nodeAttrs` and `postures`.
 *
 * These shapes come from Headscale and may be absent or malformed, so each
 * normaliser returns an empty list rather than throwing.
 */
import type { NodeAttr } from '@/lib/api'

export function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function stringList(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((entry): entry is string => typeof entry === 'string')
    : []
}

/** `autoApprovers.routes` as `CIDR → selectors`. */
export function parseApproverRoutes(approvers: unknown): [string, string[]][] {
  if (!isPlainObject(approvers) || !isPlainObject(approvers.routes)) return []
  return Object.entries(approvers.routes).map(([cidr, selectors]) => [cidr, stringList(selectors)])
}

/** `autoApprovers.exitNode` as a list of selectors. */
export function parseApproverExitNodes(approvers: unknown): string[] {
  if (!isPlainObject(approvers) || !Array.isArray(approvers.exitNode)) return []
  return stringList(approvers.exitNode)
}

/** `nodeAttrs` entries that carry both a target and at least one attribute. */
export function parseNodeAttrs(value: unknown): NodeAttr[] {
  if (!Array.isArray(value)) return []
  return value.filter(
    (entry): entry is NodeAttr =>
      isPlainObject(entry) && Array.isArray(entry.target) && Array.isArray(entry.attr),
  )
}

/** `postures` as `name → conditions`. */
export function parsePostures(value: unknown): [string, string[]][] {
  if (!isPlainObject(value)) return []
  return Object.entries(value).map(([name, conditions]) => [name, stringList(conditions)])
}
