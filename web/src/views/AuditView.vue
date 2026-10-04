<script setup lang="ts">
/**
 * The audit log: every state-changing request Sailplane handled. Headscale
 * keeps no history, so this is the only record of a rename, a revoke, or a
 * policy rewrite. Failed attempts are included.
 */
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { ArrowLeft, LoaderCircle, RefreshCw } from '@lucide/vue'

import EmptyState from '@/components/shared/EmptyState.vue'
import PageHeader from '@/components/shared/PageHeader.vue'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
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
import { errorMessage } from '@/composables/useToast'
import { formatDate } from '@/lib/format'
import { api, type AuditEntry } from '@/lib/api'

const { t } = useI18n()

const entries = ref<AuditEntry[]>([])
const next = ref<number | null>(null)
const loading = ref(true)
const loadingMore = ref(false)
const failure = ref<string | null>(null)

/**
 * The endpoint without the mount prefix. The operator chooses the prefix, so
 * its length cannot be assumed.
 */
function action(path: string): string {
  const marker = path.indexOf('/api/')
  return marker === -1 ? path : path.slice(marker + '/api/'.length)
}

function tone(status: number): 'success' | 'warning' | 'destructive' {
  if (status < 300) return 'success'
  if (status < 500) return 'warning'
  return 'destructive'
}

const failed = computed(() => entries.value.filter((entry) => entry.status >= 400).length)

async function load() {
  try {
    const response = await api.audit.list()
    entries.value = response.entries
    next.value = response.next
    failure.value = null
  } catch (err) {
    failure.value = errorMessage(err)
  } finally {
    loading.value = false
  }
}

async function loadMore() {
  if (next.value === null || loadingMore.value) return
  loadingMore.value = true
  try {
    const response = await api.audit.list(next.value)
    entries.value = [...entries.value, ...response.entries]
    next.value = response.next
  } catch (err) {
    failure.value = errorMessage(err)
  } finally {
    loadingMore.value = false
  }
}

onMounted(load)
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

    <PageHeader :title="t('audit.title')" :description="t('audit.description')">
      <template #actions>
        <Button variant="outline" :disabled="loading" @click="load">
          <LoaderCircle v-if="loading" class="animate-spin" />
          <RefreshCw v-else />
          {{ t('audit.refresh') }}
        </Button>
      </template>
    </PageHeader>

    <Alert v-if="failure" variant="destructive">
      <AlertTitle>{{ t('audit.loadFailed') }}</AlertTitle>
      <AlertDescription>{{ failure }}</AlertDescription>
    </Alert>

    <template v-else>
      <div class="flex flex-wrap items-center gap-2">
        <Badge variant="secondary">{{ t('audit.count', { count: entries.length }) }}</Badge>
        <Badge v-if="failed" variant="warning">
          {{ t('audit.failedCount', { count: failed }) }}
        </Badge>
      </div>

      <div class="rounded-lg border">
        <Table class="hp-cards">
          <TableHeader>
            <TableRow>
              <TableHead>{{ t('audit.table.when') }}</TableHead>
              <TableHead>{{ t('audit.table.actor') }}</TableHead>
              <TableHead>{{ t('audit.table.action') }}</TableHead>
              <TableHead>{{ t('audit.table.result') }}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            <TableEmpty v-if="loading" :colspan="4">
              <Skeleton class="h-6 w-full" />
            </TableEmpty>
            <TableEmpty v-else-if="!entries.length" :colspan="4">
              <EmptyState :title="t('audit.empty')" :description="t('audit.emptyDescription')" />
            </TableEmpty>

            <TableRow v-for="entry in entries" :key="entry.id">
              <TableCell class="text-sm" :label="t('audit.table.when')">
                {{ formatDate(entry.at) }}
              </TableCell>
              <TableCell :label="t('audit.table.actor')">
                <div class="flex flex-col">
                  <span class="text-sm">{{ entry.actor }}</span>
                  <span class="text-muted-foreground text-xs">{{ entry.role }}</span>
                </div>
              </TableCell>
              <TableCell :label="t('audit.table.action')">
                <div class="flex items-center gap-2">
                  <Badge variant="outline" class="font-mono text-[0.65rem]">{{
                    entry.method
                  }}</Badge>
                  <code class="text-xs">{{ action(entry.path) }}</code>
                </div>
              </TableCell>
              <TableCell :label="t('audit.table.result')">
                <Badge :variant="tone(entry.status)">{{ entry.status }}</Badge>
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </div>

      <div v-if="next !== null" class="flex justify-center">
        <Button variant="outline" :disabled="loadingMore" @click="loadMore">
          <LoaderCircle v-if="loadingMore" class="animate-spin" />
          {{ t('audit.loadMore') }}
        </Button>
      </div>
    </template>
  </div>
</template>
