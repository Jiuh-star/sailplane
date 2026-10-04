<script setup lang="ts">
import type { HTMLAttributes } from 'vue'
import { TooltipContent, type TooltipContentEmits, type TooltipContentProps, TooltipPortal, useForwardPropsEmits } from 'reka-ui'
import { cn } from '@/lib/utils'
const props = withDefaults(defineProps<TooltipContentProps & { class?: HTMLAttributes['class'] }>(), { sideOffset: 4 })
const emits = defineEmits<TooltipContentEmits>()
const forwarded = useForwardPropsEmits(props, emits)
</script>
<template>
  <TooltipPortal>
    <TooltipContent v-bind="forwarded" :class="cn('bg-primary text-primary-foreground animate-in fade-in-0 zoom-in-95 z-50 w-fit rounded-md px-3 py-1.5 text-xs', props.class)">
      <slot />
    </TooltipContent>
  </TooltipPortal>
</template>
