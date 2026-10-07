<script setup lang="ts">
/**
 * One link in the topology, labeled with what it carries. It can be a port list
 * for a network rule, a capability name, or the state of a route. The label is
 * HTML, not SVG text, so it can carry a background over the lines it crosses.
 */
import { computed } from 'vue'
import { BaseEdge, EdgeLabelRenderer, getBezierPath, type EdgeProps } from '@vue-flow/core'

import type { TopologyEdge } from '@/lib/api'

type EdgeKind = TopologyEdge['kind']

const props = defineProps<
  EdgeProps<{ label: string; kind: EdgeKind; dimmed: boolean; labelOffset?: number }>
>()

/** Each edge kind gets its own color and line style. */
const KIND_STYLE: Record<EdgeKind, { stroke: string; strokeDasharray?: string }> = {
  acl: { stroke: 'var(--foreground)' },
  grant: { stroke: 'var(--chart-4)', strokeDasharray: '2 3' },
  cap: { stroke: 'var(--chart-5)', strokeDasharray: '1 3' },
  ssh: { stroke: 'var(--chart-2)', strokeDasharray: '6 4' },
  route: { stroke: 'var(--chart-3)' },
  exit: { stroke: 'var(--destructive)' },
  relay: { stroke: 'var(--chart-1)', strokeDasharray: '3 3' },
  approval: { stroke: 'var(--muted-foreground)', strokeDasharray: '2 2' },
}

/** Recomputed if the endpoints move, which the layout does on a resize. */
const geometry = computed(() => {
  const [path, labelX, labelY] = getBezierPath({
    sourceX: props.sourceX,
    sourceY: props.sourceY,
    sourcePosition: props.sourcePosition,
    targetX: props.targetX,
    targetY: props.targetY,
    targetPosition: props.targetPosition,
  })
  return { path, labelX, labelY }
})

const lineStyle = computed(() => {
  const base = KIND_STYLE[props.data?.kind ?? 'acl']
  return {
    stroke: base.stroke,
    strokeWidth: props.selected ? 3 : 1.5,
    strokeDasharray: base.strokeDasharray,
    opacity: props.data?.dimmed ? 0.15 : 0.85,
  }
})
</script>

<template>
  <BaseEdge :id="id" :path="geometry.path" :marker-end="markerEnd" :style="lineStyle" />

  <EdgeLabelRenderer>
    <div
      v-if="$props.data?.label"
      class="bg-background text-muted-foreground pointer-events-none absolute rounded px-1 text-[11px] transition-opacity"
      :style="{
        transform: `translate(-50%, -50%) translate(${geometry.labelX}px, ${geometry.labelY + ($props.data?.labelOffset ?? 0)}px)`,
        opacity: $props.data?.dimmed ? 0.25 : 1,
      }"
    >
      {{ $props.data?.label }}
    </div>
  </EdgeLabelRenderer>
</template>
