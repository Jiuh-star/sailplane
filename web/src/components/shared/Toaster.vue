<script setup lang="ts">
import { CircleCheck, CircleX, Info, TriangleAlert, X } from '@lucide/vue'
import { useI18n } from 'vue-i18n'

import { useToast } from '@/composables/useToast'
import { cn } from '@/lib/utils'

const { t } = useI18n()
const { toasts, dismiss } = useToast()
</script>

<template>
  <div
    class="pointer-events-none fixed inset-x-0 bottom-0 z-100 flex flex-col items-center gap-2 p-4 sm:items-end"
    aria-live="polite"
  >
    <TransitionGroup
      enter-active-class="transition duration-200 ease-out"
      enter-from-class="translate-y-2 opacity-0"
      leave-active-class="transition duration-150 ease-in"
      leave-to-class="translate-y-2 opacity-0"
    >
      <div
        v-for="toast in toasts"
        :key="toast.id"
        :class="
          cn(
            'bg-popover text-popover-foreground pointer-events-auto flex w-full max-w-sm items-start gap-3 rounded-lg border p-4 shadow-lg',
            toast.variant === 'destructive' && 'border-destructive/40',
            toast.variant === 'success' && 'border-success/40',
            toast.variant === 'warning' && 'border-warning/50',
          )
        "
        role="status"
      >
        <CircleCheck v-if="toast.variant === 'success'" class="text-success mt-0.5 size-4 shrink-0" />
        <CircleX
          v-else-if="toast.variant === 'destructive'"
          class="text-destructive mt-0.5 size-4 shrink-0"
        />
        <TriangleAlert
          v-else-if="toast.variant === 'warning'"
          class="text-warning mt-0.5 size-4 shrink-0"
        />
        <Info v-else class="text-muted-foreground mt-0.5 size-4 shrink-0" />

        <div class="flex-1 space-y-1">
          <p class="text-sm font-medium">{{ toast.title }}</p>
          <p v-if="toast.description" class="text-muted-foreground text-sm">{{ toast.description }}</p>
        </div>

        <button
          class="text-muted-foreground hover:text-foreground transition-colors"
          :aria-label="t('common.toaster.dismiss')"
          @click="dismiss(toast.id)"
        >
          <X class="size-4" />
        </button>
      </div>
    </TransitionGroup>
  </div>
</template>
