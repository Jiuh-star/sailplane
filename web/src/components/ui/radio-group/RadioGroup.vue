<script setup lang="ts">
import { RadioGroupRoot, type RadioGroupRootEmits, type RadioGroupRootProps, useForwardPropsEmits } from 'reka-ui'
import { cn } from '@/lib/utils'
const props = defineProps<RadioGroupRootProps & { class?: string }>()
const emits = defineEmits<RadioGroupRootEmits>()
const forwarded = useForwardPropsEmits(props, emits)
</script>
<template><RadioGroupRoot v-bind="forwarded" :class="cn('grid gap-3', props.class)"><slot /></RadioGroupRoot></template>
