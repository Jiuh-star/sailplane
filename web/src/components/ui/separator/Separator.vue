<script setup lang="ts">
import type { HTMLAttributes } from 'vue'
import { Separator, type SeparatorProps } from 'reka-ui'
import { cn } from '@/lib/utils'
const props = defineProps<SeparatorProps & { class?: HTMLAttributes['class'] }>()
</script>
<template>
  <Separator :orientation="props.orientation" :decorative="props.decorative"
    :class="cn('bg-border shrink-0 data-[orientation=horizontal]:h-px data-[orientation=horizontal]:w-full data-[orientation=vertical]:h-full data-[orientation=vertical]:w-px', props.class)" />
</template>
