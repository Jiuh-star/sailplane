<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { Ellipsis, LoaderCircle, Pencil, Route, ShieldOff, Tags, Trash2, UserCog } from '@lucide/vue'

import ConfirmDialog from '@/components/shared/ConfirmDialog.vue'
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
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { errorMessage, useToast } from '@/composables/useToast'
import { api, type HeadscaleUser, type Machine } from '@/lib/api'
import { formatDate } from '@/lib/format'

const props = defineProps<{
  machine: Machine
  users: HeadscaleUser[]
  existingTags: string[]
  policyTags: string[]
  supports: { nodeOwnerChange: boolean; disablingKeyExpiry: boolean }
  canWrite: boolean
}>()

const emit = defineEmits<{ (e: 'changed'): void }>()

const toast = useToast()
const { t } = useI18n()

const renameOpen = ref(false)
const deleteOpen = ref(false)
const expireOpen = ref(false)
const routesOpen = ref(false)
const tagsOpen = ref(false)
const ownerOpen = ref(false)
const busy = ref(false)

const renameValue = ref('')
const ownerValue = ref('')
const tagsValue = ref<string[]>([])
const tagInput = ref('')

const isExitNode = computed(() =>
  [...props.machine.approvedRoutes, ...props.machine.availableRoutes].some(
    (route) => route === '0.0.0.0/0' || route === '::/0',
  ),
)

const subnetRoutes = computed(() =>
  props.machine.availableRoutes.filter((route) => route !== '0.0.0.0/0' && route !== '::/0'),
)

const exitRoutes = computed(() =>
  props.machine.availableRoutes.filter((route) => route === '0.0.0.0/0' || route === '::/0'),
)

const undeclaredTags = computed(() =>
  tagsValue.value.filter((tag) => !props.policyTags.includes(tag)),
)

const tagSuggestions = computed(() =>
  props.existingTags.filter((tag) => !tagsValue.value.includes(tag)),
)

function openRename() {
  renameValue.value = props.machine.givenName
  renameOpen.value = true
}

function openOwner() {
  ownerValue.value = props.machine.user?.id ?? ''
  ownerOpen.value = true
}

function openTags() {
  tagsValue.value = [...props.machine.tags]
  tagInput.value = ''
  tagsOpen.value = true
}

/** Runs an action, surfacing failures as a toast and refreshing on success. */
async function run(action: () => Promise<unknown>, successMessage: string) {
  busy.value = true
  try {
    await action()
    toast.success(successMessage)
    emit('changed')
    return true
  } catch (err) {
    toast.error(t('common.actionFailed'), errorMessage(err))
    return false
  } finally {
    busy.value = false
  }
}

async function submitRename() {
  const name = renameValue.value.trim()
  if (!name) return
  if (await run(() => api.machines.rename(props.machine.id, name), t('machines.toast.renamed'))) {
    renameOpen.value = false
  }
}

async function submitDelete() {
  if (await run(() => api.machines.remove(props.machine.id), t('machines.toast.removed'))) {
    deleteOpen.value = false
  }
}

async function submitExpire() {
  if (await run(() => api.machines.expire(props.machine.id), t('machines.toast.keyExpired'))) {
    expireOpen.value = false
  }
}

async function submitOwner() {
  if (!ownerValue.value) return
  const owner = props.users.find((user) => user.id === ownerValue.value)
  if (!owner) return
  if (
    await run(
      () => api.machines.setOwner(props.machine.id, owner.name),
      t('machines.toast.ownershipChanged'),
    )
  ) {
    ownerOpen.value = false
  }
}

async function toggleExpiry(disable: boolean) {
  await run(
    () => api.machines.setExpiry(props.machine.id, disable),
    disable ? t('machines.toast.expiryDisabled') : t('machines.toast.expiryEnabled'),
  )
}

async function setRoute(route: string, enabled: boolean) {
  await run(
    () => api.machines.setRoute(props.machine.id, route, enabled),
    t('machines.toast.routeUpdated'),
  )
}

function addTag(raw: string) {
  const tag = raw.trim().startsWith('tag:') ? raw.trim() : `tag:${raw.trim()}`
  if (tag === 'tag:' || tagsValue.value.includes(tag)) return
  tagsValue.value = [...tagsValue.value, tag]
  tagInput.value = ''
}

function removeTag(tag: string) {
  tagsValue.value = tagsValue.value.filter((value) => value !== tag)
}

async function submitTags() {
  const previous = props.machine.tags
  const unchanged =
    previous.length === tagsValue.value.length &&
    previous.every((tag) => tagsValue.value.includes(tag))
  if (unchanged) {
    tagsOpen.value = false
    return
  }
  if (await run(() => api.machines.setTags(props.machine.id, tagsValue.value), t('machines.toast.tagsUpdated'))) {
    tagsOpen.value = false
  }
}
</script>

<template>
  <DropdownMenu>
    <DropdownMenuTrigger as-child>
      <Button
        variant="ghost"
        size="icon"
        :disabled="!canWrite || busy"
        :title="canWrite ? t('machines.actionsLabel') : t('machines.actionsNoPermission')"
        :aria-label="t('machines.actionsLabel')"
      >
        <LoaderCircle v-if="busy" class="animate-spin" />
        <Ellipsis v-else />
      </Button>
    </DropdownMenuTrigger>

    <DropdownMenuContent align="end" class="w-56">
      <DropdownMenuItem @select="openRename">
        <Pencil />
        {{ t('machines.menu.rename') }}
      </DropdownMenuItem>

      <DropdownMenuItem
        v-if="supports.disablingKeyExpiry"
        :disabled="machine.expired"
        @select="toggleExpiry(!machine.expiry_disabled)"
      >
        <ShieldOff />
        {{ machine.expiry_disabled ? t('machines.menu.enableExpiry') : t('machines.menu.disableExpiry') }}
      </DropdownMenuItem>

      <DropdownMenuItem @select="routesOpen = true">
        <Route />
        {{ t('machines.menu.routes') }}
      </DropdownMenuItem>
      <DropdownMenuItem @select="openTags">
        <Tags />
        {{ t('machines.menu.tags') }}
      </DropdownMenuItem>
      <DropdownMenuItem v-if="supports.nodeOwnerChange" @select="openOwner">
        <UserCog />
        {{ t('machines.menu.owner') }}
      </DropdownMenuItem>

      <DropdownMenuSeparator />

      <DropdownMenuItem
        variant="destructive"
        :disabled="machine.expired || machine.expiry_disabled"
        @select="expireOpen = true"
      >
        <ShieldOff />
        {{ t('machines.menu.expire') }}
      </DropdownMenuItem>
      <DropdownMenuItem variant="destructive" @select="deleteOpen = true">
        <Trash2 />
        {{ t('common.remove') }}
      </DropdownMenuItem>
    </DropdownMenuContent>
  </DropdownMenu>

  <Dialog v-model:open="renameOpen">
    <DialogContent>
      <DialogHeader>
        <DialogTitle>{{ t('machines.rename.title') }}</DialogTitle>
        <DialogDescription>
          {{ t('machines.rename.description') }}
        </DialogDescription>
      </DialogHeader>

      <form class="space-y-4" @submit.prevent="submitRename">
        <div class="space-y-2">
          <Label for="machine-name">{{ t('machines.rename.name') }}</Label>
          <Input
            id="machine-name"
            v-model="renameValue"
            pattern="[a-z0-9]([a-z0-9-]*[a-z0-9])?"
            autocomplete="off"
          />
          <p class="text-muted-foreground text-xs">
            {{ t('machines.rename.hint') }}
          </p>
        </div>
      </form>

      <DialogFooter>
        <Button variant="outline" @click="renameOpen = false">{{ t('common.cancel') }}</Button>
        <Button :disabled="busy || !renameValue.trim()" @click="submitRename">{{ t('common.save') }}</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>

  <Dialog v-model:open="routesOpen">
    <DialogContent>
      <DialogHeader>
        <DialogTitle>{{ t('machines.routes.title') }}</DialogTitle>
        <DialogDescription>
          {{ t('machines.routes.description') }}
        </DialogDescription>
      </DialogHeader>

      <div class="space-y-4">
        <div class="space-y-2">
          <p class="text-sm font-medium">{{ t('machines.routes.subnetRoutes') }}</p>
          <p v-if="!subnetRoutes.length" class="text-muted-foreground text-sm">
            {{ t('machines.routes.noSubnets') }}
          </p>
          <div
            v-for="route in subnetRoutes"
            :key="route"
            class="flex items-center justify-between rounded-md border p-3"
          >
            <span class="font-mono text-sm">{{ route }}</span>
            <Switch
              :model-value="machine.approvedRoutes.includes(route)"
              :disabled="busy"
              @update:model-value="setRoute(route, $event)"
            />
          </div>
        </div>

        <div class="space-y-2">
          <p class="text-sm font-medium">{{ t('machines.routes.exitNode') }}</p>
          <p v-if="!isExitNode" class="text-muted-foreground text-sm">
            {{ t('machines.routes.notExitNode') }}
          </p>
          <div
            v-for="route in exitRoutes"
            :key="route"
            class="flex items-center justify-between rounded-md border p-3"
          >
            <span class="font-mono text-sm">{{ route }}</span>
            <Switch
              :model-value="machine.approvedRoutes.includes(route)"
              :disabled="busy"
              @update:model-value="setRoute(route, $event)"
            />
          </div>
        </div>
      </div>

      <DialogFooter>
        <Button variant="outline" @click="routesOpen = false">{{ t('common.done') }}</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>

  <Dialog v-model:open="tagsOpen">
    <DialogContent>
      <DialogHeader>
        <DialogTitle>{{ t('machines.tags.title') }}</DialogTitle>
        <DialogDescription>
          {{ t('machines.tags.description') }}
        </DialogDescription>
      </DialogHeader>

      <div class="space-y-4">
        <div v-if="tagsValue.length" class="flex flex-wrap gap-1">
          <button
            v-for="tag in tagsValue"
            :key="tag"
            class="bg-secondary hover:bg-destructive/20 inline-flex items-center gap-1 rounded-md px-2 py-1 text-xs"
            :disabled="busy"
            @click="removeTag(tag)"
          >
            {{ tag }} ×
          </button>
        </div>
        <p v-else class="text-muted-foreground text-sm">{{ t('machines.tags.empty') }}</p>

        <form class="flex gap-2" @submit.prevent="addTag(tagInput)">
          <Input v-model="tagInput" placeholder="tag:server" autocomplete="off" />
          <Button type="submit" variant="outline" :disabled="!tagInput.trim()">
            {{ t('common.add') }}
          </Button>
        </form>

        <div v-if="tagSuggestions.length" class="flex flex-wrap gap-1">
          <button
            v-for="tag in tagSuggestions"
            :key="tag"
            class="text-muted-foreground hover:bg-accent rounded-md border px-2 py-1 text-xs"
            @click="addTag(tag)"
          >
            + {{ tag }}
          </button>
        </div>

        <Alert v-if="undeclaredTags.length" variant="warning">
          <AlertTitle>{{ t('machines.tags.undeclaredTitle') }}</AlertTitle>
          <AlertDescription>
            {{
              t('machines.tags.undeclaredBody', {
                count: undeclaredTags.length,
                tags: undeclaredTags.join(', '),
              })
            }}
          </AlertDescription>
        </Alert>
      </div>

      <DialogFooter>
        <Button variant="outline" @click="tagsOpen = false">{{ t('common.cancel') }}</Button>
        <Button :disabled="busy" @click="submitTags">{{ t('common.save') }}</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>

  <Dialog v-model:open="ownerOpen">
    <DialogContent>
      <DialogHeader>
        <DialogTitle>{{ t('machines.owner.title') }}</DialogTitle>
        <DialogDescription>{{ t('machines.owner.description') }}</DialogDescription>
      </DialogHeader>

      <div class="space-y-2">
        <Label for="owner">{{ t('machines.owner.label') }}</Label>
        <Select v-model="ownerValue">
          <SelectTrigger id="owner">
            <SelectValue :placeholder="t('machines.owner.select')" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem v-for="user in users" :key="user.id" :value="user.id">
              {{ user.displayName ?? user.name }}
            </SelectItem>
          </SelectContent>
        </Select>
      </div>

      <DialogFooter>
        <Button variant="outline" @click="ownerOpen = false">{{ t('common.cancel') }}</Button>
        <Button
          :disabled="busy || !ownerValue || ownerValue === machine.user?.id"
          @click="submitOwner"
        >
          {{ t('common.save') }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>

  <ConfirmDialog
    :open="expireOpen"
    :title="t('machines.expire.title')"
    :description="t('machines.expire.description', { name: machine.givenName })"
    :confirm-label="t('machines.expire.confirm')"
    destructive
    @update:open="expireOpen = $event"
    @confirm="submitExpire"
  />

  <ConfirmDialog
    :open="deleteOpen"
    :title="t('machines.remove.title')"
    :description="t('machines.remove.description', { name: machine.givenName })"
    :confirm-label="t('machines.remove.confirm')"
    destructive
    @update:open="deleteOpen = $event"
    @confirm="submitDelete"
  >
    <dl class="text-muted-foreground grid grid-cols-2 gap-2 text-sm">
      <dt>{{ t('machines.remove.owner') }}</dt>
      <dd class="text-foreground">
        {{ machine.user?.displayName ?? machine.user?.name ?? t('machines.tagOwned') }}
      </dd>
      <dt>{{ t('machines.remove.created') }}</dt>
      <dd class="text-foreground">{{ formatDate(machine.createdAt) }}</dd>
    </dl>
  </ConfirmDialog>
</template>
