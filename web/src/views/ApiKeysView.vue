<script setup lang="ts">
/**
 * Headscale API keys that this deployment authenticates with. Revoking matters
 * most: a stale key is a standing grant of full control-plane access, so
 * creating keys stays a CLI act.
 */
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { ArrowLeft, Check, Copy, LoaderCircle, Plus, ShieldAlert } from '@lucide/vue'

import ConfirmDialog from '@/components/shared/ConfirmDialog.vue'
import EmptyState from '@/components/shared/EmptyState.vue'
import PageHeader from '@/components/shared/PageHeader.vue'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import {
  Table,
  TableBody,
  TableCell,
  TableEmpty,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { errorMessage, useToast } from '@/composables/useToast'
import { copyText, formatRelative } from '@/lib/format'
import { api, type HeadscaleApiKey } from '@/lib/api'

const { t } = useI18n()
const toast = useToast()

const keys = ref<HeadscaleApiKey[]>([])
const loading = ref(true)
const busy = ref(false)
const failure = ref<string | null>(null)
const revokeTarget = ref<HeadscaleApiKey | null>(null)
const copied = ref(false)

const createCommand = 'headscale apikeys create --expiration 90d'

/** Keys that no longer authenticate: already expired. */
function isExpired(key: HeadscaleApiKey): boolean {
  if (!key.expiration) return false
  const date = new Date(key.expiration)
  return !Number.isNaN(date.getTime()) && date < new Date()
}

const active = computed(() => keys.value.filter((key) => !isExpired(key)).length)

async function load() {
  try {
    const response = await api.apiKeys.list()
    keys.value = response.keys
    failure.value = null
  } catch (err) {
    failure.value = errorMessage(err)
  } finally {
    loading.value = false
  }
}

onMounted(load)

async function revoke(key: HeadscaleApiKey) {
  busy.value = true
  try {
    await api.apiKeys.revoke(key.id)
    toast.success(t('apiKeys.toast.revoked'))
    await load()
  } catch (err) {
    toast.error(t('apiKeys.toast.revokeFailed'), errorMessage(err))
  } finally {
    busy.value = false
    revokeTarget.value = null
  }
}

async function copyCommand() {
  if (await copyText(createCommand)) {
    copied.value = true
    window.setTimeout(() => (copied.value = false), 1500)
  }
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

    <PageHeader :title="t('apiKeys.title')" :description="t('apiKeys.description')" />

    <Alert variant="warning">
      <ShieldAlert />
      <AlertTitle>{{ t('apiKeys.warningTitle') }}</AlertTitle>
      <AlertDescription>{{ t('apiKeys.warningBody') }}</AlertDescription>
    </Alert>

    <Alert v-if="failure" variant="destructive">
      <AlertTitle>{{ t('apiKeys.loadFailed') }}</AlertTitle>
      <AlertDescription>{{ failure }}</AlertDescription>
    </Alert>

    <template v-else>
      <Card class="gap-3 py-4">
        <CardHeader class="flex-row items-center justify-between">
          <CardTitle class="text-base">{{ t('apiKeys.table.title') }}</CardTitle>
          <Badge variant="secondary">
            {{ t('apiKeys.summary', { active: active, total: keys.length }) }}
          </Badge>
        </CardHeader>
        <CardContent class="px-0">
          <Table class="hp-cards">
            <TableHeader>
              <TableRow>
                <TableHead>{{ t('apiKeys.table.prefix') }}</TableHead>
                <TableHead>{{ t('apiKeys.table.created') }}</TableHead>
                <TableHead>{{ t('apiKeys.table.lastSeen') }}</TableHead>
                <TableHead>{{ t('apiKeys.table.expires') }}</TableHead>
                <TableHead><span class="sr-only">{{ t('apiKeys.table.actions') }}</span></TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              <TableEmpty v-if="loading" :colspan="5">
                <Skeleton class="h-6 w-full" />
              </TableEmpty>
              <TableEmpty v-else-if="!keys.length" :colspan="5">
                <EmptyState :title="t('apiKeys.empty')" :description="t('apiKeys.emptyDescription')" />
              </TableEmpty>

              <TableRow v-for="key in keys" :key="key.id || key.prefix">
                <TableCell>
                  <code class="text-xs">{{ key.prefix }}</code>
                </TableCell>
                <TableCell class="text-sm" :label="t('apiKeys.table.created')">{{
                  formatRelative(key.createdAt, '—')
                }}</TableCell>
                <TableCell class="text-sm" :label="t('apiKeys.table.lastSeen')">{{
                  formatRelative(key.lastSeen, '—')
                }}</TableCell>
                <TableCell class="text-sm" :label="t('apiKeys.table.expires')">
                  <Badge v-if="isExpired(key)" variant="destructive">
                    {{ t('apiKeys.expired') }}
                  </Badge>
                  <template v-else>{{ formatRelative(key.expiration, t('apiKeys.never')) }}</template>
                </TableCell>
                <TableCell class="text-right" label="">
                  <Button
                    size="sm"
                    variant="ghost"
                    class="text-destructive hover:text-destructive"
                    :disabled="busy || isExpired(key)"
                    @click="revokeTarget = key"
                  >
                    {{ t('apiKeys.revoke') }}
                  </Button>
                </TableCell>
              </TableRow>
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      <Card>
        <CardContent class="flex items-start gap-3">
          <Plus class="text-muted-foreground mt-0.5 size-4 shrink-0" />
          <div class="min-w-0 space-y-2">
            <p class="text-sm font-medium">{{ t('apiKeys.createTitle') }}</p>
            <p class="text-muted-foreground text-sm">{{ t('apiKeys.createBody') }}</p>
            <div class="flex flex-wrap items-center gap-2">
              <code
                class="bg-muted max-w-full overflow-x-auto rounded-md px-2.5 py-1.5 font-mono text-xs"
              >
                {{ createCommand }}
              </code>
              <Button size="sm" variant="outline" @click="copyCommand">
                <Check v-if="copied" />
                <Copy v-else />
                {{ copied ? t('apiKeys.copied') : t('apiKeys.copyCommand') }}
              </Button>
            </div>
          </div>
        </CardContent>
      </Card>
    </template>

    <ConfirmDialog
      :open="revokeTarget !== null"
      :title="t('apiKeys.revokeDialog.title')"
      :description="t('apiKeys.revokeDialog.description', { prefix: revokeTarget?.prefix ?? '' })"
      :confirm-label="t('apiKeys.revokeDialog.confirm')"
      destructive
      @update:open="revokeTarget = $event ? revokeTarget : null"
      @confirm="revokeTarget && revoke(revokeTarget)"
    />

    <div v-if="loading" class="flex justify-center py-6">
      <LoaderCircle class="text-muted-foreground size-5 animate-spin" />
    </div>
  </div>
</template>
