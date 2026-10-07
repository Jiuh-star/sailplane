<script setup lang="ts">
import { computed, ref } from 'vue'
import { CircleUser, LoaderCircle, Plus, TriangleAlert } from '@lucide/vue'

import EmptyState from '@/components/shared/EmptyState.vue'
import PageHeader from '@/components/shared/PageHeader.vue'
import StatusCircle from '@/components/shared/StatusCircle.vue'
import UserActions from '@/components/users/UserActions.vue'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
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
import { Skeleton } from '@/components/ui/skeleton'
import {
  Table,
  TableBody,
  TableCell,
  TableEmpty,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import { useI18n } from 'vue-i18n'
import { useLiveResource } from '@/composables/useLive'
import { useSession } from '@/composables/useSession'
import { errorMessage, useToast } from '@/composables/useToast'
import { api, type UserListResponse } from '@/lib/api'
import { formatDate, formatRelative, initials } from '@/lib/format'

const { t, te } = useI18n()
const session = useSession()
const toast = useToast()

const data = ref<UserListResponse | null>(null)
const loading = ref(true)
const failure = ref<string | null>(null)

const createOpen = ref(false)
const createUsername = ref('')
const createDisplayName = ref('')
const createEmail = ref('')
const creating = ref(false)
const createError = ref<string | null>(null)

async function load() {
  try {
    data.value = await api.users.list()
    failure.value = null
  } catch (err) {
    failure.value = errorMessage(err)
  } finally {
    loading.value = false
  }
}

useLiveResource(['users', 'nodes'], load)

const access = computed(
  () =>
    data.value?.access ?? {
      read: false,
      write: false,
      policy_write: false,
      owner: false,
      editable_groups: false,
    },
)

const accounts = computed(() => data.value?.accounts ?? [])
const unlinked = computed(() => data.value?.unlinkedUsers ?? [])

/** Known role enums get a translated label. Anything else keeps the API label. */
function roleLabel(role: string, fallback: string) {
  const key = `users.roles.${role}.label`
  return te(key) ? t(key) : fallback
}

async function createUser() {
  const username = createUsername.value.trim().toLowerCase()
  if (!username) return

  creating.value = true
  createError.value = null
  try {
    await api.users.create(
      username,
      createDisplayName.value.trim() || undefined,
      createEmail.value.trim() || undefined,
    )
    toast.success(t('users.created'), username)
    createOpen.value = false
    createUsername.value = ''
    createDisplayName.value = ''
    createEmail.value = ''
    await load()
  } catch (err) {
    createError.value = errorMessage(err)
  } finally {
    creating.value = false
  }
}
</script>

<template>
  <div class="space-y-6">
    <PageHeader :title="t('nav.users')" :description="t('users.description')">
      <template #actions>
        <Button :disabled="!access.write" @click="createOpen = true">
          <Plus />
          {{ t('users.addUser') }}
        </Button>
      </template>
    </PageHeader>

    <Alert v-if="failure" variant="destructive">
      <TriangleAlert />
      <AlertTitle>{{ t('users.unavailableTitle') }}</AlertTitle>
      <AlertDescription>{{ failure }}</AlertDescription>
    </Alert>

    <Alert v-else-if="!session.config.value.oidcEnabled">
      <CircleUser />
      <AlertTitle>{{ t('users.localTitle') }}</AlertTitle>
      <AlertDescription>{{ t('users.localBody') }}</AlertDescription>
    </Alert>

    <div v-if="loading" class="space-y-2">
      <Skeleton v-for="index in 3" :key="index" class="h-12 w-full" />
    </div>

    <template v-else>
      <Card class="gap-3 py-4">
        <CardHeader>
          <CardTitle class="text-base">{{ t('users.accountsTitle') }}</CardTitle>
        </CardHeader>
        <CardContent class="px-0">
          <Table class="hp-cards">
            <TableHeader>
              <TableRow>
                <TableHead>{{ t('users.columns.user') }}</TableHead>
                <TableHead>{{ t('users.columns.role') }}</TableHead>
                <TableHead>{{ t('users.columns.lastLogin') }}</TableHead>
                <TableHead>{{ t('users.columns.status') }}</TableHead>
                <TableHead><span class="sr-only">{{ t('users.columns.actions') }}</span></TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              <TableEmpty v-if="!accounts.length" :colspan="5">
                <EmptyState :title="t('users.noAccounts')" />
              </TableEmpty>

              <TableRow v-for="account in accounts" :key="account.id">
                <TableCell>
                  <div class="flex items-center gap-3">
                    <Avatar>
                      <AvatarImage v-if="account.picture" :src="account.picture" :alt="account.name" />
                      <AvatarFallback>{{ initials(account.name) }}</AvatarFallback>
                    </Avatar>
                    <div class="flex flex-col">
                      <span class="text-sm font-medium">{{ account.name }}</span>
                      <span v-if="account.email" class="text-muted-foreground text-xs">
                        {{ account.email }}
                      </span>
                      <div class="mt-1 flex flex-wrap items-center gap-1">
                        <Badge v-if="!account.headscale_user_id" variant="warning">{{ t('users.notLinked') }}</Badge>
                        <Badge v-for="group in account.groups" :key="group" variant="secondary">
                          {{ group }}
                        </Badge>
                      </div>
                    </div>
                  </div>
                </TableCell>

                <TableCell :label="t('users.columns.role')">
                  <Badge :variant="account.is_owner ? 'default' : 'secondary'">
                    {{ roleLabel(account.role, account.role_label) }}
                  </Badge>
                </TableCell>

                <TableCell class="text-sm" :label="t('users.columns.lastLogin')">
                  {{ formatDate(account.last_login_at, t('common.never')) }}
                </TableCell>

                <TableCell :label="t('users.columns.status')">
                  <div class="flex items-center gap-2">
                    <StatusCircle :online="account.online" />
                    <span class="text-xs">
                      {{
                        account.online
                          ? t('users.connected')
                          : account.machine_count
                            ? formatRelative(account.last_seen, t('users.offline'))
                            : t('users.noMachines')
                      }}
                    </span>
                  </div>
                </TableCell>

                <TableCell class="text-right" label="">
                  <UserActions
                    kind="account"
                    :account="account"
                    :headscale-users="data?.headscaleUsers ?? []"
                    :policy-groups="data?.policy.groups ?? []"
                    :roles="data?.roles ?? []"
                    :access="access"
                    :current-account-id="data?.currentAccountId ?? null"
                    @changed="load"
                  />
                </TableCell>
              </TableRow>
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      <Card v-if="access.write && unlinked.length" class="gap-3 py-4">
        <CardHeader>
          <CardTitle class="text-base">{{ t('users.unlinkedTitle') }}</CardTitle>
        </CardHeader>
        <CardContent class="px-0">
          <Table class="hp-cards">
            <TableHeader>
              <TableRow>
                <TableHead>{{ t('users.columns.user') }}</TableHead>
                <TableHead>{{ t('users.columns.created') }}</TableHead>
                <TableHead>{{ t('users.columns.status') }}</TableHead>
                <TableHead><span class="sr-only">{{ t('users.columns.actions') }}</span></TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              <TableRow v-for="user in unlinked" :key="user.id">
                <TableCell>
                  <div class="flex flex-col">
                    <span class="text-sm font-medium">{{ user.displayName ?? user.name }}</span>
                    <span class="text-muted-foreground text-xs">
                      {{ user.name }}<template v-if="user.email"> · {{ user.email }}</template>
                    </span>
                  </div>
                </TableCell>
                <TableCell class="text-sm" :label="t('users.columns.created')">{{
                  formatDate(user.createdAt)
                }}</TableCell>
                <TableCell class="text-muted-foreground text-xs" :label="t('users.columns.status')">{{
                  t('users.notLinkedToLogin')
                }}</TableCell>
                <TableCell class="text-right" label="">
                  <UserActions
                    kind="headscale"
                    :headscale-user="user"
                    :policy-groups="data?.policy.groups ?? []"
                    :access="access"
                    @changed="load"
                  />
                </TableCell>
              </TableRow>
            </TableBody>
          </Table>
        </CardContent>
      </Card>
    </template>

    <Dialog v-model:open="createOpen">
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{{ t('users.create.title') }}</DialogTitle>
          <DialogDescription>{{ t('users.create.description') }}</DialogDescription>
        </DialogHeader>

        <form class="space-y-4" @submit.prevent="createUser">
          <div class="space-y-2">
            <Label for="new-username">{{ t('users.create.username') }}</Label>
            <Input id="new-username" v-model="createUsername" autocomplete="off" required minlength="2" />
            <p class="text-muted-foreground text-xs">{{ t('users.create.usernameHint') }}</p>
          </div>

          <div class="space-y-2">
            <Label for="new-display">
              {{ t('users.create.displayName') }}
              <span class="text-muted-foreground">({{ t('common.optional') }})</span>
            </Label>
            <Input id="new-display" v-model="createDisplayName" autocomplete="off" />
          </div>

          <div class="space-y-2">
            <Label for="new-email">
              {{ t('users.create.email') }}
              <span class="text-muted-foreground">({{ t('common.optional') }})</span>
            </Label>
            <Input id="new-email" v-model="createEmail" type="email" autocomplete="off" />
          </div>

          <Alert v-if="createError" variant="destructive">
            <AlertTitle>{{ t('users.create.errorTitle') }}</AlertTitle>
            <AlertDescription>{{ createError }}</AlertDescription>
          </Alert>
        </form>

        <DialogFooter>
          <Button variant="outline" @click="createOpen = false">{{ t('common.cancel') }}</Button>
          <Button :disabled="creating || !createUsername.trim()" @click="createUser">
            <LoaderCircle v-if="creating" class="animate-spin" />
            {{ t('users.create.submit') }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>
