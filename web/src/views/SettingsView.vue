<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink } from 'vue-router'
import { useI18n } from 'vue-i18n'
import {
  ArrowRight,
  Bot,
  FileKey,
  History,
  KeyRound,
  KeySquare,
  ScrollText,
  Settings,
  ShieldCheck,
  Waypoints,
} from '@lucide/vue'

import PageHeader from '@/components/shared/PageHeader.vue'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { useSession } from '@/composables/useSession'

const { t } = useI18n()
const session = useSession()

const config = computed(() => session.config.value)
const access = computed(() => session.access.value)

const sections = computed(() =>
  [
    {
      show: access.value.auth_keys || access.value.auth_keys_own,
      to: { name: 'auth-keys' },
      icon: KeyRound,
      title: t('settings.sections.authKeys.title'),
      description: t('settings.sections.authKeys.description'),
    },
    {
      show: access.value.feature,
      to: { name: 'agent' },
      icon: Bot,
      title: t('settings.sections.agent.title'),
      description: t('settings.sections.agent.description'),
      badge: config.value.agentEnabled ? t('common.enabled') : t('common.disabled'),
    },
    {
      show: access.value.network && config.value.configAvailable,
      to: { name: 'oidc' },
      icon: KeySquare,
      title: t('settings.sections.oidc.title'),
      description: t('settings.sections.oidc.description'),
    },
    {
      show: access.value.iam && config.value.configWritable && config.value.oidcEnabled,
      to: { name: 'restrictions' },
      icon: ShieldCheck,
      title: t('settings.sections.restrictions.title'),
      description: t('settings.sections.restrictions.description'),
    },
    {
      show: access.value.network && config.value.configAvailable,
      to: { name: 'derp' },
      icon: Waypoints,
      title: t('settings.sections.derp.title'),
      description: t('settings.sections.derp.description'),
      badge: config.value.configWritable ? undefined : t('settings.configState.readOnly'),
    },
    {
      show: access.value.feature,
      to: { name: 'logs' },
      icon: ScrollText,
      title: t('settings.sections.logs.title'),
      description: t('settings.sections.logs.description'),
    },
    {
      show: access.value.iam,
      to: { name: 'audit' },
      icon: History,
      title: t('settings.sections.audit.title'),
      description: t('settings.sections.audit.description'),
    },
    {
      // Root-equivalent credentials: the owner role, and nobody else.
      show: access.value.owner,
      to: { name: 'api-keys' },
      icon: FileKey,
      title: t('settings.sections.apiKeys.title'),
      description: t('settings.sections.apiKeys.description'),
    },
    {
      // Sailplane's own configuration, which only the owner can edit.
      show: access.value.owner,
      to: { name: 'deployment' },
      icon: Settings,
      title: t('settings.sections.deployment.title'),
      description: t('settings.sections.deployment.description'),
    },
  ].filter((section) => section.show),
)
</script>

<template>
  <div class="space-y-6">
    <PageHeader :title="t('nav.settings')" :description="t('settings.description')" />

    <div class="grid gap-4 sm:grid-cols-2">
      <Card v-for="section in sections" :key="section.title">
        <CardHeader>
          <CardTitle class="flex items-center gap-2 text-base">
            <component :is="section.icon" class="size-4" />
            {{ section.title }}
            <Badge v-if="section.badge" variant="secondary">{{ section.badge }}</Badge>
          </CardTitle>
          <CardDescription>{{ section.description }}</CardDescription>
        </CardHeader>
        <CardContent>
          <Button variant="outline" size="sm" as-child>
            <RouterLink :to="section.to">
              {{ t('common.open') }}
              <ArrowRight />
            </RouterLink>
          </Button>
        </CardContent>
      </Card>

      <Card v-if="!sections.length">
        <CardHeader>
          <CardTitle class="text-base">{{ t('settings.nothingToConfigure.title') }}</CardTitle>
          <CardDescription>
            {{ t('settings.nothingToConfigure.description') }}
          </CardDescription>
        </CardHeader>
      </Card>
    </div>

    <Card>
      <CardHeader>
        <CardTitle class="text-base">{{ t('settings.deployment.overviewTitle') }}</CardTitle>
        <CardDescription>{{ t('settings.deployment.description') }}</CardDescription>
      </CardHeader>
      <CardContent>
        <dl class="grid gap-3 text-sm sm:grid-cols-2">
          <div class="flex justify-between gap-4">
            <dt class="text-muted-foreground">{{ t('settings.deployment.sailplaneVersion') }}</dt>
            <dd>{{ config.version }}</dd>
          </div>
          <div class="flex justify-between gap-4">
            <dt class="text-muted-foreground">{{ t('settings.deployment.headscaleVersion') }}</dt>
            <dd>{{ session.headscale.value.version?.canonical ?? t('common.unknown') }}</dd>
          </div>
          <div class="flex justify-between gap-4">
            <dt class="text-muted-foreground">{{ t('settings.deployment.reloadIntegration') }}</dt>
            <dd>{{ config.integration }}</dd>
          </div>
          <div class="flex justify-between gap-4">
            <dt class="text-muted-foreground">{{ t('settings.deployment.headscaleConfig') }}</dt>
            <dd>
              {{
                config.configWritable
                  ? t('settings.configState.readWrite')
                  : config.configAvailable
                    ? t('settings.configState.readOnly')
                    : t('settings.configState.unavailable')
              }}
            </dd>
          </div>
          <div class="flex justify-between gap-4">
            <dt class="text-muted-foreground">{{ t('settings.deployment.singleSignOn') }}</dt>
            <dd>{{ config.oidcEnabled ? t('common.enabled') : t('common.disabled') }}</dd>
          </div>
          <div class="flex justify-between gap-4">
            <dt class="text-muted-foreground">{{ t('settings.deployment.agentBackend') }}</dt>
            <dd>{{ config.agentEnabled ? config.agentBackend : t('common.disabled') }}</dd>
          </div>
        </dl>
      </CardContent>
    </Card>
  </div>
</template>
