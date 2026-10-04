<script setup lang="ts">
/**
 * Browser SSH terminal. The server owns the SSH connection (see `crate::ssh`);
 * this view moves bytes between an xterm instance and a WebSocket. Binary
 * frames carry terminal bytes, text frames carry JSON control messages.
 */
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { ArrowLeft, LoaderCircle, RotateCw, TriangleAlert } from '@lucide/vue'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()

type Status = 'loading' | 'prompt' | 'connecting' | 'ready' | 'closed' | 'error'

interface SshInfo {
  machine: { id: string; name: string; online: boolean; ipv4: string | null }
  available: boolean
  reason: string | null
  defaultUser: string | null
  port: number
  hostKeyVerified: boolean
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

onMounted(async () => {
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
})

onBeforeUnmount(() => {
  resizeObserver?.disconnect()
  socket?.close()
  term?.dispose()
})

async function connect() {
  if (!username.value.trim()) return

  status.value = 'connecting'
  failure.value = null

  // The login and other pages never load xterm; it is imported only when a
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
    theme: { background: '#0b0b0f', foreground: '#e5e5e5' },
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
    // A malformed control frame is not worth tearing the session down for.
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
  <div class="flex h-svh flex-col bg-[#0b0b0f] text-neutral-200">
    <header class="flex items-center gap-3 border-b border-white/10 px-4 py-2">
      <Button variant="ghost" size="sm" class="text-neutral-300" @click="goBack">
        <ArrowLeft />
        {{ t('common.back') }}
      </Button>

      <span class="font-mono text-sm">
        {{ info?.machine.name ?? machineId }}
        <span v-if="status === 'ready'" class="text-neutral-500">
          — {{ username }}@{{ info?.machine.ipv4 }}:{{ info?.port }}
        </span>
      </span>

      <span
        v-if="info && !info.hostKeyVerified"
        class="ml-auto text-xs text-amber-500/80"
        :title="t('ssh.hostKeyHint')"
      >
        {{ t('ssh.hostKeyUnverified') }}
      </span>

      <span class="text-xs text-neutral-400" :class="info && !info.hostKeyVerified ? '' : 'ml-auto'">
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
        class="mx-auto mt-16 w-full max-w-sm space-y-4 rounded-lg border border-white/10 p-6"
      >
        <h1 class="text-lg font-semibold">{{ t('ssh.prompt.title') }}</h1>
        <p class="text-sm text-neutral-400">{{ t('ssh.prompt.description') }}</p>
        <div class="space-y-2">
          <Label for="ssh-user" class="text-neutral-300">{{ t('ssh.prompt.user') }}</Label>
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
        class="flex h-full flex-col items-center justify-center gap-3 text-neutral-400"
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
        <div class="flex gap-2">
          <Button variant="outline" @click="reconnect">
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
          class="pointer-events-none absolute inset-x-0 bottom-6 text-center text-xs text-neutral-400"
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
