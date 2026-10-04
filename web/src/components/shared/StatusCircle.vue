<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

import { cn } from '@/lib/utils'

const { t } = useI18n()

const props = withDefaults(
  defineProps<{
    online: boolean
    expired?: boolean
    size?: 'sm' | 'md'
  }>(),
  { expired: false, size: 'sm' },
)

const tone = computed(() => {
  if (props.expired) return 'bg-muted-foreground/40'
  return props.online ? 'bg-success' : 'bg-muted-foreground/40'
})
</script>

<template>
  <span
    :class="
      cn(
        'inline-block shrink-0 rounded-full',
        size === 'sm' ? 'size-2' : 'size-2.5',
        tone,
        online && !expired && 'ring-2 ring-success/30',
      )
    "
    :title="
      expired
        ? t('common.status.expired')
        : online
          ? t('common.status.connected')
          : t('common.status.offline')
    "
  />
</template>
