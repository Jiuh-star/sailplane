<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { RouterLink } from 'vue-router'
import {
  ArrowDown,
  ArrowUp,
  ArrowUpDown,
  ChevronDown,
  Plus,
  Search,
  X,
} from '@lucide/vue'

import MachineActions from '@/components/machines/MachineActions.vue'
import RegisterMachineDialog from '@/components/machines/RegisterMachineDialog.vue'
import EmptyState from '@/components/shared/EmptyState.vue'
import PageHeader from '@/components/shared/PageHeader.vue'
import StatusCircle from '@/components/shared/StatusCircle.vue'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
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
import { useLiveResource } from '@/composables/useLive'
import { errorMessage } from '@/composables/useToast'
import { statusTagLabel } from '@/lib/statusTags'
import { api, type MachineListResponse } from '@/lib/api'
import { formatRelative } from '@/lib/format'
import { cn } from '@/lib/utils'

type SortKey = 'name' | 'ip' | 'version' | 'lastSeen'
type SortDirection = 'asc' | 'desc'

const { t, te } = useI18n()

const data = ref<MachineListResponse | null>(null)
const loading = ref(true)
const failure = ref<string | null>(null)

const search = ref('')
const userFilter = ref<string | null>(null)
const tagFilter = ref<string | null>(null)
const statusFilter = ref<string | null>(null)
const routeFilter = ref<string | null>(null)
const sortKey = ref<SortKey>('name')
const sortDirection = ref<SortDirection>('asc')

const registerOpen = ref(false)

async function load() {
  try {
    data.value = await api.machines.list()
    failure.value = null
  } catch (err) {
    failure.value = errorMessage(err)
  } finally {
    loading.value = false
  }
}

useLiveResource('nodes', load)

const machines = computed(() => data.value?.machines ?? [])
const canWrite = computed(() => data.value?.access.write ?? false)

const tagOptions = computed(() => {
  const tags = new Set<string>()
  for (const machine of machines.value) {
    for (const tag of machine.tags) tags.add(tag)
  }
  return [...tags].sort()
})

const userOptions = computed(() => {
  const options: { value: string; label: string }[] = []
  const hasTagOwned = machines.value.some((machine) => !machine.user)
  for (const user of data.value?.users ?? []) {
    options.push({ value: user.id, label: user.displayName ?? user.name })
  }
  if (hasTagOwned) options.push({ value: '__tags__', label: t('machines.tagOwned') })
  return options
})

const statusLabels = computed<Record<string, string>>(() => ({
  online: t('machines.status.online'),
  offline: t('machines.status.offline'),
  expired: t('machines.status.expired'),
}))

const filtered = computed(() => {
  const needle = search.value.trim().toLowerCase()

  return machines.value.filter((machine) => {
    if (needle) {
      const haystack = [machine.givenName, machine.name, ...machine.ipAddresses]
        .join(' ')
        .toLowerCase()
      if (!haystack.includes(needle)) return false
    }

    if (userFilter.value === '__tags__') {
      if (machine.user) return false
    } else if (userFilter.value) {
      if (machine.user?.id !== userFilter.value) return false
    }

    if (tagFilter.value && !machine.tags.includes(tagFilter.value)) return false

    if (statusFilter.value === 'online' && !machine.online) return false
    if (statusFilter.value === 'offline' && machine.online) return false
    if (statusFilter.value === 'expired' && !machine.expired) return false

    if (routeFilter.value === 'exit') {
      const isExit = [...machine.approvedRoutes, ...machine.availableRoutes].some(
        (route) => route === '0.0.0.0/0' || route === '::/0',
      )
      if (!isExit) return false
    }
    if (routeFilter.value === 'subnet') {
      const hasSubnet = machine.availableRoutes.some(
        (route) => route !== '0.0.0.0/0' && route !== '::/0',
      )
      if (!hasSubnet) return false
    }

    return true
  })
})

const sorted = computed(() => {
  const key = sortKey.value
  const direction = sortDirection.value === 'asc' ? 1 : -1
  const items = [...filtered.value]

  items.sort((a, b) => {
    switch (key) {
      case 'name':
        return a.givenName.localeCompare(b.givenName) * direction
      case 'version': {
        const left = versionParts(a.version)
        const right = versionParts(b.version)
        return compareNumbers(left, right) * direction
      }
      case 'lastSeen': {
        // Online machines always sort first, then by recency.
        if (a.online !== b.online) return a.online ? -1 : 1
        const left = Date.parse(a.lastSeen) || 0
        const right = Date.parse(b.lastSeen) || 0
        return (left - right) * direction
      }
      case 'ip': {
        const left = ipValue(a.ipv4)
        const right = ipValue(b.ipv4)
        return (left - right) * direction
      }
      default:
        return 0
    }
  })

  return items
})

function versionParts(version: string | null): number[] {
  if (!version) return [0]
  return version
    .replace(/^v/, '')
    .split(/[.-]/)
    .map((part) => Number.parseInt(part, 10) || 0)
}

function compareNumbers(a: number[], b: number[]): number {
  for (let index = 0; index < Math.max(a.length, b.length); index += 1) {
    const left = a[index] ?? 0
    const right = b[index] ?? 0
    if (left !== right) return left - right
  }
  return 0
}

function ipValue(address: string | null): number {
  if (!address) return Number.MAX_SAFE_INTEGER
  return address
    .split('.')
    .reduce((accumulator, part) => accumulator * 256 + (Number.parseInt(part, 10) || 0), 0)
}

function toggleSort(key: SortKey) {
  if (sortKey.value === key) {
    sortDirection.value = sortDirection.value === 'asc' ? 'desc' : 'asc'
  } else {
    sortKey.value = key
    sortDirection.value = 'asc'
  }
}

const filtersActive = computed(
  () => Boolean(userFilter.value || tagFilter.value || statusFilter.value || routeFilter.value),
)

function clearFilters() {
  userFilter.value = null
  tagFilter.value = null
  statusFilter.value = null
  routeFilter.value = null
}

const showAgentColumn = computed(() => data.value?.agent.enabled ?? false)
const columnCount = computed(() => (showAgentColumn.value ? 6 : 5))
</script>

<template>
  <div class="space-y-6">
    <PageHeader
      :title="t('machines.title')"
      :description="t('machines.description')"
    >
      <template #actions>
        <Button
          :disabled="!canWrite"
          :title="canWrite ? undefined : t('machines.noRegisterPermission')"
          @click="registerOpen = true"
        >
          <Plus />
          {{ t('machines.addDevice') }}
        </Button>
      </template>
    </PageHeader>

    <div class="flex flex-wrap items-center gap-2">
      <div class="relative min-w-56 flex-1">
        <Search class="text-muted-foreground absolute top-2.5 left-2.5 size-4" />
        <Input
          v-model="search"
          class="pl-8"
          :placeholder="t('machines.searchPlaceholder')"
          maxlength="100"
          :aria-label="t('machines.searchLabel')"
        />
        <button
          v-if="search"
          class="text-muted-foreground hover:text-foreground absolute top-2.5 right-2.5"
          :aria-label="t('machines.clearSearch')"
          @click="search = ''"
        >
          <X class="size-4" />
        </button>
      </div>

      <DropdownMenu>
        <DropdownMenuTrigger as-child>
          <Button variant="outline" :class="cn(userFilter && 'border-primary')">
            {{ userFilter ? (userOptions.find((o) => o.value === userFilter)?.label ?? t('machines.filter.user')) : t('machines.filter.user') }}
            <ChevronDown />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start">
          <DropdownMenuItem @select="userFilter = null">{{ t('machines.filter.allUsers') }}</DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem
            v-for="option in userOptions"
            :key="option.value"
            @select="userFilter = option.value"
          >
            {{ option.label }}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      <DropdownMenu v-if="tagOptions.length">
        <DropdownMenuTrigger as-child>
          <Button variant="outline" :class="cn(tagFilter && 'border-primary')">
            {{ tagFilter ?? t('machines.filter.tag') }}
            <ChevronDown />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start">
          <DropdownMenuItem @select="tagFilter = null">{{ t('machines.filter.allTags') }}</DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem v-for="tag in tagOptions" :key="tag" @select="tagFilter = tag">
            {{ tag }}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      <DropdownMenu>
        <DropdownMenuTrigger as-child>
          <Button variant="outline" :class="cn(statusFilter && 'border-primary')">
            {{ statusFilter ? (statusLabels[statusFilter] ?? t('machines.filter.status')) : t('machines.filter.status') }}
            <ChevronDown />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start">
          <DropdownMenuItem @select="statusFilter = null">{{ t('machines.filter.anyStatus') }}</DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem @select="statusFilter = 'online'">{{ t('machines.status.online') }}</DropdownMenuItem>
          <DropdownMenuItem @select="statusFilter = 'offline'">{{ t('machines.status.offline') }}</DropdownMenuItem>
          <DropdownMenuItem @select="statusFilter = 'expired'">{{ t('machines.status.expired') }}</DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      <DropdownMenu>
        <DropdownMenuTrigger as-child>
          <Button variant="outline" :class="cn(routeFilter && 'border-primary')">
            {{ routeFilter === 'exit' ? t('machines.filter.exitNodes') : routeFilter === 'subnet' ? t('machines.filter.subnets') : t('machines.filter.routes') }}
            <ChevronDown />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start">
          <DropdownMenuItem @select="routeFilter = null">{{ t('machines.filter.anyRoute') }}</DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem @select="routeFilter = 'exit'">{{ t('machines.filter.exitNodes') }}</DropdownMenuItem>
          <DropdownMenuItem @select="routeFilter = 'subnet'">{{ t('machines.filter.subnetRouters') }}</DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      <Button v-if="filtersActive" variant="ghost" size="sm" @click="clearFilters">
        {{ t('machines.clearFilters') }}
      </Button>
    </div>

    <p class="text-muted-foreground text-sm">
      <template v-if="search || filtersActive">
        {{ t('machines.showing', { shown: sorted.length, total: machines.length }) }}
      </template>
      <template v-else>{{ t('machines.count', { count: machines.length }) }}</template>
    </p>

    <div v-if="loading" class="space-y-2">
      <Skeleton v-for="index in 4" :key="index" class="h-12 w-full" />
    </div>

    <div v-else class="rounded-lg border">
      <Table class="hp-cards">
        <TableHeader>
          <TableRow>
            <TableHead>
              <button class="hover:text-foreground inline-flex items-center gap-1" @click="toggleSort('name')">
                {{ t('machines.table.name') }}
                <ArrowUp v-if="sortKey === 'name' && sortDirection === 'asc'" class="size-3" />
                <ArrowDown v-else-if="sortKey === 'name'" class="size-3" />
                <ArrowUpDown v-else class="size-3 opacity-40" />
              </button>
            </TableHead>
            <TableHead>
              <button class="hover:text-foreground inline-flex items-center gap-1" @click="toggleSort('ip')">
                {{ t('machines.table.addresses') }}
                <ArrowUp v-if="sortKey === 'ip' && sortDirection === 'asc'" class="size-3" />
                <ArrowDown v-else-if="sortKey === 'ip'" class="size-3" />
                <ArrowUpDown v-else class="size-3 opacity-40" />
              </button>
            </TableHead>
            <TableHead v-if="showAgentColumn">
              <button class="hover:text-foreground inline-flex items-center gap-1" @click="toggleSort('version')">
                {{ t('machines.table.version') }}
                <ArrowUp v-if="sortKey === 'version' && sortDirection === 'asc'" class="size-3" />
                <ArrowDown v-else-if="sortKey === 'version'" class="size-3" />
                <ArrowUpDown v-else class="size-3 opacity-40" />
              </button>
            </TableHead>
            <TableHead>
              <button class="hover:text-foreground inline-flex items-center gap-1" @click="toggleSort('lastSeen')">
                {{ t('machines.table.lastSeen') }}
                <ArrowUp v-if="sortKey === 'lastSeen' && sortDirection === 'asc'" class="size-3" />
                <ArrowDown v-else-if="sortKey === 'lastSeen'" class="size-3" />
                <ArrowUpDown v-else class="size-3 opacity-40" />
              </button>
            </TableHead>
            <TableHead><span class="sr-only">{{ t('machines.table.actions') }}</span></TableHead>
          </TableRow>
        </TableHeader>

        <TableBody>
          <TableEmpty v-if="!sorted.length" :colspan="columnCount">
            <EmptyState
              :title="t('machines.empty.title')"
              :description="t('machines.empty.description')"
            />
          </TableEmpty>

          <TableRow v-for="machine in sorted" :key="machine.id">
            <TableCell>
              <div class="flex flex-col gap-1">
                <RouterLink
                  :to="{ name: 'machine', params: { id: machine.id } }"
                  class="font-medium hover:underline"
                >
                  {{ machine.givenName }}
                </RouterLink>
                <div class="flex flex-wrap items-center gap-1">
                  <span class="text-muted-foreground text-xs">
                    {{ machine.user?.displayName ?? machine.user?.name ?? t('machines.tagOwned') }}
                  </span>
                  <!-- Route, expiry and SSH state, the same badges the detail page shows. -->
                  <Badge
                    v-for="tag in machine.status_tags"
                    :key="tag.key ?? tag.label"
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
                  <Badge v-for="tag in machine.tags" :key="tag" variant="secondary">
                    {{ tag }}
                  </Badge>
                </div>
              </div>
            </TableCell>

            <TableCell :label="t('machines.table.addresses')">
              <div class="flex flex-col gap-0.5 font-mono text-xs">
                <span>{{ machine.ipv4 ?? '—' }}</span>
                <span v-if="machine.ipv6" class="text-muted-foreground">{{ machine.ipv6 }}</span>
              </div>
            </TableCell>

            <TableCell v-if="showAgentColumn" :label="t('machines.table.version')">
              <div class="flex flex-col gap-0.5 text-xs">
                <span>{{ machine.version ?? t('common.unknown') }}</span>
                <span class="text-muted-foreground">{{ machine.os ?? '—' }}</span>
              </div>
            </TableCell>

            <TableCell :label="t('machines.table.lastSeen')">
              <div class="flex items-center gap-2">
                <StatusCircle :online="machine.online" :expired="machine.expired" />
                <div class="flex flex-col">
                  <span class="text-xs">
                    {{
                      machine.online
                        ? t('machines.status.connected')
                        : formatRelative(machine.lastSeen, t('common.never'))
                    }}
                  </span>
                  <span v-if="!machine.online" class="text-muted-foreground text-xs">
                    {{ formatRelative(machine.lastSeen, '') }}
                  </span>
                </div>
              </div>
            </TableCell>

            <TableCell class="text-right" label="">
              <MachineActions
                :machine="machine"
                :users="data?.users ?? []"
                :existing-tags="tagOptions"
                :policy-tags="data?.policyTags ?? []"
                :supports="data?.supports ?? { nodeOwnerChange: false, disablingKeyExpiry: false }"
                :can-write="canWrite"
                @changed="load"
              />
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
    </div>

    <RegisterMachineDialog
      v-model:open="registerOpen"
      :users="data?.users ?? []"
      :server="data?.server ?? ''"
      @registered="load"
    />
  </div>
</template>
