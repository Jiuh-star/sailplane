<script setup lang="ts">
import { AvatarFallback, type AvatarFallbackProps } from 'reka-ui'
import { cn } from '@/lib/utils'
const props = defineProps<AvatarFallbackProps & { class?: string }>()
</script>
<template>
  <AvatarFallback v-bind="props" :class="cn('bg-muted flex size-full items-center justify-center rounded-full text-xs', props.class)"><slot /></AvatarFallback>
</template>
