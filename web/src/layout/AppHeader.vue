<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink, useRouter } from 'vue-router'
import {
  CircleHelp,
  CircleUser,
  Download,
  Globe,
  Languages,
  Lock,
  LogOut,
  Monitor,
  Moon,
  Server,
  Settings,
  Sun,
  Users,
  Waypoints,
} from '@lucide/vue'

import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import BrandMark from '@/components/shared/BrandMark.vue'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { SUPPORTED_LOCALES, currentLocale, setLocale, type LocaleTag } from '@/i18n'
import { useSession } from '@/composables/useSession'
import { useI18n } from 'vue-i18n'
import { useTheme, type ColorScheme } from '@/composables/useTheme'
import { errorMessage, useToast } from '@/composables/useToast'
import { cn } from '@/lib/utils'
import { initials } from '@/lib/format'

const { t } = useI18n()
const session = useSession()
const router = useRouter()

const locale = computed(() => currentLocale())
function chooseLocale(tag: LocaleTag) {
  setLocale(tag)
}
const { scheme, setScheme } = useTheme()
const toast = useToast()

interface NavItem {
  name: string
  label: string
  icon: typeof Server
  allowed: boolean
  needsConfig?: boolean
}

const navigation = computed<NavItem[]>(() => {
  const access = session.access.value
  const config = session.config.value

  return [
    { name: 'machines', label: t('nav.machines'), icon: Server, allowed: access.ui && access.machines },
    {
      name: 'topology',
      label: t('nav.topology'),
      icon: Waypoints,
      allowed: access.ui && access.machines,
    },
    { name: 'users', label: t('nav.users'), icon: Users, allowed: access.ui && access.users },
    { name: 'acls', label: t('nav.acls'), icon: Lock, allowed: access.ui && access.policy },
    {
      name: 'dns',
      label: t('nav.dns'),
      icon: Globe,
      allowed: access.ui && access.network && config.configAvailable,
    },
    {
      name: 'settings',
      label: t('nav.settings'),
      icon: Settings,
      allowed: access.ui && access.feature && config.configAvailable,
    },
  ].filter((item) => item.allowed)
})

const user = session.user

const helpLinks = computed(() => [
  { href: 'https://headscale.net', label: t('nav.headscale'), icon: CircleHelp },
  { href: 'https://tailscale.com/download', label: t('nav.downloadTailscale'), icon: Download },
])

const themes = computed<{ value: ColorScheme; label: string; icon: typeof Sun }[]>(() => [
  { value: 'system', label: t('nav.theme.system'), icon: Monitor },
  { value: 'light', label: t('nav.theme.light'), icon: Sun },
  { value: 'dark', label: t('nav.theme.dark'), icon: Moon },
])

async function signOut() {
  try {
    await session.logout()
    await router.push({ name: 'login' })
  } catch (err) {
    toast.error(t('nav.signOutFailed'), errorMessage(err))
  }
}
</script>

<template>
  <header class="bg-background/80 sticky top-0 z-40 border-b backdrop-blur">
    <div class="mx-auto flex h-14 max-w-7xl items-center gap-4 px-4 sm:px-6">
      <RouterLink :to="{ name: 'home' }" class="flex items-center gap-2 font-semibold">
        <BrandMark class="size-7" />
        <span class="hidden sm:inline-flex">{{ t('nav.brand') }}</span>
      </RouterLink>

      <nav class="hidden flex-1 items-center gap-1 md:flex" :aria-label="t('nav.mainNavigation')">
        <RouterLink
          v-for="item in navigation"
          :key="item.name"
          :to="{ name: item.name }"
          :class="
            cn(
              'text-muted-foreground hover:bg-accent hover:text-foreground inline-flex items-center gap-2 rounded-md px-3 py-1.5 text-sm font-medium transition-colors',
              $route.name === item.name && 'bg-accent text-foreground',
              ($route.name === 'machine' && item.name === 'machines') && 'bg-accent text-foreground',
            )
          "
        >
          <component :is="item.icon" class="size-4" />
          {{ item.label }}
        </RouterLink>
      </nav>

      <div class="flex flex-1 items-center justify-end gap-1 md:flex-none">
        <DropdownMenu>
          <DropdownMenuTrigger as-child>
            <Button variant="ghost" size="icon" :aria-label="t('nav.help')">
              <CircleHelp />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem v-for="link in helpLinks" :key="link.href" as-child>
              <a :href="link.href" target="_blank" rel="noreferrer noopener">
                <component :is="link.icon" />
                {{ link.label }}
              </a>
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>

        <DropdownMenu>
          <DropdownMenuTrigger as-child>
            <button
              class="hover:bg-accent flex items-center gap-2 rounded-md px-2 py-1.5 text-sm transition-colors"
              :aria-label="t('nav.accountMenu')"
            >
              <Avatar class="size-7">
                <AvatarImage v-if="user?.picture" :src="user.picture" :alt="user.name" />
                <AvatarFallback>
                  <CircleUser v-if="!user" class="size-4" />
                  <template v-else>{{ initials(user.name) }}</template>
                </AvatarFallback>
              </Avatar>
              <span class="hidden text-left sm:block">
                <span class="block text-xs font-medium">{{ user?.name ?? t('nav.apiKey') }}</span>
                <span v-if="user?.email" class="text-muted-foreground block text-xs">
                  {{ user.email }}
                </span>
              </span>
            </button>
          </DropdownMenuTrigger>

          <DropdownMenuContent align="end" class="w-56">
            <DropdownMenuLabel>
              <span class="flex items-center gap-2">
                {{ user?.name ?? t('nav.apiKeySession') }}
                <Badge v-if="user" variant="secondary">{{ user.role_label }}</Badge>
              </span>
            </DropdownMenuLabel>
            <DropdownMenuSeparator />

            <DropdownMenuItem
              v-for="theme in themes"
              :key="theme.value"
              @select="setScheme(theme.value)"
            >
              <component :is="theme.icon" />
              {{ theme.label }}
              <span v-if="scheme === theme.value" class="ml-auto text-xs">•</span>
            </DropdownMenuItem>

            <DropdownMenuSeparator />
            <DropdownMenuLabel class="flex items-center gap-2 font-normal">
              <Languages class="size-4" />
              {{ t('nav.language') }}
            </DropdownMenuLabel>
            <DropdownMenuItem
              v-for="option in SUPPORTED_LOCALES"
              :key="option.tag"
              @select="chooseLocale(option.tag)"
            >
              {{ option.label }}
              <span v-if="locale === option.tag" class="ml-auto text-xs">•</span>
            </DropdownMenuItem>

            <DropdownMenuSeparator />
            <DropdownMenuItem variant="destructive" @select="signOut">
              <LogOut />
              {{ t('nav.signOut') }}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </div>

    <nav
      v-if="navigation.length"
      class="flex gap-1 overflow-x-auto border-t px-4 py-2 md:hidden"
      :aria-label="t('nav.mainNavigation')"
    >
      <RouterLink
        v-for="item in navigation"
        :key="item.name"
        :to="{ name: item.name }"
        :class="
          cn(
            'text-muted-foreground hover:bg-accent inline-flex shrink-0 items-center gap-2 rounded-md px-3 py-1.5 text-sm font-medium',
            ($route.name === item.name ||
              ($route.name === 'machine' && item.name === 'machines')) &&
              'bg-accent text-foreground',
          )
        "
      >
        <component :is="item.icon" class="size-4" />
        {{ item.label }}
      </RouterLink>
    </nav>
  </header>
</template>
