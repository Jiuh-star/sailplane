<script setup lang="ts">
import type { HTMLAttributes } from 'vue'
import {
  DropdownMenuContent, type DropdownMenuContentEmits, type DropdownMenuContentProps,
  DropdownMenuPortal, useForwardPropsEmits,
} from 'reka-ui'
import { cn } from '@/lib/utils'
const props = withDefaults(defineProps<DropdownMenuContentProps & { class?: HTMLAttributes['class'] }>(), { sideOffset: 4 })
const emits = defineEmits<DropdownMenuContentEmits>()
const forwarded = useForwardPropsEmits(props, emits)
</script>
<template>
  <DropdownMenuPortal>
    <DropdownMenuContent v-bind="forwarded" :class="cn(
      'bg-popover text-popover-foreground data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 z-50 min-w-[8rem] overflow-hidden rounded-md border p-1 shadow-md',
      props.class,
    )"><slot /></DropdownMenuContent>
  </DropdownMenuPortal>
</template>
