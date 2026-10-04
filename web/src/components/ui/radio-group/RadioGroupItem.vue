<script setup lang="ts">
import { Circle } from '@lucide/vue'
import { RadioGroupIndicator, RadioGroupItem, type RadioGroupItemProps, useForwardProps } from 'reka-ui'
import { cn } from '@/lib/utils'
const props = defineProps<RadioGroupItemProps & { class?: string }>()
const forwarded = useForwardProps(props)
</script>
<template>
  <RadioGroupItem v-bind="forwarded" :class="cn(
    'border-input data-[state=checked]:border-primary aspect-square size-4 shrink-0 rounded-full border shadow-xs outline-none disabled:cursor-not-allowed disabled:opacity-50',
    props.class,
  )">
    <RadioGroupIndicator class="flex items-center justify-center">
      <Circle class="size-2 fill-primary text-primary" />
    </RadioGroupIndicator>
  </RadioGroupItem>
</template>
