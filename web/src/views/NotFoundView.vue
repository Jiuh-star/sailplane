<script setup lang="ts">
import { RouterLink } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { Button } from '@/components/ui/button'

const { t } = useI18n()
</script>

<template>
  <div class="flex min-h-svh flex-col items-center justify-center gap-4 p-6 text-center">
    <p class="text-muted-foreground font-mono text-sm">404</p>
    <h1 class="text-2xl font-semibold">{{ t('common.notFound.title') }}</h1>
    <p class="text-muted-foreground max-w-md text-sm">{{ t('common.notFound.description') }}</p>
    <Button as-child>
      <RouterLink to="/">{{ t('common.notFound.backHome') }}</RouterLink>
    </Button>
  </div>
</template>
