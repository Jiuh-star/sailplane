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
import { Switch } from '@/components/ui/switch'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Textarea } from '@/components/ui/textarea'

// CodeMirror is a few hundred kilobytes; only the ACL page needs it.
const JsonEditor = defineAsyncComponent(() => import('@/components/shared/JsonEditor.vue'))
const JsonDiff = defineAsyncComponent(() => import('@/components/shared/JsonDiff.vue'))
import { useSession } from '@/composables/useSession'
import { errorMessage, useToast } from '@/composables/useToast'
import {
  api,
  type AclResponse,
  type AclRule,
  type AutoApprovers,
  type GrantRule,
  type NodeAttr,
  type ParsedPolicy,
  type SshRule,
} from '@/lib/api'
import {
  isPlainObject,
  parseApproverExitNodes,
  parseApproverRoutes,
  parseNodeAttrs,
  parsePostures,
} from '@/lib/policy'

const { t } = useI18n()
const toast = useToast()
const { config } = useSession()

// Older servers omit the flag; when missing, keep grants editable.
const grantsSupported = computed(() => config.value.grantsSupported !== false)

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

// Comma-separated posture lists, kept apart from the rule drafts because the
// input holds text and the rule holds an array.
const rulePosture = ref('')
const sshPosture = ref('')

// --- Grant dialogs ---
const grantOpen = ref(false)
const grantIndex = ref<number | null>(null)
const grantDraft = ref({ src: '', dst: '', ip: '', srcPosture: '' })

// --- Advanced editors ---
const routeOpen = ref(false)
const routeIndex = ref<string | null>(null)
const routeDraft = ref({ cidr: '', selectors: '' })

const exitOpen = ref(false)
const exitDraft = ref('')

const nodeAttrOpen = ref(false)
const nodeAttrIndex = ref<number | null>(null)
const nodeAttrDraft = ref({ target: '', attr: '' })

const postureOpen = ref(false)
const postureIndex = ref<string | null>(null)
const postureDraft = ref({ name: '', conditions: '' })

const testsText = ref('')
const sshTestsText = ref('')
const testsError = ref<string | null>(null)
const sshTestsError = ref<string | null>(null)

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
    testsText.value = response.parsed?.tests ? JSON.stringify(response.parsed.tests, null, 2) : ''
    sshTestsText.value = response.parsed?.sshTests
      ? JSON.stringify(response.parsed.sshTests, null, 2)
      : ''
    testsError.value = null
    sshTestsError.value = null
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

const grants = computed<GrantRule[]>(() => {
  const value = parsed.value?.grants
  return Array.isArray(value) ? value : []
})

/** True when `grants` exists but is not the structured array the editor needs. */
const grantsRaw = computed(() => {
  const value = parsed.value?.grants
  return value !== undefined && !Array.isArray(value)
})

const autoApproverRoutes = computed(() => parseApproverRoutes(parsed.value?.autoApprovers))
const autoApproverExitNodes = computed(() => parseApproverExitNodes(parsed.value?.autoApprovers))
const nodeAttrs = computed(() => parseNodeAttrs(parsed.value?.nodeAttrs))
const postures = computed(() => parsePostures(parsed.value?.postures))

const randomizeClientPort = computed(() => parsed.value?.randomizeClientPort === true)

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
    if (result.warnings?.length) {
      toast.warning(t('acls.warnings.title'), result.warnings.join(' '))
    }
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
  if (index === null) {
    ruleDraft.value = { action: 'accept', src: [], dst: [], proto: '' }
    rulePosture.value = ''
  } else {
    const rule = acls.value[index]!
    ruleDraft.value = { ...rule, src: [...rule.src], dst: [...rule.dst] }
    rulePosture.value = (rule.srcPosture ?? []).join(', ')
  }
  ruleOpen.value = true
}

function saveRule() {
  if (!parsed.value) return
  // Spread the original rule, so unknown fields survive the edit.
  const base: AclRule =
    ruleIndex.value === null
      ? { action: 'accept', src: [], dst: [] }
      : { ...acls.value[ruleIndex.value]! }
  const rule: AclRule = {
    ...base,
    action: ruleDraft.value.action || 'accept',
    src: splitList(ruleDraft.value.src.join(',')),
    dst: splitList(ruleDraft.value.dst.join(',')).map(withDefaultPort),
  }
  const proto = ruleDraft.value.proto?.trim()
  if (proto) rule.proto = proto
  else delete rule.proto
  const posture = splitList(rulePosture.value)
  if (posture.length) rule.srcPosture = posture
  else delete rule.srcPosture

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
  if (index === null) {
    sshDraft.value = { action: 'accept', src: [], dst: [], users: [], checkPeriod: '' }
    sshPosture.value = ''
  } else {
    const rule = sshRules.value[index]!
    sshDraft.value = {
      ...rule,
      src: [...rule.src],
      dst: [...rule.dst],
      users: [...rule.users],
    }
    sshPosture.value = (rule.srcPosture ?? []).join(', ')
  }
  sshOpen.value = true
}

function saveSsh() {
  if (!parsed.value) return
  // Spread the original rule, so unknown fields survive the edit.
  const base: SshRule =
    sshIndex.value === null
      ? { action: 'accept', src: [], dst: [], users: [] }
      : { ...sshRules.value[sshIndex.value]! }
  const rule: SshRule = {
    ...base,
    action: sshDraft.value.action || 'accept',
    src: splitList(sshDraft.value.src.join(',')),
    dst: splitList(sshDraft.value.dst.join(',')),
    users: splitList(sshDraft.value.users.join(',')),
  }
  const period = sshDraft.value.checkPeriod?.trim()
  if (rule.action === 'check' && period) rule.checkPeriod = period
  else delete rule.checkPeriod
  const posture = splitList(sshPosture.value)
  if (posture.length) rule.srcPosture = posture
  else delete rule.srcPosture

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

// --- Grants ---

function setGrants(next: GrantRule[]) {
  if (!parsed.value) return
  const value = { ...parsed.value }
  if (next.length) value.grants = next
  else delete value.grants
  parsed.value = value
}

function editGrant(index: number | null) {
  grantIndex.value = index
  if (index === null) {
    grantDraft.value = { src: '', dst: '', ip: '', srcPosture: '' }
  } else {
    const rule = grants.value[index]!
    grantDraft.value = {
      src: rule.src.join(', '),
      dst: rule.dst.join(', '),
      ip: (rule.ip ?? []).join(', '),
      srcPosture: (rule.srcPosture ?? []).join(', '),
    }
  }
  grantOpen.value = true
}

function saveGrant() {
  if (!parsed.value) return
  const base: GrantRule =
    grantIndex.value === null ? { src: [], dst: [] } : { ...grants.value[grantIndex.value]! }
  const rule: GrantRule = {
    ...base,
    src: splitList(grantDraft.value.src),
    dst: splitList(grantDraft.value.dst),
  }
  const ip = splitList(grantDraft.value.ip)
  if (ip.length) rule.ip = ip
  else delete rule.ip
  const posture = splitList(grantDraft.value.srcPosture)
  if (posture.length) rule.srcPosture = posture
  else delete rule.srcPosture

  const next = [...grants.value]
  if (grantIndex.value === null) next.push(rule)
  else next[grantIndex.value] = rule
  setGrants(next)
  grantOpen.value = false
}

function removeGrant(index: number) {
  setGrants(grants.value.filter((_, i) => i !== index))
}

// --- Advanced: auto-approvers ---

function updateAutoApprovers(mutate: (draft: AutoApprovers) => void) {
  if (!parsed.value) return
  const current = isPlainObject(parsed.value.autoApprovers)
    ? (parsed.value.autoApprovers as AutoApprovers)
    : {}
  const draft: AutoApprovers = { ...current }
  draft.routes = isPlainObject(draft.routes)
    ? { ...(draft.routes as Record<string, string[]>) }
    : {}
  draft.exitNode = Array.isArray(draft.exitNode) ? [...draft.exitNode] : []
  mutate(draft)
  if (draft.routes && !Object.keys(draft.routes).length) delete draft.routes
  if (draft.exitNode && !draft.exitNode.length) delete draft.exitNode

  const value = { ...parsed.value }
  // Omit the section entirely once its last entry is gone.
  if (Object.keys(draft).length) value.autoApprovers = draft
  else delete value.autoApprovers
  parsed.value = value
}

function editApproverRoute(cidr: string | null) {
  routeIndex.value = cidr
  routeDraft.value = cidr
    ? {
        cidr,
        selectors: (
          autoApproverRoutes.value.find(([key]) => key === cidr)?.[1] ?? []
        ).join(', '),
      }
    : { cidr: '', selectors: '' }
  routeOpen.value = true
}

function saveApproverRoute() {
  updateAutoApprovers((draft) => {
    draft.routes = { ...(draft.routes ?? {}) }
    const cidr = routeDraft.value.cidr.trim()
    if (routeIndex.value && routeIndex.value !== cidr) delete draft.routes[routeIndex.value]
    draft.routes[cidr] = splitList(routeDraft.value.selectors)
  })
  routeOpen.value = false
}

function removeApproverRoute(cidr: string) {
  updateAutoApprovers((draft) => {
    draft.routes = { ...(draft.routes ?? {}) }
    delete draft.routes[cidr]
  })
}

function editExitNodes() {
  exitDraft.value = autoApproverExitNodes.value.join(', ')
  exitOpen.value = true
}

function saveExitNodes() {
  updateAutoApprovers((draft) => {
    draft.exitNode = splitList(exitDraft.value)
  })
  exitOpen.value = false
}

// --- Advanced: node attributes ---

function setNodeAttrs(next: NodeAttr[]) {
  if (!parsed.value) return
  const value = { ...parsed.value }
  if (next.length) value.nodeAttrs = next
  else delete value.nodeAttrs
  parsed.value = value
}

function editNodeAttr(index: number | null) {
  nodeAttrIndex.value = index
  if (index === null) {
    nodeAttrDraft.value = { target: '', attr: '' }
  } else {
    const entry = nodeAttrs.value[index]!
    nodeAttrDraft.value = { target: entry.target.join(', '), attr: entry.attr.join(', ') }
  }
  nodeAttrOpen.value = true
}

function saveNodeAttr() {
  if (!parsed.value) return
  const base: NodeAttr =
    nodeAttrIndex.value === null
      ? { target: [], attr: [] }
      : { ...nodeAttrs.value[nodeAttrIndex.value]! }
  const entry: NodeAttr = {
    ...base,
    target: splitList(nodeAttrDraft.value.target),
    attr: splitList(nodeAttrDraft.value.attr),
  }
  const next = [...nodeAttrs.value]
  if (nodeAttrIndex.value === null) next.push(entry)
  else next[nodeAttrIndex.value] = entry
  setNodeAttrs(next)
  nodeAttrOpen.value = false
}

function removeNodeAttr(index: number) {
  setNodeAttrs(nodeAttrs.value.filter((_, i) => i !== index))
}

// --- Advanced: postures ---

function setPostures(next: Record<string, string[]>) {
  if (!parsed.value) return
  const value = { ...parsed.value }
  if (Object.keys(next).length) value.postures = next
  else delete value.postures
  parsed.value = value
}

function editPosture(name: string | null) {
  postureIndex.value = name
  const source = parsed.value?.postures
  postureDraft.value = name
    ? {
        name,
        conditions: (
          isPlainObject(source) ? (source[name] as string[] | undefined) : undefined
        )?.join(', ') ?? '',
      }
    : { name: '', conditions: '' }
  postureOpen.value = true
}

function savePosture() {
  if (!parsed.value) return
  const source = isPlainObject(parsed.value.postures)
    ? { ...(parsed.value.postures as Record<string, string[]>) }
    : {}
  const name = postureDraft.value.name.trim()
  if (postureIndex.value && postureIndex.value !== name) delete source[postureIndex.value]
  source[name] = splitList(postureDraft.value.conditions)
  setPostures(source)
  postureOpen.value = false
}

function removePosture(name: string) {
  const source = isPlainObject(parsed.value?.postures)
    ? { ...(parsed.value!.postures as Record<string, string[]>) }
    : {}
  delete source[name]
  setPostures(source)
}

// --- Advanced: tests and randomizeClientPort ---

/** Parses a JSON array into `tests`/`sshTests`, or reports why it cannot. */
function applyJsonArray(
  key: 'tests' | 'sshTests',
  raw: string,
  setError: (value: string | null) => void
) {
  if (!parsed.value) return
  const trimmed = raw.trim()
  if (!trimmed) {
    const value = { ...parsed.value }
    delete value[key]
    parsed.value = value
    setError(null)
    return
  }
  let decoded: unknown
  try {
    decoded = JSON.parse(trimmed)
  } catch {
    setError(t('acls.advanced.invalidJson'))
    return
  }
  if (!Array.isArray(decoded)) {
    setError(t('acls.advanced.mustBeArray'))
    return
  }
  setError(null)
  const value = { ...parsed.value }
  if (decoded.length) value[key] = decoded
  else delete value[key]
  parsed.value = value
}

function updateTests(raw: string) {
  testsText.value = raw
  applyJsonArray('tests', raw, (error) => (testsError.value = error))
}

function updateSshTests(raw: string) {
  sshTestsText.value = raw
  applyJsonArray('sshTests', raw, (error) => (sshTestsError.value = error))
}

function setRandomizeClientPort(enabled: boolean) {
  if (!parsed.value) return
  const value = { ...parsed.value }
  if (enabled) value.randomizeClientPort = true
  else delete value.randomizeClientPort
  parsed.value = value
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
          <TabsTrigger v-if="grantsSupported" value="grants">{{ t('acls.tabs.grants') }}</TabsTrigger>
          <TabsTrigger value="advanced">{{ t('acls.tabs.advanced') }}</TabsTrigger>
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
                <Badge v-for="posture in rule.srcPosture ?? []" :key="posture" variant="secondary">
                  {{ posture }}
                </Badge>
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
                <Badge v-for="posture in rule.srcPosture ?? []" :key="posture" variant="secondary">
                  {{ posture }}
                </Badge>
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

        <TabsContent v-if="grantsSupported" value="grants" class="space-y-4 pt-4">
          <Alert v-if="grantsRaw" variant="warning">
            <AlertTitle>{{ t('acls.grants.rawTitle') }}</AlertTitle>
            <AlertDescription>{{ t('acls.grants.rawBody') }}</AlertDescription>
          </Alert>

          <Card>
            <CardHeader class="flex-row items-center justify-between">
              <CardTitle class="text-base">{{ t('acls.grants.title') }}</CardTitle>
              <Button
                size="sm"
                variant="outline"
                :disabled="!writable || grantsRaw"
                @click="editGrant(null)"
              >
                {{ t('acls.grants.add') }}
              </Button>
            </CardHeader>
            <CardContent class="space-y-2">
              <EmptyState v-if="!grants.length" :title="t('acls.grants.empty')" />
              <div
                v-for="(rule, index) in grants"
                :key="index"
                class="flex flex-wrap items-center gap-2 rounded-md border p-3 text-sm"
              >
                <Badge v-for="src in rule.src" :key="src" variant="secondary">{{ src }}</Badge>
                <span class="text-muted-foreground">→</span>
                <Badge v-for="dst in rule.dst" :key="dst" variant="outline">{{ dst }}</Badge>
                <Badge v-for="ip in rule.ip ?? []" :key="ip" variant="outline">{{ ip }}</Badge>
                <Badge v-for="posture in rule.srcPosture ?? []" :key="posture" variant="secondary">
                  {{ posture }}
                </Badge>
                <span class="ml-auto flex gap-1">
                  <Button size="sm" variant="ghost" :disabled="!writable" @click="editGrant(index)">
                    {{ t('common.edit') }}
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    class="text-destructive"
                    :disabled="!writable"
                    @click="removeGrant(index)"
                  >
                    {{ t('common.delete') }}
                  </Button>
                </span>
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="advanced" class="space-y-4 pt-4">
          <Card>
            <CardHeader>
              <CardTitle class="text-base">{{ t('acls.advanced.autoApprovers.title') }}</CardTitle>
              <p class="text-muted-foreground text-sm">
                {{ t('acls.advanced.autoApprovers.description') }}
              </p>
            </CardHeader>
            <CardContent class="space-y-4">
              <div class="space-y-2">
                <div class="flex items-center justify-between">
                  <p class="text-sm font-medium">
                    {{ t('acls.advanced.autoApprovers.routes') }}
                  </p>
                  <Button
                    size="sm"
                    variant="outline"
                    :disabled="!writable"
                    @click="editApproverRoute(null)"
                  >
                    {{ t('acls.advanced.autoApprovers.addRoute') }}
                  </Button>
                </div>
                <EmptyState
                  v-if="!autoApproverRoutes.length"
                  :title="t('acls.advanced.autoApprovers.noRoutes')"
                />
                <div
                  v-for="[cidr, routeSelectors] in autoApproverRoutes"
                  :key="cidr"
                  class="flex flex-wrap items-center gap-2 rounded-md border p-3 text-sm"
                >
                  <code class="text-xs font-medium">{{ cidr }}</code>
                  <Badge v-for="selector in routeSelectors" :key="selector" variant="secondary">
                    {{ selector }}
                  </Badge>
                  <span class="ml-auto flex gap-1">
                    <Button
                      size="sm"
                      variant="ghost"
                      :disabled="!writable"
                      @click="editApproverRoute(cidr)"
                    >
                      {{ t('common.edit') }}
                    </Button>
                    <Button
                      size="sm"
                      variant="ghost"
                      class="text-destructive"
                      :disabled="!writable"
                      @click="removeApproverRoute(cidr)"
                    >
                      {{ t('common.delete') }}
                    </Button>
                  </span>
                </div>
              </div>

              <div class="space-y-2">
                <div class="flex items-center justify-between">
                  <p class="text-sm font-medium">
                    {{ t('acls.advanced.autoApprovers.exitNodes') }}
                  </p>
                  <Button size="sm" variant="outline" :disabled="!writable" @click="editExitNodes">
                    {{ t('common.edit') }}
                  </Button>
                </div>
                <div class="flex flex-wrap items-center gap-2 rounded-md border p-3 text-sm">
                  <Badge v-for="selector in autoApproverExitNodes" :key="selector" variant="secondary">
                    {{ selector }}
                  </Badge>
                  <span v-if="!autoApproverExitNodes.length" class="text-muted-foreground text-xs">
                    {{ t('acls.advanced.autoApprovers.noExitNodes') }}
                  </span>
                </div>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader class="flex-row items-center justify-between">
              <CardTitle class="text-base">{{ t('acls.advanced.nodeAttrs.title') }}</CardTitle>
              <Button
                size="sm"
                variant="outline"
                :disabled="!writable"
                @click="editNodeAttr(null)"
              >
                {{ t('acls.advanced.nodeAttrs.add') }}
              </Button>
            </CardHeader>
            <CardContent class="space-y-2">
              <EmptyState v-if="!nodeAttrs.length" :title="t('acls.advanced.nodeAttrs.empty')" />
              <div
                v-for="(entry, index) in nodeAttrs"
                :key="index"
                class="flex flex-wrap items-center gap-2 rounded-md border p-3 text-sm"
              >
                <Badge v-for="target in entry.target" :key="target" variant="secondary">
                  {{ target }}
                </Badge>
                <span class="text-muted-foreground">→</span>
                <Badge v-for="attr in entry.attr" :key="attr" variant="outline">{{ attr }}</Badge>
                <span class="ml-auto flex gap-1">
                  <Button size="sm" variant="ghost" :disabled="!writable" @click="editNodeAttr(index)">
                    {{ t('common.edit') }}
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    class="text-destructive"
                    :disabled="!writable"
                    @click="removeNodeAttr(index)"
                  >
                    {{ t('common.delete') }}
                  </Button>
                </span>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader class="flex-row items-center justify-between">
              <CardTitle class="text-base">{{ t('acls.advanced.postures.title') }}</CardTitle>
              <Button
                size="sm"
                variant="outline"
                :disabled="!writable"
                @click="editPosture(null)"
              >
                {{ t('acls.advanced.postures.add') }}
              </Button>
            </CardHeader>
            <CardContent class="space-y-2">
              <EmptyState v-if="!postures.length" :title="t('acls.advanced.postures.empty')" />
              <div
                v-for="[name, conditions] in postures"
                :key="name"
                class="flex flex-wrap items-center gap-2 rounded-md border p-3 text-sm"
              >
                <span class="font-medium">{{ name }}</span>
                <Badge v-for="condition in conditions" :key="condition" variant="secondary">
                  {{ condition }}
                </Badge>
                <span class="ml-auto flex gap-1">
                  <Button size="sm" variant="ghost" :disabled="!writable" @click="editPosture(name)">
                    {{ t('common.edit') }}
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    class="text-destructive"
                    :disabled="!writable"
                    @click="removePosture(name)"
                  >
                    {{ t('common.delete') }}
                  </Button>
                </span>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle class="text-base">{{ t('acls.advanced.tests.title') }}</CardTitle>
              <p class="text-muted-foreground text-sm">{{ t('acls.advanced.tests.hint') }}</p>
            </CardHeader>
            <CardContent class="space-y-2">
              <JsonEditor
                :model-value="testsText"
                :readonly="!writable"
                min-height="12rem"
                @update:model-value="updateTests"
              />
              <p v-if="testsError" class="text-destructive text-xs">{{ testsError }}</p>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle class="text-base">{{ t('acls.advanced.sshTests.title') }}</CardTitle>
              <p class="text-muted-foreground text-sm">{{ t('acls.advanced.sshTests.hint') }}</p>
            </CardHeader>
            <CardContent class="space-y-2">
              <JsonEditor
                :model-value="sshTestsText"
                :readonly="!writable"
                min-height="12rem"
                @update:model-value="updateSshTests"
              />
              <p v-if="sshTestsError" class="text-destructive text-xs">{{ sshTestsError }}</p>
            </CardContent>
          </Card>

          <Card>
            <CardContent class="flex items-center justify-between gap-4 py-4">
              <div class="space-y-1">
                <Label for="randomize-client-port">
                  {{ t('acls.advanced.randomize.title') }}
                </Label>
                <p class="text-muted-foreground text-xs">
                  {{ t('acls.advanced.randomize.description') }}
                </p>
              </div>
              <Switch
                id="randomize-client-port"
                :model-value="randomizeClientPort"
                :disabled="!writable"
                @update:model-value="setRandomizeClientPort"
              />
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
          <div class="space-y-2">
            <Label for="rule-posture">
              {{ t('acls.fields.srcPosture') }}
              <span class="text-muted-foreground">({{ t('common.optional') }})</span>
            </Label>
            <Input id="rule-posture" v-model="rulePosture" placeholder="posture:corp" />
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
          <div class="space-y-2">
            <Label for="ssh-posture">
              {{ t('acls.fields.srcPosture') }}
              <span class="text-muted-foreground">({{ t('common.optional') }})</span>
            </Label>
            <Input id="ssh-posture" v-model="sshPosture" placeholder="posture:corp" />
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

    <Dialog v-model:open="grantOpen">
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {{
              grantIndex === null ? t('acls.grantDialog.newTitle') : t('acls.grantDialog.editTitle')
            }}
          </DialogTitle>
          <DialogDescription>{{ t('acls.grantDialog.description') }}</DialogDescription>
        </DialogHeader>

        <div class="space-y-3">
          <div class="space-y-2">
            <Label for="grant-src">{{ t('acls.fields.sources') }}</Label>
            <Input id="grant-src" v-model="grantDraft.src" placeholder="group:eng, tag:prod" />
          </div>
          <div class="space-y-2">
            <Label for="grant-dst">{{ t('acls.fields.destinations') }}</Label>
            <Input id="grant-dst" v-model="grantDraft.dst" placeholder="tag:web" />
          </div>
          <div class="space-y-2">
            <Label for="grant-ip">
              {{ t('acls.fields.ip') }}
              <span class="text-muted-foreground">({{ t('common.optional') }})</span>
            </Label>
            <Input id="grant-ip" v-model="grantDraft.ip" placeholder="tcp:443, udp:53" />
          </div>
          <div class="space-y-2">
            <Label for="grant-posture">
              {{ t('acls.fields.srcPosture') }}
              <span class="text-muted-foreground">({{ t('common.optional') }})</span>
            </Label>
            <Input id="grant-posture" v-model="grantDraft.srcPosture" placeholder="posture:corp" />
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" @click="grantOpen = false">{{ t('common.cancel') }}</Button>
          <Button
            :disabled="!grantDraft.src.trim() || !grantDraft.dst.trim()"
            @click="saveGrant"
          >
            {{ t('acls.grantDialog.save') }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <Dialog v-model:open="routeOpen">
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {{
              routeIndex === null
                ? t('acls.approverRouteDialog.newTitle')
                : t('acls.approverRouteDialog.editTitle')
            }}
          </DialogTitle>
          <DialogDescription>{{ t('acls.approverRouteDialog.description') }}</DialogDescription>
        </DialogHeader>

        <div class="space-y-3">
          <div class="space-y-2">
            <Label for="route-cidr">{{ t('acls.fields.cidr') }}</Label>
            <Input id="route-cidr" v-model="routeDraft.cidr" placeholder="10.0.0.0/8" />
          </div>
          <div class="space-y-2">
            <Label for="route-selectors">{{ t('acls.fields.selectors') }}</Label>
            <Input id="route-selectors" v-model="routeDraft.selectors" placeholder="tag:router" />
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" @click="routeOpen = false">{{ t('common.cancel') }}</Button>
          <Button
            :disabled="!routeDraft.cidr.trim() || !routeDraft.selectors.trim()"
            @click="saveApproverRoute"
          >
            {{ t('common.save') }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <Dialog v-model:open="exitOpen">
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{{ t('acls.exitNodesDialog.title') }}</DialogTitle>
          <DialogDescription>{{ t('acls.exitNodesDialog.description') }}</DialogDescription>
        </DialogHeader>

        <div class="space-y-2">
          <Label for="exit-nodes">{{ t('acls.fields.selectors') }}</Label>
          <Textarea id="exit-nodes" v-model="exitDraft" placeholder="tag:exit" />
        </div>

        <DialogFooter>
          <Button variant="outline" @click="exitOpen = false">{{ t('common.cancel') }}</Button>
          <Button @click="saveExitNodes">{{ t('common.save') }}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <Dialog v-model:open="nodeAttrOpen">
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {{
              nodeAttrIndex === null
                ? t('acls.nodeAttrDialog.newTitle')
                : t('acls.nodeAttrDialog.editTitle')
            }}
          </DialogTitle>
          <DialogDescription>{{ t('acls.nodeAttrDialog.description') }}</DialogDescription>
        </DialogHeader>

        <div class="space-y-3">
          <div class="space-y-2">
            <Label for="node-attr-target">{{ t('acls.fields.targets') }}</Label>
            <Input id="node-attr-target" v-model="nodeAttrDraft.target" placeholder="tag:server" />
          </div>
          <div class="space-y-2">
            <Label for="node-attr-attr">{{ t('acls.fields.attributes') }}</Label>
            <Input id="node-attr-attr" v-model="nodeAttrDraft.attr" placeholder="attr:admin" />
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" @click="nodeAttrOpen = false">{{ t('common.cancel') }}</Button>
          <Button
            :disabled="!nodeAttrDraft.target.trim() || !nodeAttrDraft.attr.trim()"
            @click="saveNodeAttr"
          >
            {{ t('common.save') }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <Dialog v-model:open="postureOpen">
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {{
              postureIndex === null
                ? t('acls.postureDialog.newTitle')
                : t('acls.postureDialog.editTitle')
            }}
          </DialogTitle>
          <DialogDescription>{{ t('acls.postureDialog.description') }}</DialogDescription>
        </DialogHeader>

        <div class="space-y-3">
          <div class="space-y-2">
            <Label for="posture-name">{{ t('acls.fields.name') }}</Label>
            <Input id="posture-name" v-model="postureDraft.name" placeholder="posture:corp" />
          </div>
          <div class="space-y-2">
            <Label for="posture-conditions">{{ t('acls.fields.conditions') }}</Label>
            <Input
              id="posture-conditions"
              v-model="postureDraft.conditions"
              placeholder="node:os == linux, node:tsVersion >= 1.60"
            />
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" @click="postureOpen = false">{{ t('common.cancel') }}</Button>
          <Button
            :disabled="!postureDraft.name.trim()"
            @click="savePosture"
          >
            {{ t('common.save') }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>
