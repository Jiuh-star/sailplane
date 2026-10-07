<script setup lang="ts">
/**
 * Headscale's OpenID Connect provider settings, which decide how devices
 * authenticate to the control server. Distinct from the `oidc` block in
 * Sailplane's own config. The client secret is write-only.
 */
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { ArrowLeft, LoaderCircle } from '@lucide/vue'

import PageHeader from '@/components/shared/PageHeader.vue'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Skeleton } from '@/components/ui/skeleton'
import { Switch } from '@/components/ui/switch'
import { errorMessage, useToast } from '@/composables/useToast'
import { api, type OidcSettings } from '@/lib/api'

const { t } = useI18n()
const toast = useToast()

/// Defaults rather than null, so the template never narrows a ref.
const settings = ref<OidcSettings>({
  configured: false,
  issuer: '',
  clientId: '',
  clientSecretSet: false,
  clientSecretPath: '',
  scope: [],
  legacyExpiry: false,
  useExpiryFromToken: false,
  pkceEnabled: true,
  onlyStartIfAvailable: true,
})
const loaded = ref(false)
const writable = ref(false)
const available = ref(false)
const loading = ref(true)
const saving = ref(false)
const failure = ref<string | null>(null)

const issuer = ref('')
const clientId = ref('')
const clientSecret = ref('')
const scope = ref('')

const canWrite = computed(() => writable.value && !saving.value)

async function load() {
  try {
    const response = await api.oidc.get()
    settings.value = response.oidc
    writable.value = response.access.writable && response.access.write
    available.value = response.access.available
    issuer.value = response.oidc.issuer
    clientId.value = response.oidc.clientId
    scope.value = response.oidc.scope.join(' ')
    loaded.value = true
    failure.value = null
  } catch (err) {
    failure.value = errorMessage(err)
  } finally {
    loading.value = false
  }
}

onMounted(load)

async function save(patch: Record<string, unknown>, done: string) {
  if (!canWrite.value) return
  saving.value = true
  try {
    const response = await api.oidc.update(patch)
    toast.success(done)
    if (response.warning) toast.warning(t('oidc.warningTitle'), response.warning)
    await load()
  } catch (err) {
    toast.error(t('oidc.failedTitle'), errorMessage(err))
  } finally {
    saving.value = false
  }
}

function saveProvider() {
  void save(
    {
      issuer: issuer.value,
      client_id: clientId.value,
      scope: scope.value.split(/\s+/).filter(Boolean),
    },
    t('oidc.toast.saved'),
  )
}

function saveSecret() {
  if (!clientSecret.value) return
  void save({ client_secret: clientSecret.value }, t('oidc.toast.secretSaved')).then(() => {
    clientSecret.value = ''
  })
}

function clearSecret() {
  void save({ client_secret: '' }, t('oidc.toast.secretCleared'))
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

    <PageHeader :title="t('oidc.title')" :description="t('oidc.description')" />

    <Alert v-if="failure" variant="destructive">
      <AlertTitle>{{ t('oidc.loadFailed') }}</AlertTitle>
      <AlertDescription>{{ failure }}</AlertDescription>
    </Alert>

    <Alert v-else-if="!available" variant="warning">
      <AlertTitle>{{ t('oidc.unavailableTitle') }}</AlertTitle>
      <AlertDescription>{{ t('oidc.unavailableBody') }}</AlertDescription>
    </Alert>

    <Alert v-else-if="!writable" variant="warning">
      <AlertTitle>{{ t('oidc.readOnlyTitle') }}</AlertTitle>
      <AlertDescription>{{ t('oidc.readOnlyBody') }}</AlertDescription>
    </Alert>

    <div v-if="loading" class="space-y-2">
      <Skeleton v-for="index in 3" :key="index" class="h-32 w-full" />
    </div>

    <template v-else-if="loaded">
      <Alert v-if="settings.legacyExpiry" variant="warning">
        <AlertTitle>{{ t('oidc.legacyExpiry.title') }}</AlertTitle>
        <AlertDescription class="space-y-2">
          <p>{{ t('oidc.legacyExpiry.body') }}</p>
          <Button
            v-if="canWrite"
            size="sm"
            variant="outline"
            @click="save({ remove_legacy_expiry: true }, t('oidc.legacyExpiry.removed'))"
          >
            {{ t('oidc.legacyExpiry.action') }}
          </Button>
        </AlertDescription>
      </Alert>

      <Card>
        <CardHeader>
          <CardTitle class="flex items-center gap-2 text-base">
            {{ t('oidc.provider.title') }}
            <Badge :variant="settings.configured ? 'success' : 'secondary'">
              {{ settings.configured ? t('oidc.configured') : t('oidc.notConfigured') }}
            </Badge>
          </CardTitle>
          <CardDescription>{{ t('oidc.provider.description') }}</CardDescription>
        </CardHeader>
        <CardContent class="space-y-4">
          <div class="grid gap-4 sm:grid-cols-2">
            <div class="space-y-2">
              <Label for="oidc-issuer">{{ t('oidc.provider.issuer') }}</Label>
              <Input
                id="oidc-issuer"
                v-model="issuer"
                placeholder="https://idp.example.com"
                autocomplete="off"
                spellcheck="false"
                :disabled="!canWrite"
              />
            </div>
            <div class="space-y-2">
              <Label for="oidc-client-id">{{ t('oidc.provider.clientId') }}</Label>
              <Input
                id="oidc-client-id"
                v-model="clientId"
                autocomplete="off"
                spellcheck="false"
                :disabled="!canWrite"
              />
            </div>
            <div class="space-y-2">
              <Label for="oidc-scope">{{ t('oidc.provider.scope') }}</Label>
              <Input
                id="oidc-scope"
                v-model="scope"
                placeholder="openid profile email"
                autocomplete="off"
                spellcheck="false"
                :disabled="!canWrite"
              />
            </div>
          </div>

          <Button :disabled="!canWrite" @click="saveProvider">
            <LoaderCircle v-if="saving" class="animate-spin" />
            {{ t('common.save') }}
          </Button>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle class="text-base">{{ t('oidc.secret.title') }}</CardTitle>
          <CardDescription>{{ t('oidc.secret.description') }}</CardDescription>
        </CardHeader>
        <CardContent class="space-y-4">
          <div class="flex items-center gap-3 text-sm">
            <Badge :variant="settings.clientSecretSet ? 'success' : 'secondary'">
              {{
                settings.clientSecretSet
                  ? t('oidc.secret.present')
                  : t('oidc.secret.absent')
              }}
            </Badge>
            <span v-if="settings.clientSecretPath" class="text-muted-foreground font-mono text-xs">
              {{ t('oidc.secret.fromPath', { path: settings.clientSecretPath }) }}
            </span>
          </div>

          <p class="text-muted-foreground text-xs">{{ t('oidc.secret.hint') }}</p>

          <div class="flex flex-wrap gap-2">
            <Input
              v-model="clientSecret"
              type="password"
              autocomplete="new-password"
              class="max-w-sm"
              :placeholder="t('oidc.secret.placeholder')"
              :disabled="!canWrite"
            />
            <Button :disabled="!canWrite || !clientSecret" @click="saveSecret">
              {{ t('oidc.secret.replace') }}
            </Button>
            <Button
              variant="outline"
              :disabled="!canWrite || !settings.clientSecretSet"
              @click="clearSecret"
            >
              {{ t('oidc.secret.clear') }}
            </Button>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle class="text-base">{{ t('oidc.behaviour.title') }}</CardTitle>
          <CardDescription>{{ t('oidc.behaviour.description') }}</CardDescription>
        </CardHeader>
        <CardContent class="space-y-4">
          <div class="flex items-center justify-between gap-4">
            <div>
              <Label for="oidc-pkce">{{ t('oidc.behaviour.pkce') }}</Label>
              <p class="text-muted-foreground text-xs">{{ t('oidc.behaviour.pkceHint') }}</p>
            </div>
            <Switch
              id="oidc-pkce"
              :model-value="settings.pkceEnabled"
              :disabled="!canWrite"
              @update:model-value="
                (value: boolean) => save({ pkce_enabled: value }, t('oidc.toast.saved'))
              "
            />
          </div>

          <div class="flex items-center justify-between gap-4">
            <div>
              <Label for="oidc-token-expiry">{{ t('oidc.behaviour.useTokenExpiry') }}</Label>
              <p class="text-muted-foreground text-xs">
                {{ t('oidc.behaviour.useTokenExpiryHint') }}
              </p>
            </div>
            <Switch
              id="oidc-token-expiry"
              :model-value="settings.useExpiryFromToken"
              :disabled="!canWrite"
              @update:model-value="
                (value: boolean) => save({ use_expiry_from_token: value }, t('oidc.toast.saved'))
              "
            />
          </div>

          <div class="flex items-center justify-between gap-4">
            <div>
              <Label for="oidc-required">{{ t('oidc.behaviour.required') }}</Label>
              <p class="text-muted-foreground text-xs">{{ t('oidc.behaviour.requiredHint') }}</p>
            </div>
            <Switch
              id="oidc-required"
              :model-value="settings.onlyStartIfAvailable"
              :disabled="!canWrite"
              @update:model-value="
                (value: boolean) =>
                  save({ only_start_if_available: value }, t('oidc.toast.saved'))
              "
            />
          </div>
        </CardContent>
      </Card>
    </template>
  </div>
</template>
