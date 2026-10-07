<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { Laptop, Link2, LoaderCircle, ShieldAlert, Smartphone, Terminal } from '@lucide/vue'
import { useI18n } from 'vue-i18n'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useSession } from '@/composables/useSession'
import { errorMessage, useToast } from '@/composables/useToast'
import { api, type HeadscaleUser } from '@/lib/api'

const { t } = useI18n()
const session = useSession()
const router = useRouter()
const toast = useToast()

const accountId = ref<string | null>(null)
const unlinked = ref<HeadscaleUser[]>([])
const selected = ref('')
const loading = ref(true)
const linking = ref(false)

const downloads = [
  { label: 'Windows 10+', href: 'https://pkgs.tailscale.com/stable/tailscale-setup-latest.exe', icon: Laptop },
  { label: 'macOS', href: 'https://pkgs.tailscale.com/stable/Tailscale-latest-macos.pkg', icon: Laptop },
  { label: 'iOS', href: 'https://apps.apple.com/app/tailscale/id1470499037', icon: Smartphone },
  { label: 'Android', href: 'https://play.google.com/store/apps/details?id=com.tailscale.ipn', icon: Smartphone },
]

const linked = computed(() => Boolean(session.user.value?.headscale_user_id))
const needsLink = computed(() => !linked.value && unlinked.value.length > 0)

onMounted(async () => {
  // Members with UI access never see this page. Send them to the dashboard.
  if (session.access.value.ui) {
    await router.replace({ name: 'machines' })
    return
  }

  try {
    const response = await api.users.list()
    accountId.value = response.currentAccountId
    unlinked.value = response.unlinkedUsers
    if (unlinked.value.length === 1) {
      selected.value = unlinked.value[0]!.id
    }
  } catch {
    // A member can lack `read_users`. The page still explains how to connect.
    unlinked.value = []
  } finally {
    loading.value = false
  }
})

async function link() {
  if (!accountId.value || !selected.value) return
  linking.value = true
  try {
    await api.accounts.link(accountId.value, selected.value)
    toast.success(t('home.link.success'))
    await session.refresh()
    if (session.access.value.ui) {
      await router.replace({ name: 'machines' })
    }
  } catch (err) {
    toast.error(t('home.link.errorTitle'), errorMessage(err))
  } finally {
    linking.value = false
  }
}
</script>

<template>
  <div class="space-y-6">
    <div>
      <h1 class="text-2xl font-semibold">{{ t('home.title') }}</h1>
      <p class="text-muted-foreground text-sm">{{ t('home.subtitle') }}</p>
    </div>

    <Card v-if="loading">
      <CardContent class="text-muted-foreground flex items-center gap-2 py-6 text-sm">
        <LoaderCircle class="size-4 animate-spin" />
        {{ t('home.checkingAccount') }}
      </CardContent>
    </Card>

    <template v-else>
      <Card v-if="needsLink">
        <CardHeader>
          <CardTitle class="flex items-center gap-2">
            <Link2 class="size-4" />
            {{ t('home.link.title') }}
          </CardTitle>
          <CardDescription>{{ t('home.link.description') }}</CardDescription>
        </CardHeader>
        <CardContent class="space-y-4">
          <div class="space-y-2">
            <Label for="link-user">{{ t('home.link.userLabel') }}</Label>
            <Select v-model="selected">
              <SelectTrigger id="link-user">
                <SelectValue :placeholder="t('home.link.userPlaceholder')" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem v-for="user in unlinked" :key="user.id" :value="user.id">
                  {{ user.displayName ?? user.name }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
          <Button :disabled="!selected || linking" @click="link">
            <LoaderCircle v-if="linking" class="animate-spin" />
            {{ t('home.link.submit') }}
          </Button>
        </CardContent>
      </Card>

      <Alert v-else-if="!linked" variant="warning">
        <ShieldAlert />
        <AlertTitle>{{ t('home.notLinked.title') }}</AlertTitle>
        <AlertDescription>{{ t('home.notLinked.description') }}</AlertDescription>
      </Alert>

      <Card v-else>
        <CardHeader>
          <CardTitle class="flex items-center gap-2">
            <Link2 class="size-4" />
            {{ t('home.linked.title') }}
          </CardTitle>
          <CardDescription>
            <i18n-t keypath="home.linked.description" tag="span">
              <template #name>
                <strong>{{ session.user.value?.name }}</strong>
              </template>
            </i18n-t>
          </CardDescription>
        </CardHeader>
      </Card>
    </template>

    <Card>
      <CardHeader>
        <CardTitle>{{ t('home.access.title') }}</CardTitle>
        <CardDescription>{{ t('home.access.description') }}</CardDescription>
      </CardHeader>
      <CardContent class="space-y-4">
        <div class="bg-muted flex items-center gap-2 rounded-md p-3 font-mono text-sm">
          <Terminal class="size-4 shrink-0" />
          <code class="overflow-x-auto">curl -fsSL https://tailscale.com/install.sh | sh</code>
        </div>

        <div class="grid gap-2 sm:grid-cols-2 lg:grid-cols-4">
          <Button v-for="item in downloads" :key="item.label" variant="outline" as-child>
            <a :href="item.href" target="_blank" rel="noreferrer noopener">
              <component :is="item.icon" />
              {{ item.label }}
            </a>
          </Button>
        </div>
      </CardContent>
    </Card>
  </div>
</template>
