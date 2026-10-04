<script setup lang="ts">
import { computed, ref } from 'vue'
import { Plus, TriangleAlert } from '@lucide/vue'
import { useI18n } from 'vue-i18n'

import EmptyState from '@/components/shared/EmptyState.vue'
import PageHeader from '@/components/shared/PageHeader.vue'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
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
import { errorMessage, useToast } from '@/composables/useToast'
import { api, type DnsResponse } from '@/lib/api'

const { t } = useI18n()
const toast = useToast()

const data = ref<DnsResponse | null>(null)
const loading = ref(true)
const failure = ref<string | null>(null)
const busy = ref(false)

const tailnetOpen = ref(false)
const tailnetName = ref('')
const nsOpen = ref(false)
const nsValue = ref('')
const nsSplit = ref(false)
const nsDomain = ref('')
const domainInput = ref('')
const recordOpen = ref(false)
const recordName = ref('')
const recordType = ref('A')
const recordValue = ref('')

async function load() {
  try {
    data.value = await api.dns.get()
    tailnetName.value = data.value.dns.base_domain ?? ''
    failure.value = null
  } catch (err) {
    failure.value = errorMessage(err)
  } finally {
    loading.value = false
  }
}

load()

const dns = computed(() => data.value?.dns ?? null)
const writable = computed(() => (data.value?.access.writable ?? false) && (data.value?.access.write ?? false))

const nameserverGroups = computed(() => {
  if (!dns.value) return []
  const groups: { name: string; servers: string[]; split: boolean }[] = [
    { name: 'Global', servers: dns.value.nameservers, split: false },
  ]
  for (const [domain, servers] of Object.entries(dns.value.split_dns)) {
    groups.push({ name: domain, servers, split: true })
  }
  return groups
})

/**
 * Runs a config change and reloads the page state. The server answers 200 with
 * a `warning` when the edit was written but Headscale could not reload; that is
 * not a failure, so surface the warning and keep the refreshed data.
 */
async function run(action: () => Promise<unknown>, message: string) {
  busy.value = true
  try {
    const result = (await action()) as { warning?: string | null } | undefined
    await load()
    if (result?.warning) toast.warning(message, result.warning)
    else toast.success(message)
    return true
  } catch (err) {
    toast.error(t('dns.changeFailed'), errorMessage(err))
    return false
  } finally {
    busy.value = false
  }
}

async function addNameserver() {
  const value = nsValue.value.trim()
  if (!value) return
  const split = nsSplit.value ? nsDomain.value.trim() : undefined
  if (nsSplit.value && !split) return
  if (await run(() => api.dns.addNameserver(value, split), t('dns.nameservers.added'))) {
    nsOpen.value = false
    nsValue.value = ''
    nsDomain.value = ''
    nsSplit.value = false
  }
}

function setOverride(value: boolean) {
  return run(() => api.dns.setOverride(value), t('dns.settingUpdated'))
}

function removeNameserver(server: string, group: { name: string; split: boolean }) {
  return run(
    () => api.dns.removeNameserver(server, group.split ? group.name : undefined),
    t('dns.nameservers.removed'),
  )
}

function removeRecord(record: { name: string; type: string; value: string }) {
  return run(() => api.dns.removeRecord(record), t('dns.records.removed'))
}

function removeSearchDomain(domain: string) {
  return run(() => api.dns.removeSearchDomain(domain), t('dns.searchDomains.removed'))
}

function addSearchDomain() {
  const domain = domainInput.value.trim()
  if (!domain) return
  return run(() => api.dns.addSearchDomain(domain), t('dns.searchDomains.added')).then((ok) => {
    if (ok) domainInput.value = ''
  })
}

function toggleMagic(value: boolean) {
  return run(() => api.dns.toggleMagic(value), t('dns.magicDns.updated'))
}

function renameTailnet() {
  const name = tailnetName.value.trim()
  if (!name) return
  return run(() => api.dns.renameTailnet(name), t('dns.tailnet.renamed')).then((ok) => {
    tailnetOpen.value = !ok
  })
}

async function addRecord() {
  if (!recordName.value.trim() || !recordValue.value.trim()) return
  if (
    await run(
      () =>
        api.dns.addRecord({
          name: recordName.value.trim(),
          type: recordType.value,
          value: recordValue.value.trim(),
        }),
      t('dns.records.added'),
    )
  ) {
    recordOpen.value = false
    recordName.value = ''
    recordValue.value = ''
  }
}
</script>

<template>
  <div class="space-y-6">
    <PageHeader :title="t('nav.dns')" :description="t('dns.description')">
      <template #actions>
        <Button :disabled="!writable" @click="nsOpen = true">
          <Plus />
          {{ t('dns.nameservers.add') }}
        </Button>
      </template>
    </PageHeader>

    <div v-if="loading" class="space-y-2">
      <Skeleton class="h-32 w-full" />
      <Skeleton class="h-32 w-full" />
    </div>

    <Alert v-else-if="failure" variant="destructive">
      <TriangleAlert />
      <AlertTitle>{{ t('dns.loadFailedTitle') }}</AlertTitle>
      <AlertDescription>{{ failure }}</AlertDescription>
    </Alert>

    <template v-else-if="dns">
      <Alert v-if="!data?.access.available" variant="warning">
        <AlertTitle>{{ t('dns.unavailableTitle') }}</AlertTitle>
        <AlertDescription>
          <i18n-t keypath="dns.unavailableBody">
            <template #path>
              <code>headscale.config_path</code>
            </template>
          </i18n-t>
        </AlertDescription>
      </Alert>

      <Alert v-else-if="!writable" variant="warning">
        <AlertTitle>{{ t('dns.readOnlyTitle') }}</AlertTitle>
        <AlertDescription>
          {{ data?.access.write ? t('dns.notWritable') : t('dns.noPermission') }}
        </AlertDescription>
      </Alert>

      <Alert v-else-if="data?.integration === 'none'" variant="warning">
        <AlertTitle>{{ t('dns.noReloadTitle') }}</AlertTitle>
        <AlertDescription>
          <i18n-t keypath="dns.noReloadBody">
            <template #signal>
              <code>SIGHUP</code>
            </template>
          </i18n-t>
        </AlertDescription>
      </Alert>

      <Card>
        <CardHeader>
          <CardTitle class="text-base">{{ t('dns.tailnet.title') }}</CardTitle>
          <CardDescription>
            {{ t('dns.tailnet.description') }}
          </CardDescription>
        </CardHeader>
        <CardContent class="flex flex-wrap items-center gap-3">
          <code class="bg-muted flex-1 rounded-md px-3 py-2 text-sm">{{ dns.base_domain ?? '—' }}</code>
          <Button variant="outline" :disabled="!writable" @click="tailnetOpen = true">
            {{ t('dns.tailnet.rename') }}
          </Button>
        </CardContent>
      </Card>

      <Card>
        <CardHeader class="flex-row items-start justify-between">
          <div class="space-y-1.5">
            <CardTitle class="text-base">{{ t('dns.nameservers.title') }}</CardTitle>
            <CardDescription>{{ t('dns.nameservers.description') }}</CardDescription>
          </div>
          <div class="flex items-center gap-2">
            <Label for="override" class="text-xs">{{ t('dns.nameservers.overrideLocalDns') }}</Label>
            <Switch
              id="override"
              :model-value="dns.override_dns"
              :disabled="!writable || busy"
              @update:model-value="setOverride($event)"
            />
          </div>
        </CardHeader>
        <CardContent class="space-y-4">
          <div v-for="group in nameserverGroups" :key="group.name" class="space-y-2">
            <p class="text-sm font-medium">
              {{ group.name === 'Global' ? t('dns.nameservers.global') : group.name }}
              <Badge v-if="group.split" variant="secondary" class="ml-1">{{ t('dns.nameservers.splitBadge') }}</Badge>
            </p>
            <p v-if="!group.servers.length" class="text-muted-foreground text-sm">
              {{ t('dns.nameservers.noneConfigured') }}
            </p>
            <div
              v-for="server in group.servers"
              :key="server"
              class="flex items-center justify-between rounded-md border p-3"
            >
              <code class="text-sm">{{ server }}</code>
              <Button
                size="sm"
                variant="ghost"
                class="text-destructive"
                :disabled="!writable || busy"
                @click="removeNameserver(server, group)"
              >
                {{ t('common.remove') }}
              </Button>
            </div>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader class="flex-row items-center justify-between">
          <div class="space-y-1.5">
            <CardTitle class="text-base">{{ t('dns.records.title') }}</CardTitle>
            <CardDescription>{{ t('dns.records.description') }}</CardDescription>
          </div>
          <Button size="sm" variant="outline" :disabled="!writable" @click="recordOpen = true">
            {{ t('dns.records.add') }}
          </Button>
        </CardHeader>
        <CardContent class="space-y-2">
          <EmptyState v-if="!dns.extra_records.length" :title="t('dns.records.empty')" />
          <div
            v-for="record in dns.extra_records"
            :key="`${record.name}-${record.type}`"
            class="flex items-center gap-3 rounded-md border p-3 text-sm"
          >
            <Badge variant="secondary">{{ record.type }}</Badge>
            <span class="font-medium">{{ record.name }}</span>
            <code class="text-muted-foreground text-xs">{{ record.value }}</code>
            <Button
              size="sm"
              variant="ghost"
              class="text-destructive ml-auto"
              :disabled="!writable || busy"
              @click="removeRecord(record)"
            >
              {{ t('common.remove') }}
            </Button>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle class="text-base">{{ t('dns.searchDomains.title') }}</CardTitle>
          <CardDescription>{{ t('dns.searchDomains.description') }}</CardDescription>
        </CardHeader>
        <CardContent class="space-y-2">
          <div
            v-for="domain in dns.search_domains"
            :key="domain"
            class="flex items-center justify-between rounded-md border p-3"
          >
            <code class="text-sm">{{ domain }}</code>
            <Button
              size="sm"
              variant="ghost"
              class="text-destructive"
              :disabled="!writable || busy"
              @click="removeSearchDomain(domain)"
            >
              {{ t('common.remove') }}
            </Button>
          </div>

          <form
            class="flex gap-2"
            @submit.prevent="addSearchDomain"
          >
            <Input v-model="domainInput" placeholder="corp.example.com" />
            <Button type="submit" variant="outline" :disabled="!writable || !domainInput.trim()">
              {{ t('common.add') }}
            </Button>
          </form>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle class="text-base">Magic DNS</CardTitle>
          <CardDescription>
            <i18n-t keypath="dns.magicDns.description">
              <template #host>
                <code>[device].{{ dns.base_domain ?? 'tailnet' }}</code>
              </template>
            </i18n-t>
          </CardDescription>
        </CardHeader>
        <CardContent class="flex items-center gap-3">
          <Switch
            :model-value="dns.magic_dns"
            :disabled="!writable || busy"
            aria-label="Magic DNS"
            @update:model-value="toggleMagic($event)"
          />
          <span class="text-sm">{{ dns.magic_dns ? t('common.enabled') : t('common.disabled') }}</span>
        </CardContent>
      </Card>
    </template>

    <Dialog v-model:open="tailnetOpen">
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{{ t('dns.tailnet.rename') }}</DialogTitle>
          <DialogDescription>
            {{ t('dns.tailnet.renameDescription') }}
          </DialogDescription>
        </DialogHeader>
        <div class="space-y-2">
          <Label for="tailnet-name">{{ t('dns.tailnet.newBaseDomain') }}</Label>
          <Input id="tailnet-name" v-model="tailnetName" placeholder="tailnet.example.com" />
        </div>
        <DialogFooter>
          <Button variant="outline" @click="tailnetOpen = false">{{ t('common.cancel') }}</Button>
          <Button
            :disabled="busy || !tailnetName.trim()"
            @click="renameTailnet"
          >
            {{ t('dns.tailnet.renameConfirm') }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <Dialog v-model:open="nsOpen">
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{{ t('dns.nameservers.add') }}</DialogTitle>
          <DialogDescription>{{ t('dns.nameservers.addDescription') }}</DialogDescription>
        </DialogHeader>

        <div class="space-y-3">
          <div class="flex items-center gap-2">
            <Switch id="split" v-model="nsSplit" />
            <Label for="split">{{ t('dns.nameservers.split') }}</Label>
          </div>

          <div v-if="nsSplit" class="space-y-2">
            <Label for="split-domain">{{ t('dns.nameservers.domain') }}</Label>
            <Input id="split-domain" v-model="nsDomain" placeholder="corp.example.com" />
          </div>

          <div class="space-y-2">
            <Label for="ns-value">{{ t('dns.nameservers.ip') }}</Label>
            <Input id="ns-value" v-model="nsValue" placeholder="1.1.1.1" />
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" @click="nsOpen = false">{{ t('common.cancel') }}</Button>
          <Button :disabled="busy || !nsValue.trim()" @click="addNameserver">{{ t('common.add') }}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <Dialog v-model:open="recordOpen">
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{{ t('dns.records.add') }}</DialogTitle>
          <DialogDescription>{{ t('dns.records.addDescription') }}</DialogDescription>
        </DialogHeader>

        <div class="space-y-3">
          <div class="space-y-2">
            <Label for="record-type">{{ t('dns.records.type') }}</Label>
            <Select v-model="recordType">
              <SelectTrigger id="record-type"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="A">A</SelectItem>
                <SelectItem value="AAAA">AAAA</SelectItem>
                <SelectItem value="CNAME">CNAME</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div class="space-y-2">
            <Label for="record-name">{{ t('dns.records.name') }}</Label>
            <Input id="record-name" v-model="recordName" placeholder="git.example.com" />
          </div>
          <div class="space-y-2">
            <Label for="record-value">{{ t('dns.records.value') }}</Label>
            <Input
              id="record-value"
              v-model="recordValue"
              :placeholder="recordType === 'AAAA' ? 'fd7a::1' : '100.64.0.5'"
            />
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" @click="recordOpen = false">{{ t('common.cancel') }}</Button>
          <Button :disabled="busy || !recordName.trim() || !recordValue.trim()" @click="addRecord">
            {{ t('common.add') }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>
