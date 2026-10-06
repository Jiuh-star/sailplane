<script setup lang="ts">
/**
 * A rule as an edge, labelled with the ports it allows. The label is HTML, not
 * SVG text, so it can carry a background over the lines it crosses.
 */
import { computed } from 'vue'
import { BaseEdge, EdgeLabelRenderer, getBezierPath, type EdgeProps } from '@vue-flow/core'

const props = defineProps<
  EdgeProps<{ ports: string; kind: 'acl' | 'grant' | 'ssh'; dimmed: boolean; labelOffset?: number }>
>()

/** Each access syntax gets its own colour and line style. */
const KIND_STYLE = {
  acl: { stroke: 'var(--foreground)', strokeDasharray: undefined },
  ssh: { stroke: 'var(--chart-2)', strokeDasharray: '6 4' },
  grant: { stroke: 'var(--chart-4)', strokeDasharray: '2 3' },
} as const

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
  <BaseEdge
    :id="id"
    :path="geometry.path"
    :marker-end="markerEnd"
    :style="lineStyle"
  />

  <EdgeLabelRenderer>
    <div
      class="bg-background text-muted-foreground pointer-events-none absolute rounded px-1 text-[11px] transition-opacity"
      :style="{
        transform: `translate(-50%, -50%) translate(${geometry.labelX}px, ${geometry.labelY + ($props.data?.labelOffset ?? 0)}px)`,
        opacity: $props.data?.dimmed ? 0.25 : 1,
      }"
    >
      {{ $props.data?.ports }}
    </div>
  </EdgeLabelRenderer>
</template>
