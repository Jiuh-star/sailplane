/**
 * Minimal toast queue. A small reactive list rendered by `Toaster.vue` avoids a
 * UI kit dependency.
 */

import { readonly, ref } from 'vue'

export interface Toast {
  id: number
  title: string
  description?: string
  variant: 'default' | 'success' | 'warning' | 'destructive'
}

const toasts = ref<Toast[]>([])
let nextId = 1

function push(toast: Omit<Toast, 'id'>, timeout = 4000) {
  const id = nextId++
  toasts.value = [...toasts.value, { ...toast, id }]
  if (timeout > 0) {
    window.setTimeout(() => dismiss(id), timeout)
  }
  return id
}

function dismiss(id: number) {
  toasts.value = toasts.value.filter((toast) => toast.id !== id)
}

export function useToast() {
  return {
    toasts: readonly(toasts),
    dismiss,
    toast: (title: string, description?: string) => push({ title, description, variant: 'default' }),
    success: (title: string, description?: string) =>
      push({ title, description, variant: 'success' }),
    warning: (title: string, description?: string) =>
      push({ title, description, variant: 'warning' }, 8000),
    error: (title: string, description?: string) =>
      push({ title, description, variant: 'destructive' }, 7000),
  }
}

/** Normalizes anything thrown into a displayable message. */
export function errorMessage(err: unknown): string {
  if (err instanceof Error) return err.message
  if (typeof err === 'string') return err
  return 'Something went wrong'
}
