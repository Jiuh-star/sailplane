<script setup lang="ts">
import { computed, defineAsyncComponent, ref, watch } from 'vue'
import { LoaderCircle, Save, TriangleAlert } from '@lucide/vue'
import { useI18n } from 'vue-i18n'

import AccessCheck from '@/components/acl/AccessCheck.vue'
import EmptyState from '@/components/shared/EmptyState.vue'
import PageHeader from '@/components/shared/PageHeader.vue'
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
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'

// CodeMirror is a few hundred kilobytes; only the ACL page needs it.
const JsonEditor = defineAsyncComponent(() => import('@/components/shared/JsonEditor.vue'))
const JsonDiff = defineAsyncComponent(() => import('@/components/shared/JsonDiff.vue'))
import { errorMessage, useToast } from '@/composables/useToast'
import { api, type AclResponse, type AclRule, type ParsedPolicy, type SshRule } from '@/lib/api'

const { t } = useI18n()
const toast = useToast()

const data = ref<AclResponse | null>(null)
const policyText = ref('')
/** The policy as Headscale has it: the left-hand side of the diff. */
const savedPolicy = ref('')
const parsed = ref<ParsedPolicy | null>(null)
const loading = ref(true)
const saving = ref(false)
const failure = ref<string | null>(null)
const dirty = ref(false)
const tab = ref('rules')

// --- Rule dialogs ---
const ruleOpen = ref(false)
const ruleIndex = ref<number | null>(null)
const ruleDraft = ref<AclRule>({ action: 'accept', src: [], dst: [], proto: '' })

const sshOpen = ref(false)
const sshIndex = ref<number | null>(null)
const sshDraft = ref<SshRule>({ action: 'accept', src: [], dst: [], users: [], checkPeriod: '' })

const hostOpen = ref(false)
const hostIndex = ref<string | null>(null)
const hostDraft = ref({ name: '', value: '' })

const listOpen = ref(false)
const listKind = ref<'group' | 'tag'>('group')
const listIndex = ref<string | null>(null)
const listDraft = ref({ name: '', members: '' })

/** The structured policy as loaded, re-serialised the same way edits are. */
const savedSnapshot = ref('')

async function load() {
  try {
    const response = await api.acl.get()
    data.value = response
    policyText.value = response.policy
    savedPolicy.value = response.policy
    parsed.value = response.parsed
    savedSnapshot.value = response.parsed ? JSON.stringify(response.parsed, null, 2) : ''
    dirty.value = false
    failure.value = null
  } catch (err) {
    failure.value = errorMessage(err)
  } finally {
    loading.value = false
  }
}

load()

const writable = computed(() => (data.value?.writable ?? false) && (data.value?.access.write ?? false))
const hasComments = computed(() => data.value?.hasComments ?? false)
const parseError = computed(() => data.value?.parseError ?? null)

watch(parsed, (value) => {
  // Any structured edit regenerates the policy text. Comparing against the
  // snapshot, rather than setting a flag, keeps load and save from marking the
  // page dirty; an edit typed back out also stops counting as a change.
  if (!value) return
  policyText.value = JSON.stringify(value, null, 2)
  dirty.value = policyText.value !== savedSnapshot.value
})

const acls = computed(() => parsed.value?.acls ?? [])
const sshRules = computed(() => parsed.value?.ssh ?? [])
const hosts = computed(() => Object.entries(parsed.value?.hosts ?? {}))
const groups = computed(() => Object.entries(parsed.value?.groups ?? {}))
const tags = computed(() => Object.entries(parsed.value?.tagOwners ?? {}))

const tagUsage = computed(() => data.value?.tagUsage ?? {})

const listDialogTitle = computed(() =>
  t(
    listKind.value === 'group'
      ? listIndex.value
        ? 'acls.listDialog.editGroupTitle'
        : 'acls.listDialog.newGroupTitle'
      : listIndex.value
        ? 'acls.listDialog.editTagTitle'
        : 'acls.listDialog.newTagTitle'
  )
)

async function save() {
  if (!parsed.value && !policyText.value.trim()) return
  saving.value = true
  try {
    const source = parsed.value ? JSON.stringify(parsed.value) : policyText.value
    const result = await api.acl.set(source)
    toast.success(t('acls.saved'))
    policyText.value = result.policy
    dirty.value = false
    await load()
  } catch (err) {
    toast.error(t('acls.saveFailed'), errorMessage(err))
  } finally {
    saving.value = false
  }
}

function discard() {
  void load()
}

// --- Rules ---

function editRule(index: number | null) {
  ruleIndex.value = index
  ruleDraft.value =
    index === null
      ? { action: 'accept', src: [], dst: [], proto: '' }
      : { ...acls.value[index]!, src: [...acls.value[index]!.src], dst: [...acls.value[index]!.dst] }
  ruleOpen.value = true
}

function saveRule() {
  if (!parsed.value) return
  const rule: AclRule = {
    action: ruleDraft.value.action || 'accept',
    src: splitList(ruleDraft.value.src.join(',')),
    dst: splitList(ruleDraft.value.dst.join(',')).map(withDefaultPort),
  }
  if (ruleDraft.value.proto?.trim()) rule.proto = ruleDraft.value.proto.trim()
  else delete rule.proto

  const next = [...acls.value]
  if (ruleIndex.value === null) next.push(rule)
  else next[ruleIndex.value] = rule
  parsed.value = { ...parsed.value, acls: next }
  ruleOpen.value = false
}

function removeRule(index: number) {
  if (!parsed.value) return
  parsed.value = { ...parsed.value, acls: acls.value.filter((_, i) => i !== index) }
}

function editSsh(index: number | null) {
  sshIndex.value = index
  sshDraft.value =
    index === null
      ? { action: 'accept', src: [], dst: [], users: [], checkPeriod: '' }
      : {
          ...sshRules.value[index]!,
          src: [...sshRules.value[index]!.src],
          dst: [...sshRules.value[index]!.dst],
          users: [...sshRules.value[index]!.users],
        }
  sshOpen.value = true
}

function saveSsh() {
  if (!parsed.value) return
  const rule: SshRule = {
    action: sshDraft.value.action || 'accept',
    src: splitList(sshDraft.value.src.join(',')),
    dst: splitList(sshDraft.value.dst.join(',')),
    users: splitList(sshDraft.value.users.join(',')),
  }
  if (rule.action === 'check' && sshDraft.value.checkPeriod?.trim()) {
    rule.checkPeriod = sshDraft.value.checkPeriod.trim()
  }

  const next = [...sshRules.value]
  if (sshIndex.value === null) next.push(rule)
  else next[sshIndex.value] = rule
  parsed.value = { ...parsed.value, ssh: next }
  sshOpen.value = false
}

function removeSsh(index: number) {
  if (!parsed.value) return
  parsed.value = { ...parsed.value, ssh: sshRules.value.filter((_, i) => i !== index) }
}

function editHost(name: string | null) {
  hostIndex.value = name
  hostDraft.value = name
    ? { name, value: parsed.value?.hosts[name] ?? '' }
    : { name: '', value: '' }
  hostOpen.value = true
}

function saveHost() {
  if (!parsed.value) return
  const next = { ...parsed.value.hosts }
  if (hostIndex.value && hostIndex.value !== hostDraft.value.name) delete next[hostIndex.value]
  next[hostDraft.value.name.trim()] = hostDraft.value.value.trim()
  parsed.value = { ...parsed.value, hosts: next }
  hostOpen.value = false
}

function removeHost(name: string) {
  if (!parsed.value) return
  const next = { ...parsed.value.hosts }
  delete next[name]
  parsed.value = { ...parsed.value, hosts: next }
}

function editList(kind: 'group' | 'tag', name: string | null) {
  listKind.value = kind
  listIndex.value = name
  const source = kind === 'group' ? parsed.value?.groups : parsed.value?.tagOwners
  listDraft.value = name
    ? { name, members: (source?.[name] ?? []).join(', ') }
    : { name: kind === 'group' ? 'group:' : 'tag:', members: '' }
  listOpen.value = true
}

function saveList() {
  if (!parsed.value) return
  const key = listKind.value === 'group' ? 'groups' : 'tagOwners'
  const next = { ...(parsed.value[key] ?? {}) }
  if (listIndex.value && listIndex.value !== listDraft.value.name) delete next[listIndex.value]
  next[listDraft.value.name.trim()] = splitList(listDraft.value.members)
  parsed.value = { ...parsed.value, [key]: next }
  listOpen.value = false
}

function removeList(kind: 'group' | 'tag', name: string) {
  if (!parsed.value) return
  const key = kind === 'group' ? 'groups' : 'tagOwners'
  const next = { ...(parsed.value[key] ?? {}) }
  delete next[name]
  parsed.value = { ...parsed.value, [key]: next }
}

function splitList(input: string): string[] {
  return input
    .split(/[,\n]/)
    .map((value) => value.trim())
    .filter(Boolean)
}

/** Headscale requires `host:port`; the editor accepts bare hosts. */
function withDefaultPort(destination: string): string {
  if (!destination || destination.includes(':')) return destination
  return `${destination}:*`
}
</script>

<template>
  <div class="space-y-6">
    <PageHeader :title="t('acls.title')" :description="t('acls.description')">
      <template #actions>
        <Button variant="outline" :disabled="!dirty || saving" @click="discard">
          {{ t('common.discard') }}
        </Button>
        <Button :disabled="!writable || !dirty || saving" @click="save">
          <LoaderCircle v-if="saving" class="animate-spin" />
          <Save v-else />
          {{ t('common.save') }}
        </Button>
      </template>
    </PageHeader>

    <div v-if="loading" class="space-y-2">
      <Skeleton class="h-10 w-full" />
      <Skeleton class="h-40 w-full" />
    </div>

    <Alert v-else-if="failure" variant="destructive">
      <TriangleAlert />
      <AlertTitle>{{ t('acls.loadErrorTitle') }}</AlertTitle>
      <AlertDescription>{{ failure }}</AlertDescription>
    </Alert>

    <template v-else>
      <Alert v-if="!data?.access.write" variant="warning">
        <AlertTitle>{{ t('acls.readOnlyAccess.title') }}</AlertTitle>
        <AlertDescription>{{ t('acls.readOnlyAccess.body') }}</AlertDescription>
      </Alert>

      <Alert v-else-if="!data?.writable" variant="warning">
        <AlertTitle>{{ t('acls.readOnlyPolicy.title') }}</AlertTitle>
        <AlertDescription>
          <i18n-t keypath="acls.readOnlyPolicy.body" tag="span">
            <template #mode>
              <code>policy.mode: database</code>
            </template>
          </i18n-t>
        </AlertDescription>
      </Alert>

      <Alert v-if="parseError" variant="destructive">
        <TriangleAlert />
        <AlertTitle>{{ t('acls.parseError.title') }}</AlertTitle>
        <AlertDescription>
          {{ t('acls.parseError.body', { error: parseError }) }}
        </AlertDescription>
      </Alert>

      <Tabs v-model="tab">
        <TabsList>
          <TabsTrigger value="rules">{{ t('acls.tabs.rules') }}</TabsTrigger>
          <TabsTrigger value="tags">{{ t('acls.tabs.tags') }}</TabsTrigger>
          <TabsTrigger value="file">{{ t('acls.tabs.file') }}</TabsTrigger>
          <TabsTrigger value="diff">{{ t('acls.tabs.diff') }}</TabsTrigger>
          <TabsTrigger value="check">{{ t('acls.tabs.check') }}</TabsTrigger>
        </TabsList>

        <TabsContent value="rules" class="space-y-4 pt-4">
          <Alert v-if="hasComments" variant="warning">
            <AlertTitle>{{ t('acls.commentsLost.title') }}</AlertTitle>
            <AlertDescription>{{ t('acls.commentsLost.body') }}</AlertDescription>
          </Alert>

          <Card>
            <CardHeader class="flex-row items-center justify-between">
              <CardTitle class="text-base">{{ t('acls.rules.title') }}</CardTitle>
              <Button size="sm" variant="outline" :disabled="!writable" @click="editRule(null)">
                {{ t('acls.rules.add') }}
              </Button>
            </CardHeader>
            <CardContent class="space-y-2">
              <EmptyState v-if="!acls.length" :title="t('acls.rules.empty')" />
              <div
                v-for="(rule, index) in acls"
                :key="index"
                class="flex flex-wrap items-center gap-2 rounded-md border p-3 text-sm"
              >
                <Badge>{{ rule.action === 'accept' ? t('acls.rules.allow') : rule.action }}</Badge>
                <Badge v-for="src in rule.src" :key="src" variant="secondary">{{ src }}</Badge>
                <span class="text-muted-foreground">→</span>
                <Badge v-for="dst in rule.dst" :key="dst" variant="outline">{{ dst }}</Badge>
                <Badge v-if="rule.proto" variant="outline">{{ rule.proto }}</Badge>
                <span class="ml-auto flex gap-1">
                  <Button size="sm" variant="ghost" :disabled="!writable" @click="editRule(index)">
                    {{ t('common.edit') }}
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    class="text-destructive"
                    :disabled="!writable"
                    @click="removeRule(index)"
                  >
                    {{ t('common.delete') }}
                  </Button>
                </span>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader class="flex-row items-center justify-between">
              <CardTitle class="text-base">{{ t('acls.ssh.title') }}</CardTitle>
              <Button size="sm" variant="outline" :disabled="!writable" @click="editSsh(null)">
                {{ t('acls.ssh.add') }}
              </Button>
            </CardHeader>
            <CardContent class="space-y-2">
              <EmptyState v-if="!sshRules.length" :title="t('acls.ssh.empty')" />
              <div
                v-for="(rule, index) in sshRules"
                :key="index"
                class="flex flex-wrap items-center gap-2 rounded-md border p-3 text-sm"
              >
                <Badge>{{ rule.action }}</Badge>
                <Badge v-for="src in rule.src" :key="src" variant="secondary">{{ src }}</Badge>
                <span class="text-muted-foreground">→</span>
                <Badge v-for="dst in rule.dst" :key="dst" variant="outline">{{ dst }}</Badge>
                <span class="text-muted-foreground">{{ t('acls.ssh.as') }}</span>
                <Badge v-for="user in rule.users" :key="user" variant="outline">{{ user }}</Badge>
                <span class="ml-auto flex gap-1">
                  <Button size="sm" variant="ghost" :disabled="!writable" @click="editSsh(index)">
                    {{ t('common.edit') }}
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    class="text-destructive"
                    :disabled="!writable"
                    @click="removeSsh(index)"
                  >
                    {{ t('common.delete') }}
                  </Button>
                </span>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader class="flex-row items-center justify-between">
              <CardTitle class="text-base">{{ t('acls.hosts.title') }}</CardTitle>
              <Button size="sm" variant="outline" :disabled="!writable" @click="editHost(null)">
                {{ t('acls.hosts.add') }}
              </Button>
            </CardHeader>
            <CardContent class="space-y-2">
              <EmptyState v-if="!hosts.length" :title="t('acls.hosts.empty')" />
              <div
                v-for="[name, value] in hosts"
                :key="name"
                class="flex items-center gap-3 rounded-md border p-3 text-sm"
              >
                <span class="font-medium">{{ name }}</span>
                <span class="text-muted-foreground font-mono text-xs">{{ value }}</span>
                <span class="ml-auto flex gap-1">
                  <Button size="sm" variant="ghost" :disabled="!writable" @click="editHost(name)">
                    {{ t('common.edit') }}
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    class="text-destructive"
                    :disabled="!writable"
                    @click="removeHost(name)"
                  >
                    {{ t('common.delete') }}
                  </Button>
                </span>
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="tags" class="space-y-4 pt-4">
          <Card>
            <CardHeader class="flex-row items-center justify-between">
              <CardTitle class="text-base">{{ t('acls.groups.title') }}</CardTitle>
              <Button size="sm" variant="outline" :disabled="!writable" @click="editList('group', null)">
                {{ t('acls.groups.add') }}
              </Button>
            </CardHeader>
            <CardContent class="space-y-2">
              <EmptyState v-if="!groups.length" :title="t('acls.groups.empty')" />
              <div
                v-for="[name, members] in groups"
                :key="name"
                class="flex flex-wrap items-center gap-2 rounded-md border p-3 text-sm"
              >
                <span class="font-medium">{{ name }}</span>
                <Badge v-for="member in members" :key="member" variant="secondary">{{ member }}</Badge>
                <span v-if="!members.length" class="text-muted-foreground text-xs">
                  {{ t('acls.groups.noMembers') }}
                </span>
                <span class="ml-auto flex gap-1">
                  <Button size="sm" variant="ghost" :disabled="!writable" @click="editList('group', name)">
                    {{ t('common.edit') }}
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    class="text-destructive"
                    :disabled="!writable"
                    @click="removeList('group', name)"
                  >
                    {{ t('common.delete') }}
                  </Button>
                </span>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader class="flex-row items-center justify-between">
              <CardTitle class="text-base">{{ t('acls.tags.title') }}</CardTitle>
              <Button size="sm" variant="outline" :disabled="!writable" @click="editList('tag', null)">
                {{ t('acls.tags.add') }}
              </Button>
            </CardHeader>
            <CardContent class="space-y-2">
              <EmptyState v-if="!tags.length" :title="t('acls.tags.empty')" />
              <div
                v-for="[name, owners] in tags"
                :key="name"
                class="flex flex-wrap items-center gap-2 rounded-md border p-3 text-sm"
              >
                <span class="font-medium">{{ name }}</span>
                <span class="text-muted-foreground text-xs">
                  {{
                    tagUsage[name]?.length
                      ? t(
                          'acls.tags.machineCount',
                          { count: tagUsage[name]!.length, names: tagUsage[name]!.join(', ') },
                          tagUsage[name]!.length
                        )
                      : t('acls.tags.unassigned')
                  }}
                </span>
                <Badge v-for="owner in owners" :key="owner" variant="secondary">{{ owner }}</Badge>
                <span v-if="!owners.length" class="text-muted-foreground text-xs">
                  {{ t('acls.tags.noOwners') }}
                </span>
                <span class="ml-auto flex gap-1">
                  <Button size="sm" variant="ghost" :disabled="!writable" @click="editList('tag', name)">
                    {{ t('common.edit') }}
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    class="text-destructive"
                    :disabled="!writable"
                    @click="removeList('tag', name)"
                  >
                    {{ t('common.delete') }}
                  </Button>
                </span>
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="file" class="space-y-4 pt-4">
          <JsonEditor
            :model-value="parsed ? JSON.stringify(parsed, null, 2) : policyText"
            :readonly="!writable"
            @update:model-value="
              (value) => {
                policyText = value
                dirty = true
              }
            "
          />
          <p class="text-muted-foreground text-xs">
            <i18n-t keypath="acls.file.hint" tag="span">
              <template #comment>
                <code>//</code>
              </template>
            </i18n-t>
          </p>
        </TabsContent>

        <TabsContent value="diff" class="space-y-4 pt-4">
          <Card>
            <CardHeader>
              <CardTitle class="text-base">{{ t('acls.diff.title') }}</CardTitle>
            </CardHeader>
            <CardContent>
              <EmptyState
                v-if="!dirty"
                :title="t('acls.diff.empty')"
                :description="t('acls.diff.emptyDescription')"
              />
              <!-- A diff against the saved policy, not a dump
                   of the whole document. -->
              <JsonDiff
                v-else
                :original="savedPolicy"
                :modified="parsed ? JSON.stringify(parsed, null, 2) : policyText"
              />
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="check" class="pt-4">
          <AccessCheck
            :selectors="data?.selectors ?? null"
            :draft="dirty ? policyText : null"
          />
        </TabsContent>
      </Tabs>
    </template>

    <Dialog v-model:open="ruleOpen">
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {{ ruleIndex === null ? t('acls.ruleDialog.newTitle') : t('acls.ruleDialog.editTitle') }}
          </DialogTitle>
          <DialogDescription>
            <i18n-t keypath="acls.ruleDialog.description" tag="span">
              <template #tag>
                <code>tag:</code>
              </template>
              <template #group>
                <code>group:</code>
              </template>
              <template #any>
                <code>*</code>
              </template>
            </i18n-t>
          </DialogDescription>
        </DialogHeader>

        <div class="space-y-3">
          <div class="space-y-2">
            <Label for="rule-src">{{ t('acls.fields.sources') }}</Label>
            <Input id="rule-src" v-model="ruleDraft.src[0]" placeholder="group:eng, tag:prod" />
          </div>
          <div class="space-y-2">
            <Label for="rule-dst">{{ t('acls.fields.destinations') }}</Label>
            <Input id="rule-dst" v-model="ruleDraft.dst[0]" placeholder="tag:web:80,443" />
            <p class="text-muted-foreground text-xs">
              <i18n-t keypath="acls.ruleDialog.portHint" tag="span">
                <template #port>
                  <code>:*</code>
                </template>
              </i18n-t>
            </p>
          </div>
          <div class="space-y-2">
            <Label for="rule-proto">
              {{ t('acls.fields.protocol') }}
              <span class="text-muted-foreground">({{ t('common.optional') }})</span>
            </Label>
            <Input id="rule-proto" v-model="ruleDraft.proto" placeholder="tcp" />
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" @click="ruleOpen = false">{{ t('common.cancel') }}</Button>
          <Button :disabled="!ruleDraft.src.join() || !ruleDraft.dst.join()" @click="saveRule">
            {{ t('acls.ruleDialog.save') }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <Dialog v-model:open="sshOpen">
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {{ sshIndex === null ? t('acls.sshDialog.newTitle') : t('acls.sshDialog.editTitle') }}
          </DialogTitle>
          <DialogDescription>{{ t('acls.sshDialog.description') }}</DialogDescription>
        </DialogHeader>

        <div class="space-y-3">
          <div class="space-y-2">
            <Label for="ssh-action">{{ t('acls.fields.action') }}</Label>
            <Input id="ssh-action" v-model="sshDraft.action" placeholder="accept" />
          </div>
          <div class="space-y-2">
            <Label for="ssh-src">{{ t('acls.fields.sources') }}</Label>
            <Input id="ssh-src" v-model="sshDraft.src[0]" placeholder="group:ops" />
          </div>
          <div class="space-y-2">
            <Label for="ssh-dst">{{ t('acls.fields.destinations') }}</Label>
            <Input id="ssh-dst" v-model="sshDraft.dst[0]" placeholder="tag:server" />
          </div>
          <div class="space-y-2">
            <Label for="ssh-users">{{ t('acls.fields.sshUsers') }}</Label>
            <Input id="ssh-users" v-model="sshDraft.users[0]" placeholder="root, autogroup:nonroot" />
          </div>
          <div v-if="sshDraft.action === 'check'" class="space-y-2">
            <Label for="ssh-period">{{ t('acls.fields.checkPeriod') }}</Label>
            <Input id="ssh-period" v-model="sshDraft.checkPeriod" placeholder="12h" />
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" @click="sshOpen = false">{{ t('common.cancel') }}</Button>
          <Button
            :disabled="!sshDraft.src.join() || !sshDraft.dst.join() || !sshDraft.users.join()"
            @click="saveSsh"
          >
            {{ t('acls.sshDialog.save') }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <Dialog v-model:open="hostOpen">
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {{ hostIndex ? t('acls.hostDialog.editTitle') : t('acls.hostDialog.newTitle') }}
          </DialogTitle>
          <DialogDescription>{{ t('acls.hostDialog.description') }}</DialogDescription>
        </DialogHeader>

        <div class="space-y-3">
          <div class="space-y-2">
            <Label for="host-name">{{ t('acls.fields.name') }}</Label>
            <Input id="host-name" v-model="hostDraft.name" placeholder="git" />
          </div>
          <div class="space-y-2">
            <Label for="host-value">{{ t('acls.fields.address') }}</Label>
            <Input id="host-value" v-model="hostDraft.value" placeholder="100.64.0.0/24" />
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" @click="hostOpen = false">{{ t('common.cancel') }}</Button>
          <Button :disabled="!hostDraft.name.trim() || !hostDraft.value.trim()" @click="saveHost">
            {{ t('common.save') }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <Dialog v-model:open="listOpen">
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{{ listDialogTitle }}</DialogTitle>
          <DialogDescription>
            {{
              listKind === 'group'
                ? t('acls.listDialog.groupDescription')
                : t('acls.listDialog.tagDescription')
            }}
          </DialogDescription>
        </DialogHeader>

        <div class="space-y-3">
          <div class="space-y-2">
            <Label for="list-name">{{ t('acls.fields.name') }}</Label>
            <Input id="list-name" v-model="listDraft.name" />
          </div>
          <div class="space-y-2">
            <Label for="list-members">
              {{ listKind === 'group' ? t('acls.fields.members') : t('acls.fields.owners') }}
            </Label>
            <Input
              id="list-members"
              v-model="listDraft.members"
              :placeholder="listKind === 'group' ? 'alice@, bob@' : 'group:ops'"
            />
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" @click="listOpen = false">{{ t('common.cancel') }}</Button>
          <Button :disabled="!listDraft.name.trim()" @click="saveList">{{ t('common.save') }}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>
