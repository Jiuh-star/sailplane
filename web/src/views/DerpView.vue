<script setup lang="ts">
/**
 * DERP configuration: where the tailnet's relays come from. Headscale builds
 * the relay map from the config file, not its API. Thus every change is a
 * config edit through the comment-preserving editor, then a reload.
 */
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { ArrowLeft, LoaderCircle, Plus, Server, X } from '@lucide/vue'

import EmptyState from '@/components/shared/EmptyState.vue'
import PageHeader from '@/components/shared/PageHeader.vue'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Skeleton } from '@/components/ui/skeleton'
import { Switch } from '@/components/ui/switch'
import { errorMessage, useToast } from '@/composables/useToast'
import { api, type DerpConfig, type DerpRegion } from '@/lib/api'

const { t } = useI18n()
const toast = useToast()

/// Defaults rather than null: the template renders the cards only once `loaded` is set, so a
/// non-nullable shape keeps field access simple.
const derp = ref<DerpConfig>({
  urls: [],
  paths: [],
  autoUpdateEnabled: false,
  updateFrequency: '24h',
  serverEnabled: false,
})
const loaded = ref(false)
const writable = ref(false)
const available = ref(false)
const loading = ref(true)
const saving = ref(false)
const failure = ref<string | null>(null)

const newUrl = ref('')
const newPath = ref('')
const frequency = ref('24h')

const regions = ref<DerpRegion[]>([])
const relayErrors = ref<string[]>([])
const relaysLoading = ref(true)

const canWrite = computed(() => writable.value && !saving.value)

/** Resolves the configured sources into the relay servers they contain. */
async function loadRelays() {
  relaysLoading.value = true
  try {
    const response = await api.derp.relays()
    regions.value = response.regions
    relayErrors.value = response.errors
  } catch (err) {
    relayErrors.value = [errorMessage(err)]
  } finally {
    relaysLoading.value = false
  }
}

async function load() {
  try {
    const response = await api.derp.get()
    derp.value = response.derp
    writable.value = response.access.writable && response.access.write
    available.value = response.access.available
    frequency.value = response.derp.updateFrequency
    loaded.value = true
    failure.value = null
  } catch (err) {
    failure.value = errorMessage(err)
  } finally {
    loading.value = false
  }
}

onMounted(() => {
  void load()
  void loadRelays()
})

/** Sends one field at a time so a request can never clear what it omits. */
async function save(patch: Record<string, unknown>, done: string) {
  if (!canWrite.value) return
  saving.value = true
  try {
    const response = await api.derp.update(patch)
    toast.success(done)
    if (response.warning) toast.warning(t('derp.warningTitle'), response.warning)
    await Promise.all([load(), loadRelays()])
  } catch (err) {
    toast.error(t('derp.failedTitle'), errorMessage(err))
  } finally {
    saving.value = false
  }
}

function addUrl() {
  const value = newUrl.value.trim()
  if (!value) return
  void save({ urls: [...derp.value.urls, value] }, t('derp.toast.urlAdded'))
  newUrl.value = ''
}

function removeUrl(url: string) {
  void save(
    { urls: derp.value.urls.filter((entry) => entry !== url) },
    t('derp.toast.urlRemoved'),
  )
}

function addPath() {
  const value = newPath.value.trim()
  if (!value) return
  void save({ paths: [...derp.value.paths, value] }, t('derp.toast.pathAdded'))
  newPath.value = ''
}

function removePath(path: string) {
  void save(
    { paths: derp.value.paths.filter((entry) => entry !== path) },
    t('derp.toast.pathRemoved'),
  )
}
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

    <PageHeader :title="t('derp.title')" :description="t('derp.description')" />

    <Alert v-if="failure" variant="destructive">
      <AlertTitle>{{ t('derp.loadFailed') }}</AlertTitle>
      <AlertDescription>{{ failure }}</AlertDescription>
    </Alert>

    <Alert v-else-if="!available" variant="warning">
      <AlertTitle>{{ t('derp.unavailableTitle') }}</AlertTitle>
      <AlertDescription>{{ t('derp.unavailableBody') }}</AlertDescription>
    </Alert>

    <Alert v-else-if="!writable" variant="warning">
      <AlertTitle>{{ t('derp.readOnlyTitle') }}</AlertTitle>
      <AlertDescription>{{ t('derp.readOnlyBody') }}</AlertDescription>
    </Alert>

    <div v-if="loading" class="space-y-2">
      <Skeleton v-for="index in 3" :key="index" class="h-28 w-full" />
    </div>

    <template v-else-if="loaded">
      <Card>
        <CardHeader>
          <CardTitle class="text-base">{{ t('derp.urls.title') }}</CardTitle>
          <CardDescription>{{ t('derp.urls.description') }}</CardDescription>
        </CardHeader>
        <CardContent class="space-y-3">
          <EmptyState v-if="!derp.urls.length" :title="t('derp.urls.empty')" />
          <div
            v-for="url in derp.urls"
            :key="url"
            class="flex items-center gap-3 rounded-md border p-3 text-sm"
          >
            <code class="min-w-0 flex-1 truncate text-xs">{{ url }}</code>
            <Button
              size="sm"
              variant="ghost"
              class="text-destructive"
              :disabled="!canWrite"
              @click="removeUrl(url)"
            >
              <X />
              {{ t('common.delete') }}
            </Button>
          </div>

          <div class="flex gap-2">
            <Input
              v-model="newUrl"
              placeholder="https://controlplane.tailscale.com/derpmap/default"
              autocomplete="off"
              spellcheck="false"
              :disabled="!canWrite"
              @keyup.enter="addUrl"
            />
            <Button :disabled="!canWrite || !newUrl.trim()" @click="addUrl">
              <Plus />
              {{ t('common.add') }}
            </Button>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle class="text-base">{{ t('derp.paths.title') }}</CardTitle>
          <CardDescription>{{ t('derp.paths.description') }}</CardDescription>
        </CardHeader>
        <CardContent class="space-y-3">
          <EmptyState v-if="!derp.paths.length" :title="t('derp.paths.empty')" />
          <div
            v-for="path in derp.paths"
            :key="path"
            class="flex items-center gap-3 rounded-md border p-3 text-sm"
          >
            <code class="min-w-0 flex-1 truncate text-xs">{{ path }}</code>
            <Button
              size="sm"
              variant="ghost"
              class="text-destructive"
              :disabled="!canWrite"
              @click="removePath(path)"
            >
              <X />
              {{ t('common.delete') }}
            </Button>
          </div>

          <div class="flex gap-2">
            <Input
              v-model="newPath"
              placeholder="/etc/headscale/derp.yaml"
              autocomplete="off"
              spellcheck="false"
              :disabled="!canWrite"
              @keyup.enter="addPath"
            />
            <Button :disabled="!canWrite || !newPath.trim()" @click="addPath">
              <Plus />
              {{ t('common.add') }}
            </Button>
          </div>
          <p class="text-muted-foreground text-xs">{{ t('derp.paths.hint') }}</p>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle class="text-base">{{ t('derp.updates.title') }}</CardTitle>
          <CardDescription>{{ t('derp.updates.description') }}</CardDescription>
        </CardHeader>
        <CardContent class="space-y-4">
          <div class="flex items-center justify-between gap-4">
            <div>
              <Label for="derp-auto">{{ t('derp.updates.auto') }}</Label>
              <p class="text-muted-foreground text-xs">{{ t('derp.updates.autoHint') }}</p>
            </div>
            <Switch
              id="derp-auto"
              :model-value="derp.autoUpdateEnabled"
              :disabled="!canWrite"
              @update:model-value="
                (value: boolean) =>
                  save({ auto_update_enabled: value }, t('derp.toast.updated'))
              "
            />
          </div>

          <div class="flex items-end gap-2">
            <div class="space-y-2">
              <Label for="derp-frequency">{{ t('derp.updates.frequency') }}</Label>
              <Input
                id="derp-frequency"
                v-model="frequency"
                class="w-40"
                placeholder="24h"
                autocomplete="off"
                spellcheck="false"
                :disabled="!canWrite"
              />
            </div>
            <Button
              variant="outline"
              :disabled="!canWrite || frequency === derp.updateFrequency"
              @click="save({ update_frequency: frequency }, t('derp.toast.updated'))"
            >
              <LoaderCircle v-if="saving" class="animate-spin" />
              {{ t('common.save') }}
            </Button>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle class="flex items-center gap-2 text-base">
            <Server class="size-4" />
            {{ t('derp.server.title') }}
            <Badge v-if="derp.serverEnabled" variant="success">{{ t('common.enabled') }}</Badge>
          </CardTitle>
          <CardDescription>{{ t('derp.server.description') }}</CardDescription>
        </CardHeader>
        <CardContent class="flex items-center justify-between gap-4">
          <p class="text-muted-foreground text-sm">{{ t('derp.server.hint') }}</p>
          <Switch
            id="derp-server"
            :model-value="derp.serverEnabled"
            :disabled="!canWrite"
            @update:model-value="
              (value: boolean) => save({ server_enabled: value }, t('derp.toast.updated'))
            "
          />
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle class="text-base">{{ t('derp.relays.title') }}</CardTitle>
          <CardDescription>{{ t('derp.relays.description') }}</CardDescription>
        </CardHeader>
        <CardContent class="space-y-3">
          <div v-if="relaysLoading" class="space-y-2">
            <Skeleton class="h-16 w-full" />
          </div>

          <Alert v-for="(error, index) in relayErrors" :key="index" variant="warning">
            <AlertDescription>{{ error }}</AlertDescription>
          </Alert>

          <EmptyState
            v-if="!relaysLoading && !regions.length"
            :title="t('derp.relays.empty')"
            :description="t('derp.relays.emptyDescription')"
          />

          <div v-for="region in regions" :key="region.id" class="space-y-2 rounded-md border p-3">
            <div class="flex flex-wrap items-center gap-2">
              <Badge variant="outline">{{ region.code || `#${region.id}` }}</Badge>
              <span class="text-sm font-medium">
                {{ region.name || t('derp.relays.unnamed') }}
              </span>
              <Badge variant="secondary">
                {{ t('derp.relays.count', { count: region.nodes.length }) }}
              </Badge>
              <span class="text-muted-foreground ml-auto font-mono text-xs">{{ region.source }}</span>
            </div>

            <div class="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
              <div v-for="node in region.nodes" :key="node.name" class="rounded border p-2 text-xs">
                <p class="truncate font-mono font-medium">{{ node.hostname }}</p>
                <p class="text-muted-foreground mt-0.5 flex gap-2">
                  <span v-if="node.derp_port">
                    {{ t('derp.relays.derpPort', { port: node.derp_port }) }}
                  </span>
                  <span v-if="node.stun_port">
                    {{ t('derp.relays.stunPort', { port: node.stun_port }) }}
                  </span>
                </p>
              </div>
            </div>
          </div>
        </CardContent>
      </Card>
    </template>
  </div>
</template>
