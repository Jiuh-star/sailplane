<script setup lang="ts">
/**
 * The tailnet's shape: policy connections, networks behind a single machine,
 * and machine relays. Each edge is a rule expanded through the access check's
 * evaluator, so the graph shows what the data plane will do.
 */
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { RouterLink } from 'vue-router'
import { LoaderCircle, TriangleAlert } from '@lucide/vue'

import AccessGraph from '@/components/topology/AccessGraph.vue'
import EmptyState from '@/components/shared/EmptyState.vue'
import PageHeader from '@/components/shared/PageHeader.vue'
import StatusCircle from '@/components/shared/StatusCircle.vue'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { errorMessage } from '@/composables/useToast'
import { api, type TopologyResponse } from '@/lib/api'

const { t } = useI18n()

const data = ref<TopologyResponse | null>(null)
const loading = ref(true)
const failure = ref<string | null>(null)

onMounted(async () => {
  try {
    data.value = await api.topology.get()
  } catch (err) {
    failure.value = errorMessage(err)
  } finally {
    loading.value = false
  }
})

const aclEdges = computed(() => data.value?.edges.filter((edge) => edge.kind === 'acl') ?? [])
const sshEdges = computed(() => data.value?.edges.filter((edge) => edge.kind === 'ssh') ?? [])

/** Routes nobody else advertises: losing that machine loses the network. */
const soleRoutes = computed(() => data.value?.routes.filter((route) => route.sole) ?? [])

/** Edges grouped by source, for the narrow-screen rendering. */
const bySource = computed(() => {
  const groups = new Map<string, NonNullable<TopologyResponse['edges']>>()
  for (const edge of data.value?.edges ?? []) {
    const list = groups.get(edge.src) ?? []
    list.push(edge)
    groups.set(edge.src, list)
  }
  return [...groups.entries()]
})
</script>

<template>
  <div class="space-y-6">
    <PageHeader :title="t('topology.title')" :description="t('topology.description')" />

    <Alert v-if="failure" variant="destructive">
      <AlertTitle>{{ t('topology.loadFailed') }}</AlertTitle>
      <AlertDescription>{{ failure }}</AlertDescription>
    </Alert>

    <div v-else-if="loading" class="flex justify-center py-10">
      <LoaderCircle class="text-muted-foreground size-5 animate-spin" />
    </div>

    <template v-else-if="data">
      <div class="flex flex-wrap gap-2">
        <Badge variant="secondary">
          {{ t('topology.summary.nodes', { count: data.totals.machines }) }}
        </Badge>
        <Badge variant="success">
          {{ t('topology.summary.online', { count: data.totals.online }) }}
        </Badge>
        <Badge variant="secondary">
          {{ t('topology.summary.rules', { count: data.totals.rules }) }}
        </Badge>
        <Badge v-if="soleRoutes.length" variant="warning">
          {{ t('topology.summary.soleRoutes', { count: soleRoutes.length }) }}
        </Badge>
      </div>

      <Alert v-if="soleRoutes.length" variant="warning">
        <TriangleAlert />
        <AlertTitle>{{ t('topology.sole.title') }}</AlertTitle>
        <AlertDescription>
          {{
            t('topology.sole.body', {
              routes: soleRoutes.map((route) => route.cidr).join(', '),
            })
          }}
        </AlertDescription>
      </Alert>

      <EmptyState
        v-if="!data.edges.length"
        :title="t('topology.noRules')"
        :description="t('topology.noRulesDescription')"
      />

      <template v-else>
        <Card>
          <CardHeader>
            <CardTitle class="text-base">{{ t('topology.graph.title') }}</CardTitle>
            <p class="text-muted-foreground text-sm">{{ t('topology.graph.description') }}</p>
          </CardHeader>
          <CardContent>
            <!-- A phone gets the edges as a
                 list, because a scaled-down diagram is unreadable. -->
            <div class="hidden md:block">
              <AccessGraph :identities="data.identities" :edges="data.edges" />
            </div>

            <div class="space-y-3 md:hidden">
              <div v-for="[source, edges] in bySource" :key="source" class="rounded-md border p-3">
                <code class="text-xs font-medium">{{ source }}</code>
                <div v-for="edge in edges" :key="`${edge.kind}-${edge.rule}`" class="mt-1.5">
                  <span class="text-muted-foreground text-xs">
                    → <code>{{ edge.dst }}</code>:{{ edge.ports }}
                  </span>
                  <Badge class="ml-2" :variant="edge.kind === 'ssh' ? 'info' : 'secondary'">
                    {{ edge.kind === 'ssh' ? t('topology.graph.ssh') : t('topology.graph.acl') }}
                  </Badge>
                </div>
              </div>
            </div>

            <div class="text-muted-foreground mt-3 flex flex-wrap gap-4 text-xs">
              <span>{{ t('topology.graph.legendAcl', { count: aclEdges.length }) }}</span>
              <span>{{ t('topology.graph.legendSsh', { count: sshEdges.length }) }}</span>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle class="text-base">{{ t('topology.routes.title') }}</CardTitle>
            <p class="text-muted-foreground text-sm">{{ t('topology.routes.description') }}</p>
          </CardHeader>
          <CardContent class="space-y-3">
            <EmptyState
              v-if="!data.routes.length"
              :title="t('topology.routes.empty')"
              :description="t('topology.routes.emptyDescription')"
            />

            <div
              v-for="route in data.routes"
              :key="route.cidr"
              class="flex flex-wrap items-center gap-3 rounded-md border p-3"
            >
              <code class="text-sm font-medium">{{ route.cidr }}</code>
              <Badge v-if="route.exit_node" variant="info">{{ t('topology.routes.exitNode') }}</Badge>
              <Badge :variant="route.approved ? 'success' : 'warning'">
                {{
                  route.approved
                    ? t('topology.routes.approved')
                    : t('topology.routes.pending')
                }}
              </Badge>
              <Badge v-if="route.sole" variant="destructive">{{ t('topology.routes.sole') }}</Badge>

              <span class="text-muted-foreground ml-auto flex flex-wrap items-center gap-3 text-xs">
                <RouterLink
                  v-for="advertiser in route.advertisers"
                  :key="advertiser.id"
                  :to="{ name: 'machine', params: { id: advertiser.id } }"
                  class="hover:text-foreground inline-flex items-center gap-1.5"
                >
                  <StatusCircle :online="advertiser.online" />
                  {{ advertiser.name }}
                </RouterLink>
              </span>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle class="text-base">{{ t('topology.relays.title') }}</CardTitle>
            <p class="text-muted-foreground text-sm">{{ t('topology.relays.description') }}</p>
          </CardHeader>
          <CardContent class="space-y-4">
            <div v-for="relay in data.relays" :key="relay.region" class="space-y-2">
              <Badge variant="outline">
                {{
                  relay.region === 'unknown'
                    ? t('topology.relays.unknown')
                    : t('topology.relays.region', { id: relay.region })
                }}
              </Badge>

              <div class="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
                <RouterLink
                  v-for="machine in relay.machines"
                  :key="machine.id"
                  :to="{ name: 'machine', params: { id: machine.id } }"
                  class="hover:bg-accent flex items-center gap-2 rounded-md border p-2.5 text-sm transition-colors"
                >
                  <StatusCircle :online="machine.online" />
                  <span class="min-w-0 flex-1 truncate font-medium">{{ machine.name }}</span>
                </RouterLink>
              </div>
            </div>
          </CardContent>
        </Card>
      </template>
    </template>
  </div>
</template>
