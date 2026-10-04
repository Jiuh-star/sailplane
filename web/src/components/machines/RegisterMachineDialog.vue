<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { LoaderCircle, Terminal } from '@lucide/vue'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { errorMessage, useToast } from '@/composables/useToast'
import { api, type HeadscaleUser } from '@/lib/api'

const props = defineProps<{
  open: boolean
  users: HeadscaleUser[]
  server: string
}>()

const emit = defineEmits<{
  (e: 'update:open', value: boolean): void
  (e: 'registered'): void
}>()

const router = useRouter()
const toast = useToast()
const { t } = useI18n()

const registerKey = ref('')
const owner = ref('')
const busy = ref(false)
const failure = ref<string | null>(null)

const command = computed(() => `tailscale up --login-server=${props.server}`)

async function submit() {
  if (!registerKey.value.trim() || !owner.value) return

  busy.value = true
  failure.value = null
  try {
    const result = await api.machines.register(registerKey.value.trim(), owner.value)
    toast.success(t('machines.toast.registered'), result.machine.givenName)
    emit('registered')
    emit('update:open', false)
    registerKey.value = ''
    await router.push({ name: 'machine', params: { id: result.machine.id } })
  } catch (err) {
    failure.value = errorMessage(err)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <Dialog :open="props.open" @update:open="emit('update:open', $event)">
    <DialogContent>
      <DialogHeader>
        <DialogTitle>{{ t('machines.register.title') }}</DialogTitle>
        <DialogDescription>
          {{ t('machines.register.description') }}
        </DialogDescription>
      </DialogHeader>

      <div class="space-y-4">
        <div class="space-y-2">
          <Label>{{ t('machines.register.step1') }}</Label>
          <div class="bg-muted flex items-start gap-2 rounded-md p-3 font-mono text-xs">
            <Terminal class="mt-0.5 size-4 shrink-0" />
            <code class="break-all">{{ command }}</code>
          </div>
        </div>

        <div class="space-y-2">
          <Label for="register-key">{{ t('machines.register.step2') }}</Label>
          <Input
            id="register-key"
            v-model="registerKey"
            placeholder="hskey-authreq-…"
            autocomplete="off"
            spellcheck="false"
          />
        </div>

        <div class="space-y-2">
          <Label for="register-owner">{{ t('machines.owner.label') }}</Label>
          <Select v-model="owner">
            <SelectTrigger id="register-owner">
              <SelectValue :placeholder="t('machines.owner.select')" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem v-for="user in users" :key="user.id" :value="user.name">
                {{ user.displayName ?? user.name }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>

        <Alert v-if="failure" variant="destructive">
          <AlertTitle>{{ t('machines.register.failed') }}</AlertTitle>
          <AlertDescription>{{ failure }}</AlertDescription>
        </Alert>
      </div>

      <DialogFooter>
        <Button variant="outline" @click="emit('update:open', false)">{{ t('common.cancel') }}</Button>
        <Button :disabled="busy || !registerKey.trim() || !owner" @click="submit">
          <LoaderCircle v-if="busy" class="animate-spin" />
          {{ t('machines.register.submit') }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
