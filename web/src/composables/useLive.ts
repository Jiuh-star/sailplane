/**
 * Live data through server-sent events. The server pushes a version for each
 * resource. Views watch their version and refetch only then. A dropped
 * connection retries with backoff, and a `resync` frame bumps every version.
 */

import { onScopeDispose, readonly, ref, watch } from 'vue'

export type ResourceKey = 'nodes' | 'users'

const versions = ref<Record<ResourceKey, number>>({ nodes: 0, users: 0 })
/** `null` until the first connection attempt settles. */
const connected = ref<boolean | null>(null)

let source: EventSource | null = null
let retries = 0
let retryTimer: number | null = null
let started = false

function resolve(path: string): string {
  const base = document.baseURI ?? `${window.location.origin}/`
  return new URL(path.replace(/^\//, ''), base).toString()
}

function applyVersions(payload: string, force: boolean) {
  try {
    const parsed = JSON.parse(payload) as Partial<Record<ResourceKey, number>>
    for (const key of ['nodes', 'users'] as const) {
      const next = parsed[key]
      if (typeof next !== 'number') continue
      // A `resync` frame means the client can hold stale data: bump past
      // whatever the server reports so every watcher fires.
      versions.value[key] = force ? Math.max(versions.value[key], next) + 1 : next
    }
  } catch {
    // A malformed frame does not justify closing the stream.
  }
}

function scheduleReconnect() {
  if (retryTimer !== null) return
  const delay = Math.min(1000 * 2 ** retries, 30_000)
  retries += 1
  retryTimer = window.setTimeout(() => {
    retryTimer = null
    connect()
  }, delay)
}

function connect() {
  source?.close()
  connected.value = false

  source = new EventSource(resolve('events/live'), { withCredentials: true })

  source.addEventListener('open', () => {
    connected.value = true
    retries = 0
  })

  source.addEventListener('hello', (event) => applyVersions((event as MessageEvent).data, false))
  source.addEventListener('resync', (event) => applyVersions((event as MessageEvent).data, true))

  source.addEventListener('changed', (event) => {
    try {
      const change = JSON.parse((event as MessageEvent).data) as {
        resource: ResourceKey
        version: number
      }
      versions.value[change.resource] = change.version
    } catch {
      // A malformed frame does not justify closing the stream.
    }
  })

  source.addEventListener('error', () => {
    connected.value = false
    source?.close()
    source = null
    scheduleReconnect()
  })
}

/** Starts the shared stream. Safe to call from every component. */
export function ensureLiveStream() {
  if (started) return
  started = true
  connect()
}

/**
 * Starts the stream for the lifetime of the app shell. A page that watches no
 * resource still uses it for the connection status in the footer.
 */
export function startLiveStream() {
  ensureLiveStream()
}

export function stopLiveStream() {
  started = false
  source?.close()
  source = null
  if (retryTimer !== null) {
    window.clearTimeout(retryTimer)
    retryTimer = null
  }
  connected.value = false
}

/**
 * Runs `onChange` once immediately and again whenever any of the watched
 * resources changes version. When the component unmounts, it stops automatically.
 */
export function useLiveResource(
  resources: ResourceKey | ResourceKey[],
  onChange: () => void,
) {
  const keys = Array.isArray(resources) ? resources : [resources]
  ensureLiveStream()

  const signature = () => keys.map((key) => versions.value[key]).join(':')
  const stop = watch(signature, () => onChange())

  queueMicrotask(onChange)

  onScopeDispose(stop)
  return { versions: readonly(versions), connected: readonly(connected) }
}

export function useLiveStatus() {
  return { connected: readonly(connected) }
}
