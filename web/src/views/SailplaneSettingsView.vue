<script setup lang="ts">
/**
 * Edits Sailplane's own configuration, which lives in its database. Secret
 * values are write-only: the API never returns them, so a stored secret shows
 * as "unchanged" and the UI can only replace or clear it.
 */
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { ArrowLeft, CircleCheck, LoaderCircle, Save, Trash2, TriangleAlert } from '@lucide/vue'

import PageHeader from '@/components/shared/PageHeader.vue'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'
import { errorMessage, useToast } from '@/composables/useToast'
import { api, type SettingsEntry } from '@/lib/api'

const { t } = useI18n()
const toast = useToast()

const GROUP_ORDER = ['server', 'headscale', 'oidc', 'integration', 'agent', 'ssh', 'advanced']

const entries = ref<SettingsEntry[]>([])
const loading = ref(true)
const failure = ref<string | null>(null)

// Editable drafts, keyed by dotted setting key.
const text = ref<Record<string, string>>({})
const bools = ref<Record<string, boolean>>({})
const secrets = ref<Record<string, string>>({})
const touched = ref<Record<string, boolean>>({})
const clearedSecrets = ref<Record<string, boolean>>({})

const saving = ref(false)
const validating = ref(false)
const validateResult = ref<{ ok: boolean; message: string } | null>(null)

const yaml = ref('')
const importing = ref(false)

const groups = computed(() => {
  const map = new Map<string, SettingsEntry[]>()
  for (const entry of entries.value) {
    const list = map.get(entry.group)
    if (list) list.push(entry)
    else map.set(entry.group, [entry])
  }
  return [...map.entries()]
    .map(([group, items]) => ({ group, items }))
    .sort((a, b) => order(a.group) - order(b.group))
})

/** Known groups sort first, in schema order; anything else keeps a stable tail. */
function order(group: string): number {
  const index = GROUP_ORDER.indexOf(group)
  return index === -1 ? GROUP_ORDER.length : index
}

function groupLabel(group: string): string {
  const key = `settings.groups.${group}`
  const label = t(key)
  return label === key ? group : label
}

function toInput(entry: SettingsEntry): string {
  if (entry.value === null || entry.value === undefined) return ''
  if (entry.kind === 'list') {
    return Array.isArray(entry.value) ? entry.value.join(', ') : String(entry.value)
  }
  return String(entry.value)
}

function resetDrafts() {
  const nextText: Record<string, string> = {}
  const nextBools: Record<string, boolean> = {}
  const nextSecrets: Record<string, string> = {}
  for (const entry of entries.value) {
    if (entry.secret) {
      nextSecrets[entry.key] = ''
    } else if (entry.kind === 'bool') {
      nextBools[entry.key] = Boolean(entry.value)
    } else {
      nextText[entry.key] = toInput(entry)
    }
  }
  text.value = nextText
  bools.value = nextBools
  secrets.value = nextSecrets
  touched.value = {}
  clearedSecrets.value = {}
  validateResult.value = null
}

async function load() {
  loading.value = true
  try {
    const response = await api.settings.get()
    entries.value = response.settings
    resetDrafts()
    failure.value = null
  } catch (err) {
    failure.value = errorMessage(err)
  } finally {
    loading.value = false
  }
}

onMounted(load)

/** An environment value wins over any stored value, so the UI cannot edit it. */
function locked(entry: SettingsEntry): boolean {
  return entry.source === 'environment'
}

function touch(key: string) {
  touched.value[key] = true
}

function touchSecret(key: string) {
  touched.value[key] = true
  clearedSecrets.value[key] = false
}

function setBool(key: string, value: boolean) {
  bools.value[key] = value
  touch(key)
}

function clearSecret(key: string) {
  secrets.value[key] = ''
  clearedSecrets.value[key] = true
  touch(key)
}

function sourceBadge(entry: SettingsEntry) {
  if (entry.source === 'environment') {
    return { variant: 'warning' as const, label: t('settings.deploymentSettings.sourceEnvironment') }
  }
  if (entry.source === 'database') {
    return { variant: 'secondary' as const, label: t('settings.deploymentSettings.sourceDatabase') }
  }
  return null
}

function coerce(entry: SettingsEntry): unknown {
  if (entry.kind === 'bool') return bools.value[entry.key]
  const raw = text.value[entry.key] ?? ''
  if (entry.kind === 'list') {
    return raw
      .split(',')
      .map((part) => part.trim())
      .filter(Boolean)
  }
  if (entry.kind === 'number') {
    if (raw.trim() === '') return null
    const value = Number(raw)
    return Number.isNaN(value) ? raw : value
  }
  return raw
}

/** Collects only the fields the user touched. A cleared secret sends null. */
function pendingValues(): Record<string, unknown> {
  const values: Record<string, unknown> = {}
  for (const entry of entries.value) {
    if (locked(entry)) continue
    if (entry.secret) {
      if (clearedSecrets.value[entry.key]) {
        values[entry.key] = null
        continue
      }
      const draft = (secrets.value[entry.key] ?? '').trim()
      if (draft) values[entry.key] = draft
      continue
    }
    if (!touched.value[entry.key]) continue
    values[entry.key] = coerce(entry)
  }
  return values
}

async function validate() {
  if (validating.value) return
  validating.value = true
  validateResult.value = null
  try {
    const result = await api.settings.validate(pendingValues())
    validateResult.value = result.valid
      ? { ok: true, message: t('settings.deploymentSettings.validateOk') }
      : {
          ok: false,
          message: result.error ?? t('settings.deploymentSettings.validateFailed'),
        }
  } catch (err) {
    validateResult.value = { ok: false, message: errorMessage(err) }
  } finally {
    validating.value = false
  }
}

async function save() {
  const values = pendingValues()
  if (!Object.keys(values).length) {
    toast.toast(t('settings.deploymentSettings.nothingToSave'))
    return
  }
  saving.value = true
  try {
    const result = await api.settings.update(values)
    if (result.restartRequired.length) {
      toast.warning(
        t('settings.deploymentSettings.restartTitle'),
        t('settings.deploymentSettings.restartBody', { keys: result.restartRequired.join(', ') }),
      )
    } else {
      toast.success(t('settings.deploymentSettings.saved'))
    }
    await load()
  } catch (err) {
    toast.error(t('settings.deploymentSettings.saveFailed'), errorMessage(err))
  } finally {
    saving.value = false
  }
}

async function runImport() {
  if (importing.value || !yaml.value.trim()) return
  importing.value = true
  try {
    await api.settings.import(yaml.value)
    toast.success(t('settings.deploymentSettings.import.done'))
    yaml.value = ''
    await load()
  } catch (err) {
    toast.error(t('settings.deploymentSettings.import.failed'), errorMessage(err))
  } finally {
    importing.value = false
  }
}
</script>

<template>
  <div class="space-y-6">
    <RouterLink
      :to="{ name: 'settings' }"
      class="text-muted-foreground hover:text-foreground inline-flex items-center gap-1 text-sm"
    >
      <ArrowLeft class="size-4" />
      {{ t('nav.settings') }}
    </RouterLink>

    <PageHeader
      :title="t('settings.deploymentSettings.title')"
      :description="t('settings.deploymentSettings.description')"
    >
      <template #actions>
        <Button variant="outline" :disabled="validating || saving" @click="validate">
          <LoaderCircle v-if="validating" class="animate-spin" />
          {{ validating ? t('settings.deploymentSettings.validating') : t('settings.deploymentSettings.validate') }}
        </Button>
        <Button :disabled="saving || validating" @click="save">
          <LoaderCircle v-if="saving" class="animate-spin" />
          <Save v-else />
          {{ t('common.save') }}
        </Button>
      </template>
    </PageHeader>

    <Alert v-if="failure" variant="destructive">
      <TriangleAlert />
      <AlertTitle>{{ t('settings.deploymentSettings.loadFailed') }}</AlertTitle>
      <AlertDescription>{{ failure }}</AlertDescription>
    </Alert>

    <Alert v-if="validateResult" :variant="validateResult.ok ? 'default' : 'destructive'">
      <CircleCheck v-if="validateResult.ok" />
      <TriangleAlert v-else />
      <AlertTitle>
        {{
          validateResult.ok
            ? t('settings.deploymentSettings.validateOk')
            : t('settings.deploymentSettings.validateFailed')
        }}
      </AlertTitle>
      <AlertDescription v-if="!validateResult.ok">{{ validateResult.message }}</AlertDescription>
    </Alert>

    <p v-if="loading" class="text-muted-foreground text-sm">{{ t('common.loading') }}</p>

    <p v-else-if="!entries.length && !failure" class="text-muted-foreground text-sm">
      {{ t('settings.deploymentSettings.empty') }}
    </p>

    <Card v-for="section in groups" :key="section.group">
      <CardHeader>
        <CardTitle class="text-base">{{ groupLabel(section.group) }}</CardTitle>
      </CardHeader>
      <CardContent class="space-y-5">
        <div v-for="entry in section.items" :key="entry.key" class="space-y-2">
          <div class="flex flex-wrap items-center gap-2">
            <Label :for="`setting-${entry.key}`" class="font-mono text-xs">
              {{ entry.key }}
            </Label>
            <Badge v-if="entry.restartRequired" variant="warning">
              {{ t('settings.deploymentSettings.restartRequired') }}
            </Badge>
            <Badge v-if="sourceBadge(entry)" :variant="sourceBadge(entry)?.variant">
              {{ sourceBadge(entry)?.label }}
            </Badge>
          </div>

          <!-- Secret: write-only. -->
          <template v-if="entry.secret">
            <div class="flex flex-wrap items-center gap-2">
              <Input
                :id="`setting-${entry.key}`"
                v-model="secrets[entry.key]"
                type="password"
                autocomplete="new-password"
                spellcheck="false"
                class="max-w-sm"
                :disabled="locked(entry)"
                :placeholder="
                  entry.set
                    ? t('settings.deploymentSettings.secretPlaceholderSet')
                    : t('settings.deploymentSettings.secretPlaceholderUnset')
                "
                @update:model-value="touchSecret(entry.key)"
              />
              <Button
                variant="outline"
                size="sm"
                :disabled="locked(entry) || (!entry.set && !clearedSecrets[entry.key])"
                @click="clearSecret(entry.key)"
              >
                <Trash2 />
                {{ t('settings.deploymentSettings.clear') }}
              </Button>
              <span v-if="clearedSecrets[entry.key]" class="text-destructive text-xs">
                {{ t('settings.deploymentSettings.cleared') }}
              </span>
            </div>
            <p class="text-muted-foreground text-xs">
              {{ t('settings.deploymentSettings.secretHint') }}
            </p>
          </template>

          <!-- Boolean. -->
          <Switch
            v-else-if="entry.kind === 'bool'"
            :id="`setting-${entry.key}`"
            :model-value="bools[entry.key]"
            :disabled="locked(entry)"
            @update:model-value="(value: boolean) => setBool(entry.key, value)"
          />

          <!-- Every other kind is a text-like input. -->
          <div v-else class="space-y-1">
            <Input
              :id="`setting-${entry.key}`"
              v-model="text[entry.key]"
              :type="entry.kind === 'number' ? 'number' : 'text'"
              autocomplete="off"
              spellcheck="false"
              class="max-w-md"
              :disabled="locked(entry)"
              @update:model-value="touch(entry.key)"
            />
            <p v-if="entry.kind === 'list'" class="text-muted-foreground text-xs">
              {{ t('settings.deploymentSettings.listHint') }}
            </p>
          </div>
        </div>
      </CardContent>
    </Card>

    <Card>
      <CardHeader>
        <CardTitle class="text-base">{{ t('settings.deploymentSettings.import.title') }}</CardTitle>
        <CardDescription>{{ t('settings.deploymentSettings.import.description') }}</CardDescription>
      </CardHeader>
      <CardContent class="space-y-3">
        <Textarea
          v-model="yaml"
          rows="8"
          spellcheck="false"
          class="font-mono text-xs"
          :placeholder="t('settings.deploymentSettings.import.placeholder')"
        />
        <Button :disabled="importing || !yaml.trim()" @click="runImport">
          <LoaderCircle v-if="importing" class="animate-spin" />
          {{ importing ? t('settings.deploymentSettings.import.importing') : t('settings.deploymentSettings.import.submit') }}
        </Button>
      </CardContent>
    </Card>
  </div>
</template>
