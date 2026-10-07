<script setup lang="ts">
/**
 * Browser SSH terminal. The server owns the SSH connection (see `crate::ssh`).
 * This view moves bytes between an xterm instance and a WebSocket. Binary
 * frames carry terminal bytes, and text frames carry JSON control messages.
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { ArrowLeft, Info, LoaderCircle, RotateCw, TriangleAlert } from '@lucide/vue'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useTheme } from '@/composables/useTheme'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const { resolved: colorScheme } = useTheme()

type Status = 'loading' | 'prompt' | 'connecting' | 'ready' | 'closed' | 'error'

/** Why a session cannot start. Mirrors the server's `reasonCode`. */
type ReasonCode = 'disabled' | 'proxy' | 'version' | 'offline' | 'no_ipv4'

interface SshInfo {
  machine: { id: string; name: string; online: boolean; ipv4: string | null }
  available: boolean
  enabled: boolean
  reasonCode: ReasonCode | null
  reason: string | null
  defaultUser: string | null
  port: number
  hostKeyVerified: boolean
}

/**
 * xterm cannot read CSS variables, so the code chooses its palette in JS from
 * the resolved theme. Both keep the terminal legible on the page it sits on.
 */
const TERMINAL_THEMES = {
  dark: { background: '#0b0b0f', foreground: '#e5e5e5' },
  light: { background: '#ffffff', foreground: '#1f2937' },
} as const

function terminalTheme() {
  return TERMINAL_THEMES[colorScheme.value]
}

const info = ref<SshInfo | null>(null)
const status = ref<Status>('loading')
const failure = ref<string | null>(null)
const username = ref('')
const terminalHost = ref<HTMLDivElement | null>(null)

let socket: WebSocket | null = null
let term: import('@xterm/xterm').Terminal | null = null
let fit: import('@xterm/addon-fit').FitAddon | null = null
let resizeObserver: ResizeObserver | null = null

const machineId = computed(() => String(route.params.id))

function resolve(path: string): URL {
  const base = document.baseURI ?? `${window.location.origin}/`
  return new URL(path.replace(/^\//, ''), base)
}

/** Localized, actionable guidance for the reason a session cannot start. */
const guidance = computed(() =>
  info.value?.reasonCode ? t(`ssh.enable.${info.value.reasonCode}`) : null,
)

// When the theme changes mid-session, keep the terminal palette in step.
watch(colorScheme, () => {
  if (term) term.options.theme = terminalTheme()
})

/**
 * Fetches the data for the session and starts it. A retry re-fetches too, so
 * the view picks up a setting changed after the page loaded (SSH enabled,
 * proxy started) without a reload.
 */
async function load() {
  failure.value = null
  try {
    const response = await fetch(resolve(`api/ssh/${machineId.value}`), {
      credentials: 'same-origin',
    })
    const payload = await response.json()
    if (!response.ok) {
      throw new Error(payload?.error?.message ?? response.statusText)
    }
    info.value = payload as SshInfo
    username.value = payload.defaultUser ?? ''

    if (!payload.available) {
      status.value = 'error'
      failure.value = payload.reason ?? t('ssh.unavailable')
      return
    }
    // Without a configured default, the view prompts for a user name.
    status.value = payload.defaultUser ? 'connecting' : 'prompt'
    if (payload.defaultUser) await connect()
  } catch (err) {
    status.value = 'error'
    failure.value = err instanceof Error ? err.message : String(err)
  }
}

onMounted(load)

onBeforeUnmount(() => {
  resizeObserver?.disconnect()
  socket?.close()
  term?.dispose()
})

async function connect() {
  if (!username.value.trim()) return

  status.value = 'connecting'
  failure.value = null

  // The login and other pages never load xterm. The view imports it only when a
  // session starts.
  const [{ Terminal }, { FitAddon }] = await Promise.all([
    import('@xterm/xterm'),
    import('@xterm/addon-fit'),
  ])
  await import('@xterm/xterm/css/xterm.css')

  await new Promise((resolve) => requestAnimationFrame(resolve))

  term = new Terminal({
    cursorBlink: true,
    fontFamily:
      "'JetBrains Mono', ui-monospace, SFMono-Regular, Menlo, Consolas, monospace",
    fontSize: 14,
    scrollback: 5000,
    theme: terminalTheme(),
  })
  fit = new FitAddon()
  term.loadAddon(fit)
  if (terminalHost.value) {
    term.open(terminalHost.value)
    fit.fit()
  }

  const url = resolve(
    `api/ssh/${machineId.value}/ws?user=${encodeURIComponent(username.value.trim())}` +
      `&cols=${term.cols}&rows=${term.rows}`,
  )
  url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:'

  socket = new WebSocket(url)
  socket.binaryType = 'arraybuffer'

  socket.onmessage = (event) => {
    if (typeof event.data === 'string') {
      handleControl(event.data)
      return
    }
    const bytes = new Uint8Array(event.data as ArrayBuffer)
    term?.write(bytes)
  }

  socket.onclose = () => {
    if (status.value !== 'error') status.value = 'closed'
  }

  socket.onerror = () => {
    failure.value = t('ssh.connectionFailed')
    status.value = 'error'
  }

  term.onData((data) => {
    if (socket?.readyState === WebSocket.OPEN) socket.send(data)
  })

  resizeObserver = new ResizeObserver(() => {
    if (!fit || !term) return
    fit.fit()
    if (socket?.readyState === WebSocket.OPEN) {
      socket.send(JSON.stringify({ type: 'resize', cols: term.cols, rows: term.rows }))
    }
  })
  if (terminalHost.value) resizeObserver.observe(terminalHost.value)
}

function handleControl(raw: string) {
  try {
    const message = JSON.parse(raw) as { type: string; message?: string }
    if (message.type === 'ready') {
      status.value = 'ready'
      term?.focus()
    } else if (message.type === 'exit') {
      status.value = 'closed'
    } else if (message.type === 'error') {
      failure.value = message.message ?? t('ssh.connectionFailed')
      status.value = 'error'
    }
  } catch {
    // A malformed control frame does not justify closing the session.
  }
}

function reconnect() {
  terminate()
  void connect()
}

function terminate() {
  resizeObserver?.disconnect()
  resizeObserver = null
  socket?.close()
  socket = null
  term?.dispose()
  term = null
  fit = null
}

function goBack() {
  void router.push({ name: 'machine', params: { id: machineId.value } })
}
</script>

<template>
  <div class="bg-background text-foreground flex h-svh flex-col">
    <header class="border-border flex items-center gap-3 border-b px-4 py-2">
      <Button variant="ghost" size="sm" @click="goBack">
        <ArrowLeft />
        {{ t('common.back') }}
      </Button>

      <span class="font-mono text-sm">
        {{ info?.machine.name ?? machineId }}
        <span v-if="status === 'ready'" class="text-muted-foreground">
          — {{ username }}@{{ info?.machine.ipv4 }}:{{ info?.port }}
        </span>
      </span>

      <span
        v-if="info && !info.hostKeyVerified"
        class="text-warning ml-auto text-xs"
        :title="t('ssh.hostKeyHint')"
      >
        {{ t('ssh.hostKeyUnverified') }}
      </span>

      <span class="text-muted-foreground text-xs" :class="info && !info.hostKeyVerified ? '' : 'ml-auto'">
        {{
          status === 'connecting'
            ? t('ssh.status.connecting')
            : status === 'ready'
              ? t('ssh.status.connected')
              : status === 'closed'
                ? t('ssh.status.closed')
                : ''
        }}
      </span>
    </header>

    <div class="flex-1 overflow-hidden p-2">
      <div
        v-if="status === 'prompt'"
        class="border-border mx-auto mt-16 w-full max-w-sm space-y-4 rounded-lg border p-6"
      >
        <h1 class="text-lg font-semibold">{{ t('ssh.prompt.title') }}</h1>
        <p class="text-muted-foreground text-sm">{{ t('ssh.prompt.description') }}</p>
        <div class="space-y-2">
          <Label for="ssh-user">{{ t('ssh.prompt.user') }}</Label>
          <Input
            id="ssh-user"
            v-model="username"
            autocomplete="off"
            spellcheck="false"
            @keyup.enter="connect"
          />
        </div>
        <Button :disabled="!username.trim()" @click="connect">
          {{ t('ssh.prompt.connect') }}
        </Button>
      </div>

      <div
        v-else-if="status === 'connecting'"
        class="text-muted-foreground flex h-full flex-col items-center justify-center gap-3"
      >
        <LoaderCircle class="size-6 animate-spin" />
        <p class="text-sm">{{ t('ssh.connecting', { host: info?.machine.name ?? '' }) }}</p>
      </div>

      <div v-else-if="status === 'error'" class="mx-auto mt-16 w-full max-w-lg space-y-4">
        <Alert variant="destructive">
          <TriangleAlert />
          <AlertTitle>{{ t('ssh.failedTitle') }}</AlertTitle>
          <AlertDescription>{{ failure }}</AlertDescription>
        </Alert>

        <!-- How to make SSH work. Without this the only message is that it does
             not, which leaves the operator with nothing to act on. -->
        <Alert v-if="guidance">
          <Info />
          <AlertTitle>{{ t('ssh.enable.title') }}</AlertTitle>
          <AlertDescription class="whitespace-pre-line">{{ guidance }}</AlertDescription>
        </Alert>

        <div class="flex gap-2">
          <Button variant="outline" @click="load">
            <RotateCw />
            {{ t('common.retry') }}
          </Button>
          <Button variant="ghost" @click="goBack">{{ t('common.back') }}</Button>
        </div>
      </div>

      <div v-show="status === 'ready' || status === 'closed'" class="h-full">
        <div ref="terminalHost" class="h-full w-full" />
        <div
          v-if="status === 'closed'"
          class="text-muted-foreground pointer-events-none absolute inset-x-0 bottom-6 text-center text-xs"
        >
          {{ t('ssh.sessionClosed') }}
          <button class="pointer-events-auto ml-2 underline" @click="reconnect">
            {{ t('common.retry') }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
