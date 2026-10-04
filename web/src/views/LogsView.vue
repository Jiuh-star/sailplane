<script setup lang="ts">
/**
 * The Headscale container's logs. The stream is plain text over `fetch`, not
 * SSE, so the browser treats it as one. Following keeps the connection open
 * until the reader stops or navigates away.
 */
import { computed, nextTick, onBeforeUnmount, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { ArrowLeft, Pause, Play, RefreshCw, Trash2 } from '@lucide/vue'

import PageHeader from '@/components/shared/PageHeader.vue'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { errorMessage } from '@/composables/useToast'

const { t } = useI18n()

const tails = [100, 500, 2000]
const tail = ref(500)
const follow = ref(true)
const text = ref('')
const failure = ref<string | null>(null)
const running = ref(false)

const viewport = ref<HTMLDivElement | null>(null)
let controller: AbortController | null = null

/** Keeps the DOM bounded while following a chatty container. */
const MAX_CHARS = 200_000

const lineCount = computed(() => (text.value ? text.value.trimEnd().split('\n').length : 0))

function url(): string {
  const base = document.baseURI ?? `${window.location.origin}/`
  const path = `api/logs?tail=${tail.value}&follow=${follow.value ? 1 : 0}`
  return new URL(path.replace(/^\//, ''), base).toString()
}

async function scrollToEnd() {
  await nextTick()
  const element = viewport.value
  if (element) element.scrollTop = element.scrollHeight
}

function append(chunk: string) {
  text.value = (text.value + chunk).slice(-MAX_CHARS)
  if (follow.value) void scrollToEnd()
}

async function start() {
  stop()
  failure.value = null
  running.value = true
  controller = new AbortController()

  try {
    const response = await fetch(url(), {
      credentials: 'same-origin',
      signal: controller.signal,
    })

    if (!response.ok) {
      const payload = await response.json().catch(() => null)
      throw new Error(payload?.error?.message ?? response.statusText)
    }
    if (!response.body) throw new Error(t('logs.noStream'))

    const reader = response.body.getReader()
    const decoder = new TextDecoder()

    // A non-following read ends on its own; a following one runs until stopped.
    for (;;) {
      const { done, value } = await reader.read()
      if (done) break
      append(decoder.decode(value, { stream: true }))
    }
  } catch (err) {
    // An abort is a normal stop, not a failure to report.
    if (!(err instanceof DOMException && err.name === 'AbortError')) {
      failure.value = errorMessage(err)
    }
  } finally {
    running.value = false
    controller = null
  }
}

function stop() {
  controller?.abort()
  controller = null
  running.value = false
}

function restart() {
  text.value = ''
  void start()
}

onBeforeUnmount(stop)
</script>

<template>
  <div class="space-y-6">
    <RouterLink
      :to="{ name: 'settings' }"
      class="text-muted-foreground hover:text-foreground inline-flex items-center gap-1 text-sm"
    >
      <ArrowLeft class="size-4" />
      {{ t('nav.settings') }}
    </RouterLink>

    <PageHeader :title="t('logs.title')" :description="t('logs.description')" />

    <div class="flex flex-wrap items-end gap-3">
      <div class="space-y-2">
        <Label for="log-tail">{{ t('logs.history') }}</Label>
        <select
          id="log-tail"
          v-model.number="tail"
          class="border-input bg-background h-9 rounded-md border px-3 text-sm"
          :disabled="running"
        >
          <option v-for="size in tails" :key="size" :value="size">
            {{ t('logs.lines', { count: size }) }}
          </option>
        </select>
      </div>

      <label class="flex items-center gap-2 pb-2 text-sm">
        <input v-model="follow" type="checkbox" class="accent-primary size-4" :disabled="running" />
        {{ t('logs.follow') }}
      </label>

      <div class="flex gap-2 pb-0.5">
        <Button v-if="!running" @click="start">
          <Play />
          {{ t('logs.start') }}
        </Button>
        <Button v-else variant="outline" @click="stop">
          <Pause />
          {{ t('logs.stop') }}
        </Button>

        <Button variant="outline" :disabled="running" @click="restart">
          <RefreshCw />
          {{ t('logs.reload') }}
        </Button>

        <Button variant="ghost" :disabled="!text" @click="text = ''">
          <Trash2 />
          {{ t('logs.clear') }}
        </Button>
      </div>

      <Badge v-if="lineCount" variant="secondary" class="mb-2">
        {{ t('logs.lineCount', { count: lineCount }) }}
      </Badge>
    </div>

    <Alert v-if="failure" variant="destructive">
      <AlertTitle>{{ t('logs.failedTitle') }}</AlertTitle>
      <AlertDescription>{{ failure }}</AlertDescription>
    </Alert>

    <div
      ref="viewport"
      class="h-[60vh] overflow-auto rounded-lg border bg-[#0b0b0f] p-3 font-mono text-xs leading-relaxed text-neutral-200"
    >
      <pre v-if="text" class="whitespace-pre">{{ text }}</pre>
      <p v-else class="text-neutral-500">
        {{ running ? t('logs.waiting') : t('logs.idle') }}
      </p>
    </div>
  </div>
</template>
