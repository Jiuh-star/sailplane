<script setup lang="ts">
import { computed, ref } from 'vue'
import { Ellipsis, Link2, LoaderCircle, Pencil, Tags, Trash2, UserCog } from '@lucide/vue'

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
import { useI18n } from 'vue-i18n'
import { errorMessage, useToast } from '@/composables/useToast'
import { api, type AccountView, type HeadscaleUser, type UserListResponse } from '@/lib/api'

type Access = UserListResponse['access']

const props = defineProps<{
  kind: 'account' | 'headscale'
  account?: AccountView
  headscaleUser?: HeadscaleUser
  headscaleUsers?: HeadscaleUser[]
  policyGroups: string[]
  roles?: { value: string; label: string; description: string }[]
  access: Access
  currentAccountId?: string | null
}>()

const emit = defineEmits<{ (e: 'changed'): void }>()

const { t, te } = useI18n()
const toast = useToast()

const roleOpen = ref(false)
const linkOpen = ref(false)
const renameOpen = ref(false)
const groupsOpen = ref(false)
const transferOpen = ref(false)
const deleteOpen = ref(false)
const busy = ref(false)

const roleValue = ref('')
const linkValue = ref('')
const renameValue = ref('')
const groupsValue = ref<string[]>([])
const groupInput = ref('')

const isSelf = computed(() => props.account?.id === props.currentAccountId)
const label = computed(
  () => props.account?.name ?? props.headscaleUser?.name ?? t('users.userFallback'),
)

/** Known role enums get translated copy. Anything else keeps the API label. */
function roleLabel(role: { value: string; label: string }) {
  const key = `users.roles.${role.value}.label`
  return te(key) ? t(key) : role.label
}

function roleDescription(role: { value: string; label: string; description: string }) {
  const key = `users.roles.${role.value}.description`
  return te(key) ? t(key) : role.description
}

/** All Headscale users are candidates. The server rejects a user that another account already claims. */
const availableUsers = computed(() => props.headscaleUsers ?? [])

async function run(action: () => Promise<unknown>, message: string) {
  busy.value = true
  try {
    await action()
    toast.success(message)
    emit('changed')
    return true
  } catch (err) {
    toast.error(t('common.actionFailed'), errorMessage(err))
    return false
  } finally {
    busy.value = false
  }
}

function openRole() {
  roleValue.value = props.account?.role ?? 'member'
  roleOpen.value = true
}

function openLink() {
  linkValue.value = props.account?.headscale_user_id ?? ''
  linkOpen.value = true
}

function openRename() {
  renameValue.value = props.headscaleUser?.name ?? ''
  renameOpen.value = true
}

function openGroups() {
  groupsValue.value = [...(props.account?.groups ?? [])]
  groupInput.value = ''
  groupsOpen.value = true
}

async function submitRole() {
  if (!props.account) return
  if (await run(() => api.accounts.setRole(props.account!.id, roleValue.value), t('users.toasts.roleUpdated'))) {
    roleOpen.value = false
  }
}

async function submitLink() {
  if (!props.account || !linkValue.value) return
  if (await run(() => api.accounts.link(props.account!.id, linkValue.value), t('users.toasts.accountLinked'))) {
    linkOpen.value = false
  }
}

async function submitRename() {
  const name = renameValue.value.trim().toLowerCase()
  if (!props.headscaleUser || !name) return
  if (await run(() => api.users.rename(props.headscaleUser!.id, name), t('users.toasts.userRenamed'))) {
    renameOpen.value = false
  }
}

async function submitGroups() {
  const userName = props.account?.headscale_user_name ?? props.headscaleUser?.name
  if (!userName) return
  if (await run(() => api.users.setGroups(userName, groupsValue.value), t('users.toasts.groupsUpdated'))) {
    groupsOpen.value = false
  }
}

async function submitTransfer() {
  if (!props.account) return
  if (
    await run(
      () => api.accounts.transferOwnership(props.account!.id),
      t('users.toasts.ownershipTransferred'),
    )
  ) {
    transferOpen.value = false
  }
}

async function submitDelete() {
  const account = props.account
  const user = props.headscaleUser
  const action = account ? api.accounts.remove(account.id) : api.users.remove(user!.id)
  if (
    await run(() => action, account ? t('users.toasts.accountRemoved') : t('users.toasts.userDeleted'))
  ) {
    deleteOpen.value = false
  }
}

function addGroup() {
  const name = groupInput.value.trim()
  if (!name) return
  const value = name.startsWith('group:') ? name : `group:${name}`
  if (!groupsValue.value.includes(value)) groupsValue.value = [...groupsValue.value, value]
  groupInput.value = ''
}

const canEditGroups = computed(
  () => props.access.editable_groups && Boolean(props.account?.headscale_user_id || props.kind === 'headscale'),
)
</script>

<template>
  <DropdownMenu>
    <DropdownMenuTrigger as-child>
      <Button variant="ghost" size="icon" :disabled="busy" :aria-label="t('users.actions')">
        <LoaderCircle v-if="busy" class="animate-spin" />
        <Ellipsis v-else />
      </Button>
    </DropdownMenuTrigger>

    <DropdownMenuContent align="end" class="w-56">
      <template v-if="kind === 'account'">
        <DropdownMenuItem :disabled="!access.write || !account?.headscale_user_id" @select="openRole">
          <UserCog />
          {{ t('users.menu.changeRole') }}
        </DropdownMenuItem>
        <DropdownMenuItem :disabled="!access.write" @select="openLink">
          <Link2 />
          {{ account?.headscale_user_id ? t('users.menu.changeLinkedUser') : t('users.menu.linkHeadscaleUser') }}
        </DropdownMenuItem>
        <DropdownMenuItem v-if="canEditGroups" @select="openGroups">
          <Tags />
          {{ t('users.menu.editGroups') }}
        </DropdownMenuItem>

        <template v-if="access.owner && !account?.is_owner && !isSelf">
          <DropdownMenuSeparator />
          <DropdownMenuItem variant="destructive" @select="transferOpen = true">
            {{ t('users.menu.transferOwnership') }}
          </DropdownMenuItem>
        </template>

        <template v-if="access.write && !isSelf && !account?.is_owner">
          <DropdownMenuSeparator />
          <DropdownMenuItem variant="destructive" @select="deleteOpen = true">
            <Trash2 />
            {{ t('users.menu.deleteAccount') }}
          </DropdownMenuItem>
        </template>
      </template>

      <template v-else>
        <DropdownMenuItem
          :disabled="!access.write || headscaleUser?.provider === 'oidc'"
          @select="openRename"
        >
          <Pencil />
          {{ t('users.menu.rename') }}
        </DropdownMenuItem>
        <DropdownMenuItem v-if="canEditGroups" @select="openGroups">
          <Tags />
          {{ t('users.menu.editGroups') }}
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem variant="destructive" :disabled="!access.write" @select="deleteOpen = true">
          <Trash2 />
          {{ t('common.delete') }}
        </DropdownMenuItem>
      </template>
    </DropdownMenuContent>
  </DropdownMenu>

  <Dialog v-model:open="roleOpen">
    <DialogContent>
      <DialogHeader>
        <DialogTitle>{{ t('users.roleDialog.title') }}</DialogTitle>
        <DialogDescription>{{ t('users.roleDialog.description') }}</DialogDescription>
      </DialogHeader>

      <div class="space-y-2">
        <Label for="role">{{ t('users.roleDialog.role') }}</Label>
        <Select v-model="roleValue">
          <SelectTrigger id="role"><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem v-for="role in roles ?? []" :key="role.value" :value="role.value">
              {{ roleLabel(role) }} — {{ roleDescription(role) }}
            </SelectItem>
          </SelectContent>
        </Select>
      </div>

      <DialogFooter>
        <Button variant="outline" @click="roleOpen = false">{{ t('common.cancel') }}</Button>
        <Button :disabled="busy || roleValue === account?.role" @click="submitRole">{{ t('common.save') }}</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>

  <Dialog v-model:open="linkOpen">
    <DialogContent>
      <DialogHeader>
        <DialogTitle>{{ t('users.linkDialog.title') }}</DialogTitle>
        <DialogDescription>{{ t('users.linkDialog.description') }}</DialogDescription>
      </DialogHeader>

      <div class="space-y-2">
        <Label for="link">{{ t('users.linkDialog.userLabel') }}</Label>
        <Select v-model="linkValue">
          <SelectTrigger id="link">
            <SelectValue :placeholder="t('users.linkDialog.placeholder')" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem v-for="user in availableUsers" :key="user.id" :value="user.id">
              {{ user.displayName ?? user.name }}<template v-if="user.id === account?.headscale_user_id">{{ t('users.linkDialog.current') }}</template>
            </SelectItem>
          </SelectContent>
        </Select>
      </div>

      <DialogFooter>
        <Button variant="outline" @click="linkOpen = false">{{ t('common.cancel') }}</Button>
        <Button :disabled="busy || !linkValue" @click="submitLink">{{ t('common.save') }}</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>

  <Dialog v-model:open="renameOpen">
    <DialogContent>
      <DialogHeader>
        <DialogTitle>{{ t('users.renameDialog.title') }}</DialogTitle>
        <DialogDescription>
          <i18n-t keypath="users.renameDialog.description" tag="span">
            <template #name><code>{{ headscaleUser?.name }}@</code></template>
          </i18n-t>
        </DialogDescription>
      </DialogHeader>

      <div class="space-y-2">
        <Label for="rename">{{ t('users.renameDialog.name') }}</Label>
        <Input id="rename" v-model="renameValue" autocomplete="off" />
      </div>

      <DialogFooter>
        <Button variant="outline" @click="renameOpen = false">{{ t('common.cancel') }}</Button>
        <Button :disabled="busy || !renameValue.trim()" @click="submitRename">{{ t('common.save') }}</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>

  <Dialog v-model:open="groupsOpen">
    <DialogContent>
      <DialogHeader>
        <DialogTitle>{{ t('users.groupsDialog.title') }}</DialogTitle>
        <DialogDescription>
          <i18n-t keypath="users.groupsDialog.description" tag="span">
            <template #name><code>{{ label }}@</code></template>
          </i18n-t>
        </DialogDescription>
      </DialogHeader>

      <div class="space-y-4">
        <div v-if="groupsValue.length" class="flex flex-wrap gap-1">
          <button
            v-for="group in groupsValue"
            :key="group"
            class="bg-secondary hover:bg-destructive/20 rounded-md px-2 py-1 text-xs"
            @click="groupsValue = groupsValue.filter((value) => value !== group)"
          >
            {{ group }} ×
          </button>
        </div>
        <p v-else class="text-muted-foreground text-sm">{{ t('users.groupsDialog.empty') }}</p>

        <form class="flex gap-2" @submit.prevent="addGroup">
          <Input v-model="groupInput" :placeholder="t('users.groupsDialog.placeholder')" autocomplete="off" />
          <Button type="submit" variant="outline" :disabled="!groupInput.trim()">{{ t('common.add') }}</Button>
        </form>

        <div v-if="policyGroups.length" class="flex flex-wrap gap-1">
          <button
            v-for="group in policyGroups.filter((g) => !groupsValue.includes(g))"
            :key="group"
            class="text-muted-foreground hover:bg-accent rounded-md border px-2 py-1 text-xs"
            @click="groupsValue = [...groupsValue, group]"
          >
            + {{ group }}
          </button>
        </div>
      </div>

      <DialogFooter>
        <Button variant="outline" @click="groupsOpen = false">{{ t('common.cancel') }}</Button>
        <Button :disabled="busy" @click="submitGroups">{{ t('common.save') }}</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>

  <ConfirmDialog
    :open="transferOpen"
    :title="t('users.transfer.title')"
    :description="t('users.transfer.description', { name: label })"
    :confirm-label="t('users.transfer.confirm')"
    destructive
    @update:open="transferOpen = $event"
    @confirm="submitTransfer"
  />

  <ConfirmDialog
    :open="deleteOpen"
    :title="kind === 'account' ? t('users.delete.accountTitle') : t('users.delete.userTitle')"
    :description="
      kind === 'account'
        ? t('users.delete.accountDescription', { name: label })
        : t('users.delete.userDescription', { name: label })
    "
    :confirm-label="t('common.delete')"
    destructive
    @update:open="deleteOpen = $event"
    @confirm="submitDelete"
  >
    <Alert v-if="account && account.machine_count > 0" variant="warning">
      <AlertTitle>{{ t('users.delete.ownedMachinesTitle') }}</AlertTitle>
      <AlertDescription class="text-xs">
        {{ t('users.delete.ownedMachinesBody', { count: account.machine_count }) }}
      </AlertDescription>
    </Alert>
  </ConfirmDialog>
</template>
