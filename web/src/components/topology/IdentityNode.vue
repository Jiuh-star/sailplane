<script setup lang="ts">
/** A policy identity as a graph node: the selector and what it resolves to. */
import { Handle, Position, type NodeProps } from '@vue-flow/core'
import { useI18n } from 'vue-i18n'

import { cn } from '@/lib/utils'

const { t } = useI18n()

const props = defineProps<
  NodeProps<{ selector: string; kind: string; machines: number; sample: string[] }>
>()

/** Exposes the library's selection state to the template. */
const selected = () => props.selected
</script>

<template>
  <div
    :class="
      cn(
        'bg-card min-w-44 rounded-lg border px-3 py-2 shadow-xs transition-shadow',
        selected() && 'ring-ring ring-2',
      )
    "
  >
    <Handle type="target" :position="Position.Left" class="!bg-border !size-1.5" />

    <p class="truncate font-mono text-sm font-medium">{{ data.selector }}</p>
    <p class="text-muted-foreground text-xs">
      {{ t('topology.graph.machineCount', { count: data.machines }) }}
    </p>

    <Handle type="source" :position="Position.Right" class="!bg-border !size-1.5" />
  </div>
</template>
