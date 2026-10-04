<script setup lang="ts">
import type { HTMLAttributes } from 'vue'
import { cn } from '@/lib/utils'
const props = defineProps<{
  class?: HTMLAttributes['class']
  /**
   * Column name, printed beside the value when the table collapses into cards
   * on a narrow screen. Leave it out on the row's identity cell (the machine
   * or user name) and pass an empty string on action cells.
   */
  label?: string
}>()
</script>
<template>
  <td
    :data-label="props.label"
    :class="cn('p-2 align-middle whitespace-nowrap [&:has([role=checkbox])]:pr-0', props.class)"
  >
    <slot />
  </td>
</template>
