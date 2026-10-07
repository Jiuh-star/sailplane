<script setup lang="ts">
/**
 * Side-by-side diff of the saved policy against the working copy. It uses
 * CodeMirror's merge view. The ACL page lazy-loads it.
 */
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { EditorState, type Extension } from '@codemirror/state'
import { EditorView, lineNumbers } from '@codemirror/view'
import { bracketMatching, defaultHighlightStyle, indentOnInput, syntaxHighlighting } from '@codemirror/language'
import { json } from '@codemirror/lang-json'
import { MergeView } from '@codemirror/merge'
import { oneDark } from '@codemirror/theme-one-dark'

const props = defineProps<{
  /** The policy as Headscale has it. */
  original: string
  /** The policy as the form shows it. */
  modified: string
}>()

const host = ref<HTMLDivElement | null>(null)
let view: MergeView | null = null
let themeObserver: MutationObserver | null = null

function themeExtension(): Extension {
  return document.documentElement.classList.contains('dark')
    ? oneDark
    : syntaxHighlighting(defaultHighlightStyle, { fallback: true })
}

const shared: Extension[] = [json(), lineNumbers(), indentOnInput(), bracketMatching()]

const sizing: Extension = EditorView.theme({
  '&': { fontSize: '0.75rem' },
  '.cm-scroller': {
    fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace',
    maxHeight: '32rem',
  },
})

function build() {
  if (!host.value) return
  const theme = themeExtension()

  view = new MergeView({
    a: {
      doc: props.original,
      extensions: [...shared, theme, sizing, EditorState.readOnly.of(true), EditorView.editable.of(false)],
    },
    b: {
      doc: props.modified,
      extensions: [...shared, theme, sizing, EditorState.readOnly.of(true), EditorView.editable.of(false)],
    },
    parent: host.value,
    highlightChanges: true,
    gutter: true,
    // Collapse long runs of identical lines so the diff stays readable.
    collapseUnchanged: { margin: 3, minSize: 4 },
  })
  host.value.classList.add('hp-diff')
}

onMounted(() => {
  build()
  themeObserver = new MutationObserver(() => {
    view?.destroy()
    view = null
    if (host.value) host.value.innerHTML = ''
    build()
  })
  themeObserver.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['class'],
  })
})

onBeforeUnmount(() => {
  themeObserver?.disconnect()
  themeObserver = null
  view?.destroy()
  view = null
})

// When either side changes, rebuild. MergeView cannot reconfigure its
// document pairs in place.
watch(
  () => [props.original, props.modified],
  () => {
    view?.destroy()
    view = null
    if (host.value) host.value.innerHTML = ''
    build()
  },
)
</script>

<template>
  <div ref="host" class="overflow-hidden rounded-md border" />
</template>

<style>
/* Tint changed and inserted lines in both panes. */
.hp-diff .cm-merge-a .cm-changedLine,
.hp-diff .cm-merge-a .cm-deletedChunk {
  background-color: color-mix(in oklab, var(--destructive) 12%, transparent);
}

.hp-diff .cm-merge-b .cm-changedLine,
.hp-diff .cm-merge-b .cm-insertedChunk {
  background-color: color-mix(in oklab, var(--success) 14%, transparent);
}

.hp-diff .cm-changedText {
  background-color: color-mix(in oklab, var(--warning) 30%, transparent);
}

.hp-diff .cm-mergeSpacer {
  background: var(--muted);
}
</style>
