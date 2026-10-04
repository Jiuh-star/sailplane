<script setup lang="ts">
import { ref } from 'vue'
import { Eye, EyeOff } from '@lucide/vue'

import { Badge } from '@/components/ui/badge'
import { useI18n } from 'vue-i18n'
import { useLiveStatus } from '@/composables/useLive'
import { useSession } from '@/composables/useSession'

const { t } = useI18n()
const session = useSession()
const { connected } = useLiveStatus()
const revealed = ref(false)

const masked = (value: string) => '•'.repeat(Math.min(value.length, 32))
</script>

<template>
  <footer
    class="text-muted-foreground border-t px-4 py-3 text-xs sm:px-6"
    :aria-label="t('nav.footer.information')"
  >
    <div class="mx-auto flex max-w-7xl flex-wrap items-center justify-between gap-3">
      <div class="flex flex-wrap items-center gap-3">
        <span>sailplane {{ session.config.value.version }}</span>

        <Badge v-if="session.config.value.debug" variant="warning">Debug</Badge>

        <span class="inline-flex items-center gap-1.5">
          <span
            class="size-1.5 rounded-full"
            :class="
              connected === true
                ? 'bg-success'
                : connected === false
                  ? 'bg-warning'
                  : 'bg-muted-foreground/40'
            "
          />
          {{ t(`common.connection.${connected === true ? 'live' : connected === false ? 'reconnecting' : 'connecting'}`) }}
        </span>
      </div>

      <div class="flex items-center gap-2">
        <span class="font-mono">
          {{ revealed ? session.config.value.baseUrl : masked(session.config.value.baseUrl) }}
        </span>
        <button
          class="hover:text-foreground transition-colors"
          :aria-label="revealed ? t('nav.footer.hideAddress') : t('nav.footer.showAddress')"
          @click="revealed = !revealed"
        >
          <EyeOff v-if="revealed" class="size-3.5" />
          <Eye v-else class="size-3.5" />
        </button>
      </div>
    </div>
  </footer>
</template>
