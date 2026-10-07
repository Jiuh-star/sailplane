<script setup lang="ts">
/**
 * The tailnet drawn as a graph. Vue Flow owns panning, zooming and routing.
 * This component owns the four-column layout: source selectors, destination
 * selectors, the machines that carry infrastructure, then the networks and
 * relay regions from those machines.
 */
import { computed, ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { VueFlow, type Edge, type Node } from '@vue-flow/core'
import { Background } from '@vue-flow/background'
import { Controls } from '@vue-flow/controls'

import { Badge } from '@/components/ui/badge'
import type { TopologyEdge, TopologyNode } from '@/lib/api'

import TopologyNodeView from './TopologyNode.vue'
import PortEdge from './PortEdge.vue'

const props = defineProps<{
  nodes: TopologyNode[]
  edges: TopologyEdge[]
}>()

const { t } = useI18n()

const COLUMN = 340
const ROW = 112

const selected = ref<string | null>(null)

/** Sources, destinations, machines, then networks — left to right. */
function columnOf(node: TopologyNode): number {
  if (node.kind === 'selector') return node.role === 'destination' ? 1 : 0
  if (node.kind === 'machine') return 2
  return 3
}

const positioned = computed(() => {
  const rows = [0, 0, 0, 0]
  const seen = new Map<string, { x: number; y: number }>()
  for (const node of props.nodes) {
    const column = columnOf(node)
    const row = rows[column]++
    seen.set(node.id, { x: column * COLUMN, y: row * ROW })
  }
  return seen
})

const nodes = computed<Node[]>(() =>
  props.nodes.map((node) => ({
    id: node.id,
    type: 'topo',
    position: positioned.value.get(node.id) ?? { x: 0, y: 0 },
    data: node,
  })),
)

/** Nudges edge labels apart so crossing links do not stack their text. */
const labelOffsets = computed(() => {
  const MIN_GAP = 22
  const items = props.edges.map((edge) => {
    const from = positioned.value.get(edge.source)
    const to = positioned.value.get(edge.target)
    const y = ((from?.y ?? 0) + (to?.y ?? 0)) / 2
    return { id: edge.id, y }
  })

  const deltas = new Map<string, number>()
  const sorted = [...items].sort((a, b) => a.y - b.y)
  let floor = Number.NEGATIVE_INFINITY
  let total = 0

  for (const item of sorted) {
    const y = Math.max(item.y, floor + MIN_GAP)
    floor = y
    deltas.set(item.id, y - item.y)
    total += y - item.y
  }

  // Spread without drifting the whole set downward.
  const drift = sorted.length ? total / sorted.length : 0
  for (const [id, delta] of deltas) deltas.set(id, delta - drift)
  return deltas
})

const edges = computed<Edge[]>(() =>
  props.edges.map((edge) => ({
    id: edge.id,
    source: edge.source,
    target: edge.target,
    type: 'port',
    data: {
      label: edge.label,
      kind: edge.kind,
      labelOffset: labelOffsets.value.get(edge.id) ?? 0,
      // Selecting one link fades the rest, so a single line is easy to follow.
      dimmed: selected.value !== null && selected.value !== edge.id,
    },
  })),
)

const selectedEdge = computed(
  () => props.edges.find((edge) => edge.id === selected.value) ?? null,
)

/** The badge text for an edge kind. */
function edgeKindLabel(kind: TopologyEdge['kind']): string {
  return t(`topology.graph.edge.${kind}`)
}

// Shallow refs: nothing mutates the components, so deep reactivity is wasted work.
const nodeTypes = shallowRef({ topo: TopologyNodeView })
const edgeTypes = shallowRef({ port: PortEdge })
</script>

<template>
  <div class="space-y-3">
    <div class="h-[28rem] overflow-hidden rounded-lg border">
      <VueFlow
        :nodes="nodes"
        :edges="edges"
        :node-types="nodeTypes"
        :edge-types="edgeTypes"
        :min-zoom="0.3"
        :max-zoom="1.6"
        :nodes-draggable="false"
        :nodes-connectable="false"
        :edges-updatable="false"
        fit-view-on-init
        @edge-click="(event) => (selected = selected === event.edge.id ? null : event.edge.id)"
      >
        <Background :gap="18" :size="1.4" />
        <Controls :show-interactive="false" />
      </VueFlow>
    </div>

    <div v-if="selectedEdge" class="bg-muted/40 rounded-lg border p-3 text-sm">
      <div class="flex flex-wrap items-center gap-2">
        <Badge variant="secondary">{{ edgeKindLabel(selectedEdge.kind) }}</Badge>
        <Badge v-if="selectedEdge.action" :variant="selectedEdge.action === 'accept' ? 'success' : 'warning'">
          {{ selectedEdge.action }}
        </Badge>
        <span v-if="selectedEdge.rule !== undefined" class="text-muted-foreground text-xs">
          {{ t('topology.graph.ruleNumber', { index: selectedEdge.rule + 1 }) }}
        </span>
        <Badge v-if="selectedEdge.label" variant="outline">
          {{ t('topology.graph.portLabel', { ports: selectedEdge.label }) }}
        </Badge>
      </div>
      <p class="mt-2 font-mono text-xs">
        {{ selectedEdge.source }}
        <span class="text-muted-foreground mx-2">→</span>
        {{ selectedEdge.target }}
      </p>
      <p
        v-if="selectedEdge.src_machines !== undefined && selectedEdge.dst_machines !== undefined"
        class="text-muted-foreground mt-1 text-xs"
      >
        {{
          t('topology.graph.reach', {
            from: selectedEdge.src_machines,
            to: selectedEdge.dst_machines,
          })
        }}
        <template v-if="selectedEdge.users?.length">
          · {{ t('topology.graph.users', { users: selectedEdge.users.join(', ') }) }}
        </template>
      </p>
    </div>

    <p class="text-muted-foreground text-xs">{{ t('topology.graph.hint') }}</p>
  </div>
</template>

<style>
/* The import goes here rather than in `main.ts`, so only the route that draws a
   graph pays for the library's stylesheet. */
@import '@vue-flow/core/dist/style.css';
@import '@vue-flow/core/dist/theme-default.css';
@import '@vue-flow/controls/dist/style.css';

/* Vue Flow ships a light/dark theme of its own. The app's tokens replace it. */
.vue-flow__controls {
  box-shadow: var(--shadow-sm);
  overflow: hidden;
  border-radius: var(--radius-md);
  border: 1px solid var(--border);
}

.vue-flow__controls-button {
  background: var(--card);
  border-bottom: 1px solid var(--border);
  fill: var(--foreground);
}

.vue-flow__controls-button:hover {
  background: var(--accent);
}

.vue-flow__edge {
  cursor: pointer;
}

.vue-flow__edge-path {
  transition: opacity 0.15s ease;
}

.vue-flow__handle {
  border: none;
}
</style>
