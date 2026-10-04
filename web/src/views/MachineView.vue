<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { ArrowLeft, LoaderCircle, Terminal } from '@lucide/vue'

import MachineActions from '@/components/machines/MachineActions.vue'
import CopyButton from '@/components/shared/CopyButton.vue'
import StatusCircle from '@/components/shared/StatusCircle.vue'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { useLiveResource } from '@/composables/useLive'
import { errorMessage } from '@/composables/useToast'
import { statusTagLabel } from '@/lib/statusTags'
import { api, type Machine, type MachineListResponse } from '@/lib/api'
import { formatDate, formatRelative, isNever } from '@/lib/format'

const route = useRoute()
const router = useRouter()
const { t, te } = useI18n()

const machine = ref<Machine | null>(null)
const magic = ref<string | null>(null)
const access = ref<{ read: boolean; write: boolean }>({ read: false, write: false })
const supports = ref<MachineListResponse['supports']>({
  nodeOwnerChange: false,
  disablingKeyExpiry: false,
})
const users = ref<MachineListResponse['users']>([])
const existingTags = ref<string[]>([])
const policyTags = ref<string[]>([])
const loading = ref(true)
const failure = ref<string | null>(null)

const id = computed(() => String(route.params.id))

async function load() {
  try {
    const response = await api.machines.detail(id.value)
    machine.value = response.machine
    magic.value = response.magic
    access.value = response.access
    supports.value = response.supports
    failure.value = null

    // The list endpoint carries the tag vocabulary the dialogs need.
    const list = await api.machines.list()
    users.value = list.users
    policyTags.value = list.policyTags
    const tags = new Set<string>()
    for (const node of list.machines) for (const tag of node.tags) tags.add(tag)
    existingTags.value = [...tags].sort()
  } catch (err) {
    failure.value = errorMessage(err)
  } finally {
    loading.value = false
  }
}

useLiveResource('nodes', load)

const hostInfo = computed(() => machine.value?.host_info ?? null)

const netInfo = computed(() => {
  const info = (hostInfo.value?.NetInfo ?? null) as Record<string, unknown> | null
  if (!info) return []
  return [
    [t('machine.netInfo.variesByDestination'), info.MappingVariesByDestIP],
    [t('machine.netInfo.hairpinning'), info.HairPinning],
    [t('machine.ipv6'), info.WorkingIPv6],
    [t('machine.netInfo.udp'), info.WorkingUDP],
    [t('machine.netInfo.upnp'), info.UPnP],
    [t('machine.netInfo.pcp'), info.PCP],
    [t('machine.netInfo.natPmp'), info.PMP],
  ].filter(([, value]) => typeof value === 'boolean') as [string, boolean][]
})

// The terminal is a full-screen route rather than a popup window: the browser
// back button returns to this page, and the address is shareable.
function openSsh() {
  if (!machine.value) return
  void router.push({ name: 'ssh', params: { id: machine.value.id } })
}

function fullDomain(): string {
  if (!machine.value || !magic.value) return ''
  return `${machine.value.givenName}.${magic.value}`
}
</script>

<template>
  <div class="space-y-6">
    <div v-if="loading" class="text-muted-foreground flex items-center gap-2 text-sm">
      <LoaderCircle class="size-4 animate-spin" />
      {{ t('machine.loading') }}
    </div>

    <Alert v-else-if="failure" variant="destructive">
      <AlertTitle>{{ t('machine.loadFailed') }}</AlertTitle>
      <AlertDescription>{{ failure }}</AlertDescription>
    </Alert>

    <template v-else-if="machine">
      <div class="space-y-3">
        <RouterLink
          :to="{ name: 'machines' }"
          class="text-muted-foreground hover:text-foreground inline-flex items-center gap-1 text-sm"
        >
          <ArrowLeft class="size-4" />
          {{ t('machine.allMachines') }}
        </RouterLink>

        <div class="flex flex-wrap items-start justify-between gap-4">
          <div class="flex items-center gap-3">
            <h1 class="text-2xl font-semibold">{{ machine.givenName }}</h1>
            <StatusCircle :online="machine.online" :expired="machine.expired" size="md" />
          </div>

          <div class="flex items-center gap-2">
            <!-- The bridge uses the server's own tailnet identity,
                 so it is offered only to accounts that may manage the node. -->
            <Button
              v-if="machine.online && access.write"
              variant="outline"
              @click="openSsh"
            >
              <Terminal />
              {{ t('ssh.open') }}
            </Button>

            <MachineActions
              :machine="machine"
              :users="users"
              :existing-tags="existingTags"
              :policy-tags="policyTags"
              :supports="supports"
              :can-write="access.write"
              @changed="load"
            />
          </div>
        </div>

        <div class="flex flex-wrap items-center gap-2">
          <Badge variant="secondary">
            {{
              t('machine.managedBy', {
                owner: machine.user?.displayName ?? machine.user?.name ?? t('machine.tagOwned'),
              })
            }}
          </Badge>
          <Badge
            v-for="tag in machine.status_tags"
            :key="tag.label"
            :variant="
              tag.kind === 'success'
                ? 'success'
                : tag.kind === 'warning'
                  ? 'warning'
                  : tag.kind === 'danger'
                    ? 'destructive'
                    : tag.kind === 'info'
                      ? 'info'
                      : 'outline'
            "
          >
            {{ statusTagLabel(tag, t, te) }}
          </Badge>
          <Badge v-for="tag in machine.tags" :key="tag" variant="outline">{{ tag }}</Badge>
        </div>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>{{ t('machine.routes.title') }}</CardTitle>
          <CardDescription>
            {{ t('machine.routes.description') }}
          </CardDescription>
        </CardHeader>
        <CardContent class="grid gap-4 sm:grid-cols-3">
          <div class="space-y-1">
            <p class="text-sm font-medium">{{ t('machine.routes.approved') }}</p>
            <p v-if="!machine.approvedRoutes.length" class="text-muted-foreground text-sm">—</p>
            <ul v-else class="space-y-0.5 font-mono text-xs">
              <li v-for="route in machine.approvedRoutes" :key="route">{{ route }}</li>
            </ul>
          </div>
          <div class="space-y-1">
            <p class="text-sm font-medium">{{ t('machine.routes.awaiting') }}</p>
            <p v-if="machine.availableRoutes.length === machine.approvedRoutes.length" class="text-muted-foreground text-sm">
              —
            </p>
            <ul v-else class="space-y-0.5 font-mono text-xs">
              <li
                v-for="route in machine.availableRoutes.filter((r) => !machine!.approvedRoutes.includes(r))"
                :key="route"
              >
                {{ route }}
              </li>
            </ul>
          </div>
          <div class="space-y-1">
            <p class="text-sm font-medium">{{ t('machine.routes.exitNode') }}</p>
            <p class="text-muted-foreground text-sm">
              {{
                machine.approvedRoutes.includes('0.0.0.0/0')
                  ? t('machine.routes.allowed')
                  : machine.availableRoutes.includes('0.0.0.0/0')
                    ? t('machine.routes.awaiting')
                    : '—'
              }}
            </p>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{{ t('machine.details.title') }}</CardTitle>
        </CardHeader>
        <CardContent class="grid gap-6 sm:grid-cols-2">
          <dl class="space-y-3 text-sm">
            <div class="flex justify-between gap-4">
              <dt class="text-muted-foreground">{{ t('machine.details.owner') }}</dt>
              <dd>{{ machine.user?.displayName ?? machine.user?.name ?? t('machines.tagOwned') }}</dd>
            </div>
            <div class="flex justify-between gap-4">
              <dt class="text-muted-foreground">{{ t('machine.details.name') }}</dt>
              <dd>{{ machine.name || '—' }}</dd>
            </div>
            <div class="flex justify-between gap-4">
              <dt class="text-muted-foreground">{{ t('machine.details.version') }}</dt>
              <dd>{{ machine.version ?? t('common.unknown') }}</dd>
            </div>
            <div class="flex justify-between gap-4">
              <dt class="text-muted-foreground">{{ t('machine.details.os') }}</dt>
              <dd>{{ machine.os ?? '—' }}</dd>
            </div>
            <div class="flex items-center justify-between gap-4">
              <dt class="text-muted-foreground">{{ t('machine.details.nodeId') }}</dt>
              <dd class="flex items-center gap-1 font-mono text-xs">
                {{ machine.id }}
                <CopyButton :value="machine.id" label="" />
              </dd>
            </div>
            <div class="flex items-center justify-between gap-4">
              <dt class="text-muted-foreground">{{ t('machine.details.nodeKey') }}</dt>
              <dd class="flex items-center gap-1 font-mono text-xs">
                {{ machine.nodeKey.slice(0, 24) }}…
                <CopyButton :value="machine.nodeKey" label="" />
              </dd>
            </div>
            <div class="flex justify-between gap-4">
              <dt class="text-muted-foreground">{{ t('machine.details.created') }}</dt>
              <dd>{{ formatDate(machine.createdAt) }}</dd>
            </div>
            <div class="flex justify-between gap-4">
              <dt class="text-muted-foreground">{{ t('machine.details.lastSeen') }}</dt>
              <dd>
                {{
                  machine.online
                    ? t('machines.status.connected')
                    : formatDate(machine.lastSeen, t('common.never'))
                }}
              </dd>
            </div>
            <div class="flex justify-between gap-4">
              <dt class="text-muted-foreground">{{ t('machine.details.keyExpiry') }}</dt>
              <dd>
                {{ isNever(machine.expiry) ? t('machine.neverExpires') : formatDate(machine.expiry) }}
                <span v-if="machine.expired" class="text-destructive">
                  {{ t('machine.details.expired') }}
                </span>
              </dd>
            </div>
            <div v-if="magic" class="flex items-center justify-between gap-4">
              <dt class="text-muted-foreground">{{ t('machine.details.domain') }}</dt>
              <dd class="flex items-center gap-1 font-mono text-xs">
                {{ fullDomain() }}
                <CopyButton :value="fullDomain()" label="" />
              </dd>
            </div>
          </dl>

          <div class="space-y-3 text-sm">
            <p class="text-muted-foreground">{{ t('machine.details.addresses') }}</p>
            <div class="flex items-center justify-between gap-4">
              <span>{{ t('machine.ipv4') }}</span>
              <span class="flex items-center gap-1 font-mono text-xs">
                {{ machine.ipv4 ?? '—' }}
                <CopyButton v-if="machine.ipv4" :value="machine.ipv4" label="" />
              </span>
            </div>
            <div v-if="machine.ipv6" class="flex items-center justify-between gap-4">
              <span>{{ t('machine.ipv6') }}</span>
              <span class="flex items-center gap-1 font-mono text-xs">
                {{ machine.ipv6 }}
                <CopyButton :value="machine.ipv6" label="" />
              </span>
            </div>

            <template v-if="machine.endpoints.length">
              <p class="text-muted-foreground pt-2">{{ t('machine.details.endpoints') }}</p>
              <ul class="space-y-0.5 font-mono text-xs">
                <li v-for="endpoint in machine.endpoints" :key="endpoint">{{ endpoint }}</li>
              </ul>
            </template>

            <template v-if="netInfo.length">
              <p class="text-muted-foreground pt-2">{{ t('machine.details.connectivity') }}</p>
              <div
                v-for="[label, value] in netInfo"
                :key="label"
                class="flex items-center justify-between gap-4"
              >
                <span>{{ label }}</span>
                <Badge :variant="value ? 'success' : 'outline'">
                  {{ value ? t('common.yes') : t('common.no') }}
                </Badge>
              </div>
            </template>

            <p v-if="!hostInfo" class="text-muted-foreground pt-2 text-xs">
              {{ t('machine.details.agentHint') }}
            </p>
          </div>
        </CardContent>
      </Card>

      <p class="text-muted-foreground text-xs">
        <template v-if="isNever(machine.lastSeen)">{{ t('machine.neverSeen') }}</template>
        <template v-else>
          {{ t('machine.lastSeenRelative', { value: formatRelative(machine.lastSeen) }) }}
        </template>
      </p>
    </template>
  </div>
</template>
