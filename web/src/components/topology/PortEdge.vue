<script setup lang="ts">
/**
 * A rule as an edge, labelled with the ports it allows. The label is HTML, not
 * SVG text, so it can carry a background over the lines it crosses.
 */
import { computed } from 'vue'
import { BaseEdge, EdgeLabelRenderer, getBezierPath, type EdgeProps } from '@vue-flow/core'

const props = defineProps<
  EdgeProps<{ ports: string; kind: 'acl' | 'ssh'; dimmed: boolean; labelOffset?: number }>
>()

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
</script>

<template>
  <BaseEdge
    :id="id"
    :path="geometry.path"
    :marker-end="markerEnd"
    :style="{
      stroke: $props.data?.kind === 'ssh' ? 'var(--chart-2)' : 'var(--foreground)',
      strokeWidth: $props.selected ? 3 : 1.5,
      strokeDasharray: $props.data?.kind === 'ssh' ? '6 4' : undefined,
      opacity: $props.data?.dimmed ? 0.15 : 0.85,
    }"
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
