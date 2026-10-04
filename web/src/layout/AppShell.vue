<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted } from 'vue'
import { RouterView, useRoute } from 'vue-router'
import { CircleAlert } from '@lucide/vue'

import AppHeader from './AppHeader.vue'
import AppFooter from './AppFooter.vue'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { useI18n } from 'vue-i18n'
import { startLiveStream, stopLiveStream } from '@/composables/useLive'
import { useSession } from '@/composables/useSession'

const { t } = useI18n()
const session = useSession()
const route = useRoute()

// One stream per session, owned by the shell.
onMounted(startLiveStream)
onBeforeUnmount(stopLiveStream)

const unhealthy = computed(() => session.headscale.value.healthy === false)
const title = computed(() => (route.meta.title as string | undefined) ?? 'Sailplane')
</script>

<template>
  <div class="flex min-h-svh flex-col">
    <AppHeader />

    <main class="mx-auto w-full max-w-7xl flex-1 px-4 py-6 sm:px-6">
      <Alert v-if="unhealthy" variant="destructive" class="mb-6">
        <CircleAlert />
        <AlertTitle>{{ t('common.shell.unreachableTitle') }}</AlertTitle>
        <AlertDescription>
          {{
            t('common.shell.unreachableBody', {
              url: session.config.value.headscaleUrl,
            })
          }}
        </AlertDescription>
      </Alert>

      <RouterView v-slot="{ Component }">
        <component :is="Component" :key="title" />
      </RouterView>
    </main>

    <AppFooter />
  </div>
</template>
