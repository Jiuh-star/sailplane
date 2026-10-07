<script setup lang="ts">
/**
 * CodeMirror-based JSON/HuJSON editor. The ACL page lazy-loads it, so CodeMirror
 * never reaches another route's bundle.
 */
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { EditorState, type Extension } from '@codemirror/state'
import { EditorView, keymap, lineNumbers, highlightActiveLine } from '@codemirror/view'
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands'
import { bracketMatching, indentOnInput, syntaxHighlighting, defaultHighlightStyle } from '@codemirror/language'
import { closeBrackets, closeBracketsKeymap } from '@codemirror/autocomplete'
import { json } from '@codemirror/lang-json'
import { oneDark } from '@codemirror/theme-one-dark'

const props = withDefaults(
  defineProps<{
    modelValue: string
    readonly?: boolean
    /** Rendered height. The editor scrolls inside it. */
    minHeight?: string
  }>(),
  { readonly: false, minHeight: '28rem' },
)

const emit = defineEmits<{ (e: 'update:modelValue', value: string): void }>()

const host = ref<HTMLDivElement | null>(null)
let view: EditorView | null = null
let themeObserver: MutationObserver | null = null

/** CodeMirror uses the light or dark extension chosen up front. */
function themeExtension(): Extension {
  return document.documentElement.classList.contains('dark')
    ? oneDark
    : syntaxHighlighting(defaultHighlightStyle, { fallback: true })
}

function extensions(): Extension[] {
  const list: Extension[] = [
    lineNumbers(),
    history(),
    indentOnInput(),
    bracketMatching(),
    closeBrackets(),
    highlightActiveLine(),
    json(),
    themeExtension(),
    keymap.of([...closeBracketsKeymap, ...defaultKeymap, ...historyKeymap, indentWithTab]),
    EditorView.updateListener.of((update) => {
      if (update.docChanged) emit('update:modelValue', update.state.doc.toString())
    }),
    EditorView.theme({
      '&': { fontSize: '0.75rem', minHeight: props.minHeight, border: '1px solid var(--border)', borderRadius: 'var(--radius-md)' },
      '.cm-scroller': { fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace', maxHeight: '36rem' },
      '&.cm-focused': { outline: 'none', borderColor: 'var(--ring)' },
    }),
  ]

  if (props.readonly) {
    list.push(EditorState.readOnly.of(true), EditorView.editable.of(false))
  }

  return list
}

onMounted(() => {
  if (!host.value) return
  view = new EditorView({
    state: EditorState.create({ doc: props.modelValue, extensions: extensions() }),
    parent: host.value,
  })

  // When the app toggles dark mode, swap the highlight theme.
  themeObserver = new MutationObserver(rebuild)
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

/** Recreates the view so the theme extension takes effect. */
function rebuild() {
  if (!view || !host.value) return
  const doc = view.state.doc.toString()
  const scrollTop = view.scrollDOM.scrollTop
  view.destroy()
  view = new EditorView({
    state: EditorState.create({ doc, extensions: extensions() }),
    parent: host.value,
  })
  view.scrollDOM.scrollTop = scrollTop
}

// Follow external changes, such as a save rewriting the policy, without
// fighting the user's own typing.
watch(
  () => props.modelValue,
  (next) => {
    if (!view) return
    const current = view.state.doc.toString()
    if (next === current) return
    view.dispatch({
      changes: { from: 0, to: current.length, insert: next },
    })
  },
)
</script>

<template>
  <div ref="host" class="overflow-hidden rounded-md" />
</template>
