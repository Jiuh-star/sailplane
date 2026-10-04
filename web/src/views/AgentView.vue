<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { ExternalLink, LoaderCircle, RefreshCw } from '@lucide/vue'

import PageHeader from '@/components/shared/PageHeader.vue'
import StatusCircle from '@/components/shared/StatusCircle.vue'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { errorMessage, useToast } from '@/composables/useToast'
import { api, type AgentStatus } from '@/lib/api'
import { formatRelative } from '@/lib/format'

const { t } = useI18n()
const toast = useToast()

const agent = ref<AgentStatus | null>(null)
const loading = ref(true)
const syncing = ref(false)
const failure = ref<string | null>(null)

async function load() {
  try {
    const response = await api.agent.status()
    agent.value = response.agent
    failure.value = null
  } catch (err) {
    failure.value = errorMessage(err)
  } finally {
    loading.value = false
  }
}

onMounted(load)

const healthy = computed(() => Boolean(agent.value?.enabled) && !agent.value?.error)

async function sync() {
  syncing.value = true
  try {
    const result = await api.agent.sync()
    agent.value = result.agent
    if (result.ok) {
      toast.success(
        t('agent.toast.synced'),
        t('agent.nodesReported', { count: result.nodeCount ?? 0 }),
      )
    } else {
      toast.error(t('agent.toast.syncFailed'), result.error)
    }
  } catch (err) {
    toast.error(t('agent.toast.syncFailed'), errorMessage(err))
  } finally {
    syncing.value = false
  }
}
</script>

<template>
  <div class="space-y-6">
    <PageHeader :title="t('agent.title')" :description="t('agent.description')" />

    <div v-if="loading" class="text-muted-foreground flex items-center gap-2 text-sm">
      <LoaderCircle class="size-4 animate-spin" />
      {{ t('agent.loading') }}
    </div>

    <Alert v-else-if="failure" variant="destructive">
      <AlertTitle>{{ t('agent.loadFailedTitle') }}</AlertTitle>
      <AlertDescription>{{ failure }}</AlertDescription>
    </Alert>

    <template v-else-if="agent">
      <Alert v-if="!agent.enabled" variant="warning">
        <AlertTitle>{{ t('agent.disabled.title') }}</AlertTitle>
        <AlertDescription>
          {{ agent.reason }}
          <i18n-t keypath="agent.disabled.description" tag="span">
            <template #config>
              <code>integration.agent</code>
            </template>
          </i18n-t>
        </AlertDescription>
      </Alert>

      <template v-else>
        <Card>
          <CardHeader>
            <CardTitle class="flex items-center gap-2 text-base">
              <StatusCircle :online="healthy" size="md" />
              {{
                healthy
                  ? t('agent.status.healthy')
                  : agent.error
                    ? t('agent.status.error')
                    : t('agent.status.pendingApproval')
              }}
              <Badge variant="secondary">{{ agent.backend }}</Badge>
            </CardTitle>
            <CardDescription>
              {{
                t('agent.lastSynced', {
                  time: formatRelative(agent.synced_at, t('common.never')),
                })
              }}
              · {{ t('agent.nodesReported', { count: agent.node_count }) }}
            </CardDescription>
          </CardHeader>
          <CardContent>
            <Button :disabled="syncing" @click="sync">
              <LoaderCircle v-if="syncing" class="animate-spin" />
              <RefreshCw v-else />
              {{ t('agent.syncNow') }}
            </Button>
          </CardContent>
        </Card>

        <Alert v-if="agent.auth_url" variant="warning">
          <AlertTitle>{{ t('agent.approval.title') }}</AlertTitle>
          <AlertDescription class="space-y-2">
            <p>{{ t('agent.approval.description') }}</p>
            <a
              :href="agent.auth_url"
              target="_blank"
              rel="noreferrer noopener"
              class="inline-flex items-center gap-1 underline"
            >
              {{ t('agent.approval.action') }}
              <ExternalLink class="size-3" />
            </a>
          </AlertDescription>
        </Alert>

        <Alert v-if="agent.error" variant="destructive">
          <AlertTitle>{{ t('agent.syncError.title') }}</AlertTitle>
          <AlertDescription>{{ agent.error }}</AlertDescription>
        </Alert>
      </template>
    </template>
  </div>
</template>
