<script setup lang="ts">
import { computed, ref } from 'vue'
import { Check, Copy, X } from '@lucide/vue'
import { useI18n } from 'vue-i18n'

import { Button } from '@/components/ui/button'
import { useToast } from '@/composables/useToast'
import { copyText } from '@/lib/format'
import { cn } from '@/lib/utils'

const props = defineProps<{ value: string; label?: string; class?: string }>()

const { t } = useI18n()
const { error } = useToast()
const state = ref<'idle' | 'copied' | 'failed'>('idle')

/** The visible label, or a generic verb when the caller passes `label=""`. */
const displayLabel = computed(() => props.label ?? t('common.copy'))

/**
 * The accessible name, which appends the value. When the caller passes
 * `label=""`, it uses a generic verb.
 */
const accessibleName = computed(() => {
  const verb = (props.label ?? '').trim() || t('common.copy')
  return `${verb}: ${props.value}`
})

async function copy() {
  if (await copyText(props.value)) {
    state.value = 'copied'
    window.setTimeout(() => (state.value = 'idle'), 1500)
    return
  }

  // Stay quiet about *what* failed to copy: the value is often a key.
  state.value = 'failed'
  window.setTimeout(() => (state.value = 'idle'), 2500)
  error(t('common.copyErrorTitle'), t('common.copyErrorBody'))
}
</script>

<template>
  <Button
    variant="ghost"
    size="sm"
    :class="cn('h-6 px-2 text-xs', props.class)"
    :aria-label="accessibleName"
    @click="copy"
  >
    <Check v-if="state === 'copied'" class="text-success" />
    <X v-else-if="state === 'failed'" class="text-destructive" />
    <Copy v-else />
    <span v-if="displayLabel" class="sr-only sm:not-sr-only">
      {{
        state === 'copied'
          ? t('common.copied')
          : state === 'failed'
            ? t('common.copyFailed')
            : displayLabel
      }}
    </span>
  </Button>
</template>
