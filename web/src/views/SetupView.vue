<script setup lang="ts">
/**
 * First-run onboarding. Public and bare: the server sends this page before any
 * account exists. Every call carries the one-time setup token when the caller
 * typed one. A loopback caller can leave it empty.
 */
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import {
  ArrowLeft,
  ArrowRight,
  Check,
  CircleCheck,
  LoaderCircle,
  ShieldCheck,
  TriangleAlert,
} from '@lucide/vue'

import BrandMark from '@/components/shared/BrandMark.vue'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { useSession } from '@/composables/useSession'
import { errorMessage } from '@/composables/useToast'
import { api } from '@/lib/api'
import { cn } from '@/lib/utils'

const { t } = useI18n()
const router = useRouter()
const session = useSession()

const step = ref(0)
const checking = ref(true)

const token = ref('')
const url = ref('')
const apiKey = ref('')
const baseUrl = ref('')
// A browser drops a secure cookie over plain HTTP, so guess from the current scheme.
const cookieSecure = ref(window.location.protocol.startsWith('https'))

const testing = ref(false)
const testResult = ref<{ ok: boolean; message: string } | null>(null)
const submitting = ref(false)
const failure = ref<string | null>(null)

const stepKeys = ['welcome', 'headscale', 'sailplane', 'finish'] as const

const steps = computed(() =>
  stepKeys.map((key, index) => ({
    index,
    title: t(`setup.steps.${key}.title`),
    description: t(`setup.steps.${key}.description`),
  })),
)

const current = computed(() => steps.value[step.value])

const canContinue = computed(() => {
  if (step.value === 0) return true
  if (step.value === 1) return url.value.trim() !== '' && apiKey.value.trim() !== ''
  return true
})

// Loopback does not use the token, so the first step stays optional.
const isLastStep = computed(() => step.value === steps.value.length - 1)

onMounted(async () => {
  try {
    const status = await api.setup.status()
    url.value = status.headscaleUrl ?? ''
    if (!status.required) {
      await router.replace({ name: 'login' })
      return
    }
  } catch {
    // When the status probe fails, keep the wizard usable.
  } finally {
    checking.value = false
  }
})

function goTo(index: number) {
  if (index < 0 || index > step.value) return
  step.value = index
  failure.value = null
}

function next() {
  if (!canContinue.value || isLastStep.value) return
  step.value += 1
  failure.value = null
}

function back() {
  goTo(step.value - 1)
}

async function testConnection() {
  if (testing.value) return
  testing.value = true
  testResult.value = null
  failure.value = null
  try {
    const result = await api.setup.testHeadscale(
      url.value.trim(),
      apiKey.value.trim(),
      token.value.trim() || undefined,
    )
    testResult.value = {
      ok: true,
      message: result.version
        ? t('setup.headscale.testOkVersion', { version: result.version })
        : t('setup.headscale.testOk'),
    }
  } catch (err) {
    testResult.value = { ok: false, message: errorMessage(err) }
  } finally {
    testing.value = false
  }
}

async function finish() {
  if (submitting.value || !url.value.trim() || !apiKey.value.trim()) return
  submitting.value = true
  failure.value = null
  try {
    await api.setup.complete(
      {
        url: url.value.trim(),
        api_key: apiKey.value.trim(),
        base_url: baseUrl.value.trim() || undefined,
        cookie_secure: cookieSecure.value,
      },
      token.value.trim() || undefined,
    )
    // Clear the cached `setupRequired` flag before the login page loads.
    await session.refresh()
    await router.replace({ name: 'login' })
  } catch (err) {
    failure.value = errorMessage(err)
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <div class="flex min-h-svh items-center justify-center p-4">
    <div class="w-full max-w-2xl space-y-4">
      <div class="text-center">
        <div class="flex items-center justify-center gap-2 text-lg font-semibold">
          <BrandMark class="size-10" />
          {{ t('nav.brand') }}
          <span class="bg-brand/12 text-brand rounded px-1 text-xs leading-5">rs</span>
        </div>
        <h1 class="mt-3 text-xl font-semibold">{{ t('setup.title') }}</h1>
        <p class="text-muted-foreground text-sm">{{ t('setup.subtitle') }}</p>
      </div>

      <ol v-if="!checking" class="flex items-center justify-center gap-2">
        <li v-for="item of steps" :key="item.index" class="flex items-center gap-2">
          <button
            type="button"
            class="flex items-center gap-2 disabled:cursor-default"
            :disabled="item.index > step"
            @click="goTo(item.index)"
          >
            <span
              :class="
                cn(
                  'flex size-6 items-center justify-center rounded-full border text-xs font-medium',
                  item.index === step && 'border-primary bg-primary text-primary-foreground',
                  item.index < step && 'border-primary text-primary',
                  item.index > step && 'text-muted-foreground',
                )
              "
            >
              <Check v-if="item.index < step" class="size-3.5" />
              <template v-else>{{ item.index + 1 }}</template>
            </span>
            <span
              :class="
                cn(
                  'hidden text-sm sm:block',
                  item.index === step ? 'text-foreground font-medium' : 'text-muted-foreground',
                )
              "
            >
              {{ item.title }}
            </span>
          </button>
          <span
            v-if="item.index < steps.length - 1"
            class="bg-border h-px w-4 sm:w-8"
            aria-hidden="true"
          />
        </li>
      </ol>

      <Card v-if="!checking">
        <CardHeader>
          <CardTitle>{{ current.title }}</CardTitle>
          <CardDescription>{{ current.description }}</CardDescription>
        </CardHeader>

        <CardContent class="space-y-4">
          <Alert v-if="failure" variant="destructive">
            <TriangleAlert />
            <AlertTitle>{{ t('setup.errorTitle') }}</AlertTitle>
            <AlertDescription>{{ failure }}</AlertDescription>
          </Alert>

          <!-- 1. Welcome -->
          <div v-if="step === 0" class="space-y-4">
            <Alert>
              <ShieldCheck />
              <AlertTitle>{{ t('setup.welcome.noteTitle') }}</AlertTitle>
              <AlertDescription>{{ t('setup.welcome.noteBody') }}</AlertDescription>
            </Alert>
            <div class="space-y-2">
              <Label for="setup-token">{{ t('setup.welcome.tokenLabel') }}</Label>
              <Input
                id="setup-token"
                v-model="token"
                type="password"
                autocomplete="off"
                spellcheck="false"
                :placeholder="t('setup.welcome.tokenPlaceholder')"
              />
              <p class="text-muted-foreground text-xs">{{ t('setup.welcome.tokenHint') }}</p>
            </div>
          </div>

          <!-- 2. Headscale -->
          <div v-else-if="step === 1" class="space-y-4">
            <div class="space-y-2">
              <Label for="setup-url">{{ t('setup.headscale.urlLabel') }}</Label>
              <Input
                id="setup-url"
                v-model="url"
                placeholder="http://127.0.0.1:8080"
                autocomplete="off"
                spellcheck="false"
              />
            </div>
            <div class="space-y-2">
              <Label for="setup-api-key">{{ t('setup.headscale.apiKeyLabel') }}</Label>
              <Input
                id="setup-api-key"
                v-model="apiKey"
                type="password"
                autocomplete="new-password"
                spellcheck="false"
                :placeholder="t('setup.headscale.apiKeyPlaceholder')"
              />
              <p class="text-muted-foreground text-xs">{{ t('setup.headscale.apiKeyHint') }}</p>
            </div>

            <div class="flex flex-wrap items-center gap-3">
              <Button
                variant="outline"
                :disabled="testing || !url.trim() || !apiKey.trim()"
                @click="testConnection"
              >
                <LoaderCircle v-if="testing" class="animate-spin" />
                {{ testing ? t('setup.headscale.testing') : t('setup.headscale.test') }}
              </Button>
              <span
                v-if="testResult"
                :class="
                  cn(
                    'inline-flex items-center gap-1 text-sm',
                    testResult.ok ? 'text-success' : 'text-destructive',
                  )
                "
              >
                <CircleCheck v-if="testResult.ok" class="size-4" />
                <TriangleAlert v-else class="size-4" />
                {{ testResult.message }}
              </span>
            </div>
          </div>

          <!-- 3. Sailplane address -->
          <div v-else-if="step === 2" class="space-y-4">
            <div class="space-y-2">
              <Label for="setup-base-url">{{ t('setup.sailplane.baseUrlLabel') }}</Label>
              <Input
                id="setup-base-url"
                v-model="baseUrl"
                placeholder="https://sailplane.example.com"
                autocomplete="off"
                spellcheck="false"
              />
              <p class="text-muted-foreground text-xs">{{ t('setup.sailplane.baseUrlHint') }}</p>
            </div>
            <div class="flex items-center justify-between gap-4">
              <div>
                <Label for="setup-cookie-secure">{{ t('setup.sailplane.cookieSecureLabel') }}</Label>
                <p class="text-muted-foreground text-xs">
                  {{ t('setup.sailplane.cookieSecureHint') }}
                </p>
              </div>
              <Switch id="setup-cookie-secure" v-model="cookieSecure" />
            </div>
          </div>

          <!-- 4. Finish -->
          <div v-else class="space-y-4">
            <dl class="grid gap-3 text-sm sm:grid-cols-2">
              <div class="flex justify-between gap-4">
                <dt class="text-muted-foreground">{{ t('setup.finish.summaryUrl') }}</dt>
                <dd class="truncate font-medium">{{ url || t('setup.finish.none') }}</dd>
              </div>
              <div class="flex justify-between gap-4">
                <dt class="text-muted-foreground">{{ t('setup.finish.summaryBaseUrl') }}</dt>
                <dd class="truncate font-medium">{{ baseUrl || t('setup.finish.none') }}</dd>
              </div>
              <div class="flex justify-between gap-4">
                <dt class="text-muted-foreground">{{ t('setup.finish.summaryCookies') }}</dt>
                <dd class="font-medium">
                  {{ cookieSecure ? t('common.enabled') : t('common.disabled') }}
                </dd>
              </div>
            </dl>
            <p class="text-muted-foreground text-xs">{{ t('setup.finish.note') }}</p>
          </div>

          <div class="flex items-center justify-between gap-2 pt-2">
            <Button v-if="step > 0" variant="ghost" :disabled="submitting" @click="back">
              <ArrowLeft />
              {{ t('setup.back') }}
            </Button>
            <span v-else />

            <Button v-if="!isLastStep" :disabled="!canContinue" @click="next">
              {{ t('setup.next') }}
              <ArrowRight />
            </Button>
            <Button
              v-else
              :disabled="submitting || !url.trim() || !apiKey.trim()"
              @click="finish"
            >
              <LoaderCircle v-if="submitting" class="animate-spin" />
              {{ submitting ? t('setup.finish.submitting') : t('setup.finish.submit') }}
            </Button>
          </div>
        </CardContent>
      </Card>

      <p v-else class="text-muted-foreground text-center text-sm">{{ t('common.loading') }}</p>
    </div>
  </div>
</template>
