<script setup lang="ts">
import type { HTMLAttributes } from 'vue'
import { DropdownMenuItem, type DropdownMenuItemProps, useForwardProps } from 'reka-ui'
import { cn } from '@/lib/utils'
const props = defineProps<DropdownMenuItemProps & { class?: HTMLAttributes['class'], inset?: boolean, variant?: 'default' | 'destructive' }>()
const forwarded = useForwardProps(props)
</script>
<template>
  <DropdownMenuItem v-bind="{ ...forwarded, class: undefined, variant: undefined, inset: undefined }" :class="cn(
    'focus:bg-accent focus:text-accent-foreground relative flex cursor-default items-center gap-2 rounded-sm px-2 py-1.5 text-sm outline-hidden select-none data-[disabled]:pointer-events-none data-[disabled]:opacity-50 [&_svg]:size-4 [&_svg]:shrink-0',
    props.inset && 'pl-8',
    props.variant === 'destructive' && 'text-destructive focus:bg-destructive/10 focus:text-destructive',
    props.class,
  )"><slot /></DropdownMenuItem>
</template>
