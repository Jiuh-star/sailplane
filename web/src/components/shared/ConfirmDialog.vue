<script setup lang="ts">
import { computed, ref } from 'vue'
import { LoaderCircle } from '@lucide/vue'
import { useI18n } from 'vue-i18n'

import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'

const props = withDefaults(
  defineProps<{
    title: string
    description?: string
    confirmLabel?: string
    destructive?: boolean
    open: boolean
  }>(),
  { destructive: false },
)

const { t } = useI18n()

const confirmText = computed(() => props.confirmLabel ?? t('common.confirm'))

const emit = defineEmits<{
  (e: 'update:open', value: boolean): void
  (e: 'confirm'): void | Promise<void>
}>()

const busy = ref(false)

async function confirm() {
  busy.value = true
  try {
    await emit('confirm')
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <Dialog :open="props.open" @update:open="emit('update:open', $event)">
    <DialogContent>
      <DialogHeader>
        <DialogTitle>{{ title }}</DialogTitle>
        <DialogDescription v-if="description">{{ description }}</DialogDescription>
      </DialogHeader>

      <slot />

      <DialogFooter>
        <Button variant="outline" :disabled="busy" @click="emit('update:open', false)">
          {{ t('common.cancel') }}
        </Button>
        <Button
          :variant="destructive ? 'destructive' : 'default'"
          :disabled="busy"
          @click="confirm"
        >
          <LoaderCircle v-if="busy" class="animate-spin" />
          {{ confirmText }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
