<script setup lang="ts">
/**
 * One node in the access graph. A single component handles every kind: a
 * policy selector, a machine, a routed CIDR, the internet behind an exit node,
 * or a relay region. The kinds share the same card and handles. Only the body
 * differs.
 */
import { Handle, Position, type NodeProps } from '@vue-flow/core'
import { useI18n } from 'vue-i18n'

import { Badge } from '@/components/ui/badge'
import StatusCircle from '@/components/shared/StatusCircle.vue'
import type { TopologyNode } from '@/lib/api'
import { cn } from '@/lib/utils'

const { t } = useI18n()

const props = defineProps<NodeProps<TopologyNode>>()

const selected = () => props.selected
</script>

<template>
  <div
    :class="
      cn(
        'bg-card min-w-36 rounded-lg border px-3 py-2 shadow-xs transition-shadow',
        data.kind === 'selector' && 'min-w-44',
        data.kind !== 'selector' && 'font-mono',
        selected() && 'ring-ring ring-2',
      )
    "
  >
    <Handle type="target" :position="Position.Left" class="!bg-border !size-1.5" />

    <!-- A policy selector: the selector text, what it resolves to, and any
         node attributes the policy attaches to it. -->
    <template v-if="data.kind === 'selector'">
      <p class="truncate text-sm font-medium">{{ data.label }}</p>
      <p class="text-muted-foreground text-xs">
        {{ t('topology.graph.machineCount', { count: data.machines ?? 0 }) }}
      </p>
      <div v-if="data.attrs?.length" class="mt-1 flex flex-wrap gap-1">
        <Badge v-for="attr in data.attrs" :key="attr" variant="outline">{{ attr }}</Badge>
      </div>
    </template>

    <!-- A machine that carries infrastructure. -->
    <template v-else-if="data.kind === 'machine'">
      <p class="flex items-center gap-2 text-sm font-medium">
        <StatusCircle :online="data.online ?? false" />
        <span class="truncate">{{ data.label }}</span>
      </p>
    </template>

    <!-- The internet behind an exit node. -->
    <template v-else-if="data.kind === 'exit'">
      <p class="text-sm font-medium">{{ t('topology.graph.internet') }}</p>
    </template>

    <!-- A relay region. -->
    <template v-else-if="data.kind === 'region'">
      <p class="text-muted-foreground text-[11px]">{{ t('topology.graph.relayRegion') }}</p>
      <p class="truncate text-sm font-medium">{{ data.label }}</p>
    </template>

    <!-- A routed CIDR. -->
    <template v-else>
      <p class="truncate text-sm font-medium">{{ data.label }}</p>
      <div class="mt-1 flex flex-wrap gap-1">
        <Badge v-if="data.exit_node" variant="info">{{ t('topology.routes.exitNode') }}</Badge>
        <Badge :variant="data.approved ? 'success' : 'warning'">
          {{ data.approved ? t('topology.routes.approved') : t('topology.routes.pending') }}
        </Badge>
        <Badge v-if="data.sole" variant="destructive">{{ t('topology.routes.sole') }}</Badge>
      </div>
    </template>

    <Handle type="source" :position="Position.Right" class="!bg-border !size-1.5" />
  </div>
</template>
