<script setup lang="ts">
import type { HTMLAttributes } from 'vue'
import { DropdownMenuSeparator, type DropdownMenuSeparatorProps } from 'reka-ui'
import { cn } from '@/lib/utils'
const props = defineProps<DropdownMenuSeparatorProps & { class?: HTMLAttributes['class'] }>()
</script>
<template><DropdownMenuSeparator :class="cn('bg-border -mx-1 my-1 h-px', props.class)" /></template>
