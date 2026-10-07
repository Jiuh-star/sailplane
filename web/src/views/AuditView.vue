<script setup lang="ts">
/**
 * The audit log: every state-changing request Sailplane handled. Headscale
 * keeps no history, so this is the only record of a rename, a revoke, or a
 * policy rewrite. The log includes failed attempts.
 */
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { ArrowLeft, LoaderCircle, RefreshCw, ChevronLeft, ChevronRight } from '@lucide/vue'

import EmptyState from '@/components/shared/EmptyState.vue'
import PageHeader from '@/components/shared/PageHeader.vue'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
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
const total = ref(0)
const page = ref(1)
const pages = ref(1)
const perPage = ref(50)
const loading = ref(true)
const failure = ref<string | null>(null)

/** The page-size choices offered in the footer. */
const pageSizes = [25, 50, 100, 200]

/**
 * The endpoint without the mount prefix. The operator chooses the prefix, so
 * the code cannot assume its length.
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
  loading.value = true
  try {
    const response = await api.audit.list(page.value, perPage.value)
    entries.value = response.entries
    total.value = response.total
    // The server clamps the page when entries were trimmed since the last load.
    page.value = response.page
    pages.value = response.pages
    failure.value = null
  } catch (err) {
    failure.value = errorMessage(err)
  } finally {
    loading.value = false
  }
}

function goTo(target: number) {
  const clamped = Math.min(Math.max(target, 1), pages.value)
  if (clamped === page.value) return
  page.value = clamped
  load()
}

// A new page size reflows the row boundaries, so start again from the top.
watch(perPage, () => {
  page.value = 1
  load()
})

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
        <Badge variant="secondary">{{ t('audit.count', { count: total }) }}</Badge>
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

      <div class="flex flex-wrap items-center justify-between gap-4">
        <p class="text-muted-foreground text-sm">
          {{ t('common.shownOf', { shown: entries.length, total }) }}
        </p>

        <div class="flex items-center gap-2">
          <span class="text-muted-foreground text-sm">{{ t('audit.rowsPerPage') }}</span>
          <Select v-model="perPage">
            <SelectTrigger class="w-20" :aria-label="t('audit.rowsPerPage')">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem v-for="size in pageSizes" :key="size" :value="size">{{ size }}</SelectItem>
            </SelectContent>
          </Select>
        </div>

        <div class="flex items-center gap-2">
          <Button
            variant="outline"
            size="icon"
            :disabled="page <= 1 || loading"
            :aria-label="t('audit.previous')"
            @click="goTo(page - 1)"
          >
            <ChevronLeft />
          </Button>
          <span class="text-sm tabular-nums">{{ t('audit.pageOf', { page, pages }) }}</span>
          <Button
            variant="outline"
            size="icon"
            :disabled="page >= pages || loading"
            :aria-label="t('audit.next')"
            @click="goTo(page + 1)"
          >
            <ChevronRight />
          </Button>
        </div>
      </div>
    </template>
  </div>
</template>
