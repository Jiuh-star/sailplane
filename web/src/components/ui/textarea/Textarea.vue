<script setup lang="ts">
import type { HTMLAttributes } from 'vue'
import { useVModel } from '@vueuse/core'
import { cn } from '@/lib/utils'
const props = defineProps<{ defaultValue?: string, modelValue?: string, class?: HTMLAttributes['class'] }>()
const emits = defineEmits<{ (e: 'update:modelValue', payload: string): void }>()
const modelValue = useVModel(props, 'modelValue', emits, { passive: true, defaultValue: props.defaultValue })
</script>
<template>
  <textarea v-model="modelValue" :class="cn(
    'border-input placeholder:text-muted-foreground flex min-h-16 w-full rounded-md border bg-transparent px-3 py-2 text-base shadow-xs outline-none disabled:cursor-not-allowed disabled:opacity-50 md:text-sm',
    'focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-[3px]',
    props.class,
  )" />
</template>
