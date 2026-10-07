<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { KeyRound, LoaderCircle, TriangleAlert } from '@lucide/vue'
import { useI18n } from 'vue-i18n'

import BrandMark from '@/components/shared/BrandMark.vue'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useSession } from '@/composables/useSession'
import { errorMessage } from '@/composables/useToast'
import { oidcStartUrl } from '@/lib/api'

const { t } = useI18n()
const session = useSession()
const route = useRoute()
const router = useRouter()

const apiKey = ref('')
const submitting = ref(false)
const failure = ref<string | null>(null)

const statusParam = computed(() => (route.query.s as string | undefined) ?? null)

const oidcMessageKeys: Record<string, string> = {
  error_no_query: 'login.oidc.errorNoQuery',
  error_no_session: 'login.oidc.errorNoSession',
  error_invalid_session: 'login.oidc.errorInvalidSession',
  error_no_sub: 'login.oidc.errorNoSub',
  error_auth_failed: 'login.oidc.errorAuthFailed',
  discovery_failed: 'login.oidc.discoveryFailed',
  missing_endpoints: 'login.oidc.missingEndpoints',
  invalid_api_key: 'login.oidc.invalidApiKey',
  not_configured: 'login.oidc.notConfigured',
}

const notice = computed(() => {
  const status = statusParam.value
  if (!status) return null
  if (status === 'logout') {
    return { kind: 'info' as const, message: t('login.signedOutNotice') }
  }
  const key = oidcMessageKeys[status]
  return key ? { kind: 'warning' as const, message: t(key) } : null
})

// The browser drops a secure cookie over plain HTTP, so sign-in can fail
// silently. The reverse combination only means the cookie is not `Secure`.
const insecureCookies = computed(
  () => session.config.value.cookieSecure && !window.location.protocol.startsWith('https'),
)

onMounted(async () => {
  if (session.session.value === null) {
    await session.refresh()
  }
  if (session.authenticated.value) {
    await router.replace(session.landingRoute.value)
  }
})

async function submit() {
  if (!apiKey.value.trim() || submitting.value) return

  submitting.value = true
  failure.value = null
  try {
    await session.login(apiKey.value.trim())
    const returnTo = route.query.returnTo as string | undefined
    await router.replace(returnTo && returnTo.startsWith('/') ? returnTo : session.landingRoute.value)
  } catch (err) {
    failure.value = errorMessage(err)
  } finally {
    submitting.value = false
  }
}

function startOidc() {
  window.location.assign(oidcStartUrl())
}
</script>

<template>
  <div class="flex min-h-svh items-center justify-center p-4">
    <div class="w-full max-w-md space-y-4">
      <div class="text-center">
        <div class="flex items-center justify-center gap-2 text-lg font-semibold">
          <BrandMark class="size-10" />
          {{ t('nav.brand') }}
          <span class="bg-brand/12 text-brand rounded px-1 text-xs leading-5">rs</span>
        </div>
        <h1 class="mt-3 text-xl font-semibold">{{ t('login.title') }}</h1>
        <p class="text-muted-foreground text-sm">{{ t('login.subtitle') }}</p>
      </div>

      <Alert v-if="insecureCookies" variant="warning">
        <TriangleAlert />
        <AlertTitle>{{ t('login.insecureCookies.title') }}</AlertTitle>
        <AlertDescription>
          <i18n-t keypath="login.insecureCookies.body" tag="span">
            <template #setting>
              <code>server.cookie_secure</code>
            </template>
          </i18n-t>
        </AlertDescription>
      </Alert>

      <Alert v-if="notice" :variant="notice.kind === 'warning' ? 'warning' : 'default'">
        <TriangleAlert v-if="notice.kind === 'warning'" />
        <AlertTitle>
          {{ notice.kind === 'warning' ? t('login.signInProblemTitle') : t('login.signedOutTitle') }}
        </AlertTitle>
        <AlertDescription>{{ notice.message }}</AlertDescription>
      </Alert>

      <Card>
        <CardHeader>
          <CardTitle>{{ t('login.cardTitle') }}</CardTitle>
          <CardDescription>{{ t('login.cardDescription') }}</CardDescription>
        </CardHeader>

        <CardContent class="space-y-4">
          <Alert v-if="failure" variant="destructive">
            <TriangleAlert />
            <AlertTitle>{{ t('login.couldNotSignIn') }}</AlertTitle>
            <AlertDescription>{{ failure }}</AlertDescription>
          </Alert>

          <form
            v-if="!session.config.value.apiKeyLoginDisabled"
            class="space-y-4"
            @submit.prevent="submit"
          >
            <div class="space-y-2">
              <Label for="api-key">{{ t('login.apiKeyLabel') }}</Label>
              <Input
                id="api-key"
                v-model="apiKey"
                type="password"
                autocomplete="off"
                placeholder="hskey-api-…"
                :disabled="submitting"
              />
              <p class="text-muted-foreground text-xs">
                <i18n-t keypath="login.apiKeyHint" tag="span">
                  <template #command>
                    <code class="font-mono">headscale apikeys create</code>
                  </template>
                </i18n-t>
              </p>
            </div>

            <Button type="submit" class="w-full" :disabled="submitting || !apiKey.trim()">
              <LoaderCircle v-if="submitting" class="animate-spin" />
              <KeyRound v-else />
              {{ submitting ? t('login.signingIn') : t('login.signIn') }}
            </Button>
          </form>

          <template v-if="session.config.value.oidcEnabled">
            <div v-if="!session.config.value.apiKeyLoginDisabled" class="flex items-center gap-3">
              <span class="bg-border h-px flex-1" />
              <span class="text-muted-foreground text-xs uppercase">{{ t('login.or') }}</span>
              <span class="bg-border h-px flex-1" />
            </div>

            <Button variant="outline" class="w-full" @click="startOidc">
              {{ t('login.continueWithSso') }}
            </Button>
          </template>
        </CardContent>
      </Card>
    </div>
  </div>
</template>
