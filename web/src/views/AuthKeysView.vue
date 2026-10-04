<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { Check, Copy, LoaderCircle, Plus } from '@lucide/vue'

import ConfirmDialog from '@/components/shared/ConfirmDialog.vue'
import EmptyState from '@/components/shared/EmptyState.vue'
import PageHeader from '@/components/shared/PageHeader.vue'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
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
import { Skeleton } from '@/components/ui/skeleton'
import { Switch } from '@/components/ui/switch'
import {
  Table,
  TableBody,
  TableCell,
  TableEmpty,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { errorMessage, useToast } from '@/composables/useToast'
import { api, type AuthKeysResponse, type PreAuthKey } from '@/lib/api'
import { copyText, formatDate, formatRelative } from '@/lib/format'

const { t } = useI18n()
const toast = useToast()

const data = ref<AuthKeysResponse | null>(null)
const loading = ref(true)
const failure = ref<string | null>(null)
const busy = ref(false)

const userFilter = ref('all')
const statusFilter = ref('active')

const createOpen = ref(false)
const deleteTarget = ref<PreAuthKey | null>(null)
const tagOnly = ref(false)
const userId = ref('')
const aclTags = ref('')
const expiryDays = ref(90)
const reusable = ref(false)
const ephemeral = ref(false)

const createdKey = ref<{ key: PreAuthKey; command: string } | null>(null)
const copied = ref(false)

async function load() {
  try {
    data.value = await api.authKeys.list()
    failure.value = null
    if (data.value.selfServiceOnly && data.value.access.linkedHeadscaleUserId) {
      userId.value = data.value.access.linkedHeadscaleUserId
      tagOnly.value = false
    }
  } catch (err) {
    failure.value = errorMessage(err)
  } finally {
    loading.value = false
  }
}

load()

const selfService = computed(() => data.value?.selfServiceOnly ?? false)

/**
 * A key needs an owner or ACL tags; the server rejects a request with neither.
 * Tag-only mode hides the user field, so a selected user no longer counts.
 */
const canCreateKey = computed(() => {
  if (aclTags.value.trim()) return true
  return !tagOnly.value && Boolean(userId.value)
})

const visibleUsers = computed(() => {
  const users = data.value?.users ?? []
  if (!selfService.value) return users
  const linked = data.value?.access.linkedHeadscaleUserId
  return users.filter((user) => user.id === linked)
})

function isExpired(key: PreAuthKey): boolean {
  const date = new Date(key.expiration)
  return !Number.isNaN(date.getTime()) && date.getTime() < Date.now()
}

function isActive(key: PreAuthKey): boolean {
  return !isExpired(key) && (!key.used || key.reusable)
}

const filtered = computed(() => {
  const keys = data.value?.keys ?? []
  return keys.filter((key) => {
    if (userFilter.value === 'tag-only' && key.user) return false
    if (userFilter.value !== 'all' && userFilter.value !== 'tag-only') {
      if (key.user?.id !== userFilter.value) return false
    }

    switch (statusFilter.value) {
      case 'active':
        return isActive(key)
      case 'used':
        return !isActive(key)
      case 'reusable':
        return key.reusable
      case 'ephemeral':
        return key.ephemeral
      default:
        return true
    }
  })
})

async function createKey() {
  busy.value = true
  try {
    const result = await api.authKeys.create({
      user_id: tagOnly.value ? null : userId.value || null,
      acl_tags: aclTags.value
        .split(',')
        .map((tag) => tag.trim())
        .filter(Boolean)
        .map((tag) => (tag.startsWith('tag:') ? tag : `tag:${tag}`)),
      expiry_days: Number(expiryDays.value) || 90,
      reusable: reusable.value,
      ephemeral: ephemeral.value,
    })
    createdKey.value = result
    createOpen.value = false
    await load()
  } catch (err) {
    toast.error(t('authKeys.toast.createFailed'), errorMessage(err))
  } finally {
    busy.value = false
  }
}

async function expireKey(key: PreAuthKey) {
  busy.value = true
  try {
    await api.authKeys.expire({
      key_id: key.id,
      key: key.key,
      user_id: key.user?.id ?? null,
    })
    toast.success(t('authKeys.toast.expired'))
    await load()
  } catch (err) {
    toast.error(t('authKeys.toast.expireFailed'), errorMessage(err))
  } finally {
    busy.value = false
  }
}

async function deleteKey(key: PreAuthKey) {
  busy.value = true
  try {
    await api.authKeys.remove({
      key_id: key.id,
      key: key.key,
      user_id: key.user?.id ?? null,
    })
    toast.success(t('authKeys.toast.deleted'))
    await load()
  } catch (err) {
    toast.error(t('authKeys.toast.deleteFailed'), errorMessage(err))
  } finally {
    busy.value = false
    deleteTarget.value = null
  }
}

/** Whether the key carries any badge; without one the card row is empty. */
function hasAttributes(key: PreAuthKey): boolean {
  return key.reusable || key.ephemeral || key.used || isExpired(key)
}

async function copyCommand() {
  if (!createdKey.value) return
  if (await copyText(createdKey.value.command)) {
    copied.value = true
    window.setTimeout(() => (copied.value = false), 1500)
  }
}
</script>

<template>
  <div class="space-y-6">
    <PageHeader :title="t('authKeys.title')" :description="t('authKeys.description')">
      <template #actions>
        <Button :disabled="!data?.access.any && !data?.access.own" @click="createOpen = true">
          <Plus />
          {{ t('authKeys.createKey') }}
        </Button>
      </template>
    </PageHeader>

    <div v-if="loading" class="space-y-2">
      <Skeleton v-for="index in 3" :key="index" class="h-12 w-full" />
    </div>

    <Alert v-else-if="failure" variant="destructive">
      <AlertTitle>{{ t('authKeys.loadFailedTitle') }}</AlertTitle>
      <AlertDescription>{{ failure }}</AlertDescription>
    </Alert>

    <template v-else>
      <Alert v-if="data?.missing.length" variant="warning">
        <AlertTitle>{{ t('authKeys.missingTitle') }}</AlertTitle>
        <AlertDescription>
          {{ t('authKeys.missingDescription', { users: data.missing.join(', ') }) }}
        </AlertDescription>
      </Alert>

      <div class="flex flex-wrap items-center gap-2">
        <Select v-model="userFilter">
          <SelectTrigger class="w-48"><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem value="all">{{ t('authKeys.filters.allUsers') }}</SelectItem>
            <SelectItem v-for="user in visibleUsers" :key="user.id" :value="user.id">
              {{ user.displayName ?? user.name }}
            </SelectItem>
            <SelectItem v-if="!selfService" value="tag-only">{{
              t('authKeys.filters.tagOnly')
            }}</SelectItem>
          </SelectContent>
        </Select>

        <Select v-model="statusFilter">
          <SelectTrigger class="w-40"><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem value="all">{{ t('authKeys.filters.anyStatus') }}</SelectItem>
            <SelectItem value="active">{{ t('authKeys.status.active') }}</SelectItem>
            <SelectItem value="used">{{ t('authKeys.filters.usedOrExpired') }}</SelectItem>
            <SelectItem value="reusable">{{ t('authKeys.status.reusable') }}</SelectItem>
            <SelectItem value="ephemeral">{{ t('authKeys.status.ephemeral') }}</SelectItem>
          </SelectContent>
        </Select>
      </div>

      <div class="rounded-lg border">
        <Table class="hp-cards">
          <TableHeader>
            <TableRow>
              <TableHead>{{ t('authKeys.table.key') }}</TableHead>
              <TableHead>{{ t('authKeys.table.user') }}</TableHead>
              <TableHead>{{ t('authKeys.table.attributes') }}</TableHead>
              <TableHead>{{ t('authKeys.table.created') }}</TableHead>
              <TableHead>{{ t('authKeys.table.expires') }}</TableHead>
              <TableHead><span class="sr-only">{{ t('authKeys.table.actions') }}</span></TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            <TableEmpty v-if="!filtered.length" :colspan="6">
              <EmptyState
                :title="
                  data?.keys.length ? t('authKeys.emptyFiltered') : t('authKeys.empty')
                "
              />
            </TableEmpty>

            <TableRow v-for="key in filtered" :key="key.id || key.key">
              <TableCell class="max-w-48">
                <code class="text-xs">{{ key.key }}</code>
              </TableCell>
              <TableCell class="text-sm" :label="t('authKeys.table.user')">
                {{ key.user ? (key.user.displayName ?? key.user.name) : t('authKeys.tagOnly') }}
                <div v-if="key.aclTags.length" class="mt-1 flex flex-wrap gap-1">
                  <Badge v-for="tag in key.aclTags" :key="tag" variant="secondary">{{ tag }}</Badge>
                </div>
              </TableCell>
              <TableCell
                :label="t('authKeys.table.attributes')"
                :class="hasAttributes(key) ? '' : 'hp-card-hide'"
              >
                <div class="flex flex-wrap gap-1">
                  <Badge v-if="key.reusable" variant="secondary">{{
                    t('authKeys.status.reusable')
                  }}</Badge>
                  <Badge v-if="key.ephemeral" variant="secondary">{{
                    t('authKeys.status.ephemeral')
                  }}</Badge>
                  <Badge v-if="key.used" variant="outline">{{ t('authKeys.status.used') }}</Badge>
                  <Badge v-if="isExpired(key)" variant="destructive">{{
                    t('authKeys.status.expired')
                  }}</Badge>
                </div>
              </TableCell>
              <TableCell class="text-sm" :label="t('authKeys.table.created')">{{
                formatDate(key.createdAt)
              }}</TableCell>
              <TableCell class="text-sm" :label="t('authKeys.table.expires')">
                {{ formatRelative(key.expiration, '—') }}
              </TableCell>
              <TableCell class="text-right whitespace-nowrap" label="">
                <Button
                  size="sm"
                  variant="ghost"
                  :disabled="busy || isExpired(key) || !key.user"
                  @click="expireKey(key)"
                >
                  {{ t('authKeys.expire') }}
                </Button>
                <!-- Deleting removes the row; expiring only marks it. -->
                <Button
                  v-if="key.id"
                  size="sm"
                  variant="ghost"
                  class="text-destructive hover:text-destructive"
                  :disabled="busy"
                  @click="deleteTarget = key"
                >
                  {{ t('authKeys.delete') }}
                </Button>
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </div>
    </template>

    <Dialog v-model:open="createOpen">
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{{ t('authKeys.createDialog.title') }}</DialogTitle>
          <DialogDescription>
            <i18n-t keypath="authKeys.createDialog.description" tag="span">
              <template #command>
                <code class="whitespace-nowrap">tailscale up --authkey</code>
              </template>
            </i18n-t>
          </DialogDescription>
        </DialogHeader>

        <div class="space-y-3">
          <div v-if="!selfService" class="flex items-center gap-2">
            <Switch id="tag-only" v-model="tagOnly" />
            <Label for="tag-only">{{ t('authKeys.createDialog.tagOnly') }}</Label>
          </div>

          <div v-if="!tagOnly" class="space-y-2">
            <Label for="key-user">
              {{ t('authKeys.createDialog.user') }}
              <span class="text-muted-foreground">{{ t('authKeys.createDialog.userOptional') }}</span>
            </Label>
            <Select v-model="userId" :disabled="selfService">
              <SelectTrigger id="key-user">
                <SelectValue :placeholder="t('authKeys.createDialog.selectUser')" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem v-for="user in visibleUsers" :key="user.id" :value="user.id">
                  {{ user.displayName ?? user.name }}
                </SelectItem>
              </SelectContent>
            </Select>
            <p v-if="selfService" class="text-muted-foreground text-xs">
              {{ t('authKeys.createDialog.selfServiceHint') }}
            </p>
          </div>

          <div class="space-y-2">
            <Label for="acl-tags">
              {{ t('authKeys.createDialog.aclTags') }}
              <span v-if="!tagOnly" class="text-muted-foreground">({{ t('common.optional') }})</span>
            </Label>
            <Input
              id="acl-tags"
              v-model="aclTags"
              :placeholder="t('authKeys.createDialog.aclTagsPlaceholder')"
            />
          </div>

          <div class="space-y-2">
            <Label for="expiry">{{ t('authKeys.createDialog.expiryDays') }}</Label>
            <Input id="expiry" v-model="expiryDays" type="number" min="1" max="365000" />
          </div>

          <div class="flex items-center gap-4">
            <div class="flex items-center gap-2">
              <Switch id="reusable" v-model="reusable" />
              <Label for="reusable">{{ t('authKeys.status.reusable') }}</Label>
            </div>
            <div class="flex items-center gap-2">
              <Switch id="ephemeral" v-model="ephemeral" />
              <Label for="ephemeral">{{ t('authKeys.status.ephemeral') }}</Label>
            </div>
          </div>

          <p v-if="!canCreateKey" class="text-muted-foreground text-xs">
            {{
              tagOnly
                ? t('authKeys.createDialog.needTags')
                : t('authKeys.createDialog.needOwnerOrTags')
            }}
          </p>
        </div>

        <DialogFooter>
          <Button variant="outline" @click="createOpen = false">{{ t('common.cancel') }}</Button>
          <Button :disabled="busy || !canCreateKey" @click="createKey">
            <LoaderCircle v-if="busy" class="animate-spin" />
            {{ t('authKeys.createKey') }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <Dialog :open="createdKey !== null" @update:open="createdKey = null">
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{{ t('authKeys.createdDialog.title') }}</DialogTitle>
          <DialogDescription>{{ t('authKeys.createdDialog.description') }}</DialogDescription>
        </DialogHeader>

        <div class="space-y-3">
          <div class="bg-muted flex items-center gap-2 rounded-md p-3">
            <code class="flex-1 text-xs break-all">{{ createdKey?.key.key }}</code>
          </div>
          <div class="bg-muted flex items-start gap-2 rounded-md p-3">
            <code class="flex-1 text-xs break-all">{{ createdKey?.command }}</code>
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" @click="copyCommand">
            <Check v-if="copied" />
            <Copy v-else />
            {{ t('authKeys.createdDialog.copyCommand') }}
          </Button>
          <Button @click="createdKey = null">{{ t('common.done') }}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <ConfirmDialog
      :open="deleteTarget !== null"
      :title="t('authKeys.deleteDialog.title')"
      :description="t('authKeys.deleteDialog.description', { key: deleteTarget?.key ?? '' })"
      :confirm-label="t('authKeys.deleteDialog.confirm')"
      destructive
      @update:open="deleteTarget = $event ? deleteTarget : null"
      @confirm="deleteTarget && deleteKey(deleteTarget)"
    />
  </div>
</template>
