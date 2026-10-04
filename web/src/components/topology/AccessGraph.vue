<script setup lang="ts">
/**
 * The policy drawn as a graph, with each edge labelled by its rule's ports. Vue
 * Flow owns panning, zooming, and routing; this component owns the two-column
 * layout.
 */
import { computed, ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { VueFlow, type Edge, type Node } from '@vue-flow/core'
import { Background } from '@vue-flow/background'
import { Controls } from '@vue-flow/controls'

import { Badge } from '@/components/ui/badge'
import type { TopologyEdge, TopologyIdentity } from '@/lib/api'

import IdentityNode from './IdentityNode.vue'
import PortEdge from './PortEdge.vue'

const props = defineProps<{
  identities: TopologyIdentity[]
  edges: TopologyEdge[]
}>()

const { t } = useI18n()

const COLUMN = 380
const ROW = 96

const selected = ref<string | null>(null)

const sources = computed(() => [...new Set(props.edges.map((edge) => edge.src))])
const destinations = computed(() => [...new Set(props.edges.map((edge) => edge.dst))])

const identity = (selector: string) =>
  props.identities.find((entry) => entry.selector === selector)

/** A stable id per edge, so selection survives a re-render. */
const edgeId = (edge: TopologyEdge, index: number) => `${edge.kind}-${edge.rule}-${index}`

function node(selector: string, column: 'source' | 'destination', index: number): Node {
  const entry = identity(selector)
  return {
    id: `${column}:${selector}`,
    type: 'identity',
    position: { x: column === 'source' ? 0 : COLUMN, y: index * ROW },
    data: {
      selector,
      kind: entry?.kind ?? 'any',
      machines: entry?.machines ?? 0,
      sample: entry?.sample ?? [],
    },
  }
}

const nodes = computed<Node[]>(() => [
  ...sources.value.map((selector, index) => node(selector, 'source', index)),
  ...destinations.value.map((selector, index) => node(selector, 'destination', index)),
])

const edges = computed<Edge[]>(() =>
  props.edges.map((edge, index) => ({
    id: edgeId(edge, index),
    source: `source:${edge.src}`,
    target: `destination:${edge.dst}`,
    type: 'port',
    data: {
      ports: edge.ports,
      kind: edge.kind,
      labelOffset: labelOffsets.value.get(edgeId(edge, index)) ?? 0,
      // Selecting one rule fades the rest, so a single line can be followed
      // across a busy graph.
      dimmed: selected.value !== null && selected.value !== edgeId(edge, index),
    },
  })),
)

/**
 * Nudges edge labels apart: Vue Flow anchors each label to its own curve
 * midpoint, so crossing rules would stack their port lists. The deltas are
 * relative, so an estimate of the midpoints is enough.
 */
const labelOffsets = computed(() => {
  // Where a handle attaches: the node's vertical centre.
  const CENTRE = 30
  const MIN_GAP = 20

  const items = props.edges.map((edge, index) => {
    const y1 = sources.value.indexOf(edge.src) * ROW + CENTRE
    const y2 = destinations.value.indexOf(edge.dst) * ROW + CENTRE
    return { id: edgeId(edge, index), y: (y1 + y2) / 2 }
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

  // Spread without drifting the whole set downwards.
  const drift = sorted.length ? total / sorted.length : 0
  for (const [id, delta] of deltas) deltas.set(id, delta - drift)
  return deltas
})

const selectedEdge = computed(() => {
  if (!selected.value) return null
  const index = props.edges.findIndex((edge, position) => edgeId(edge, position) === selected.value)
  return index === -1 ? null : props.edges[index]
})

// Shallow refs: the components are never mutated, so deep reactivity is wasted work.
const nodeTypes = shallowRef({ identity: IdentityNode })
const edgeTypes = shallowRef({ port: PortEdge })
</script>

<template>
  <div class="space-y-3">
    <div class="h-[26rem] overflow-hidden rounded-lg border">
      <VueFlow
        :nodes="nodes"
        :edges="edges"
        :node-types="nodeTypes"
        :edge-types="edgeTypes"
        :min-zoom="0.4"
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
        <Badge variant="secondary">{{ selectedEdge.kind.toUpperCase() }}</Badge>
        <Badge :variant="selectedEdge.action === 'accept' ? 'success' : 'warning'">
          {{ selectedEdge.action }}
        </Badge>
        <span class="text-muted-foreground text-xs">
          {{ t('topology.graph.ruleNumber', { index: selectedEdge.rule + 1 }) }}
        </span>
        <Badge variant="outline">
          {{ t('topology.graph.portLabel', { ports: selectedEdge.ports }) }}
        </Badge>
      </div>
      <p class="mt-2">
        <code class="text-xs">{{ selectedEdge.src }}</code>
        <span class="text-muted-foreground mx-2">→</span>
        <code class="text-xs">{{ selectedEdge.dst }}</code>
      </p>
      <p class="text-muted-foreground mt-1 text-xs">
        {{
          t('topology.graph.reach', {
            from: selectedEdge.src_machines,
            to: selectedEdge.dst_machines,
          })
        }}
        <template v-if="selectedEdge.users.length">
          · {{ t('topology.graph.users', { users: selectedEdge.users.join(', ') }) }}
        </template>
      </p>
    </div>

    <p class="text-muted-foreground text-xs">{{ t('topology.graph.hint') }}</p>
  </div>
</template>

<style>
/* Imported here rather than in `main.ts`, so only the route that draws a graph
   pays for the library's stylesheet. */
@import '@vue-flow/core/dist/style.css';
@import '@vue-flow/core/dist/theme-default.css';
@import '@vue-flow/controls/dist/style.css';

/* Vue Flow ships a light/dark theme of its own; the app's tokens replace it. */
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
