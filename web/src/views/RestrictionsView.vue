<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { Plus } from '@lucide/vue'

import PageHeader from '@/components/shared/PageHeader.vue'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Skeleton } from '@/components/ui/skeleton'
import { errorMessage, useToast } from '@/composables/useToast'
import { api, type RestrictionsResponse } from '@/lib/api'

type Kind = 'domains' | 'groups' | 'users'

const { t } = useI18n()
const toast = useToast()

const data = ref<RestrictionsResponse | null>(null)
const loading = ref(true)
const failure = ref<string | null>(null)
const busy = ref(false)

const drafts = ref<Record<Kind, string>>({ domains: '', groups: '', users: '' })

async function load() {
  try {
    data.value = await api.restrictions.get()
    failure.value = null
  } catch (err) {
    failure.value = errorMessage(err)
  } finally {
    loading.value = false
  }
}

load()

const writable = computed(() => (data.value?.access.writable ?? false) && (data.value?.access.write ?? false))

const sections = computed(() => [
  {
    kind: 'domains' as Kind,
    title: t('restrictions.sections.domains.title'),
    empty: t('restrictions.sections.domains.empty'),
    placeholder: t('restrictions.sections.domains.placeholder'),
    values: data.value?.restrictions.allowed_domains ?? [],
  },
  {
    kind: 'groups' as Kind,
    title: t('restrictions.sections.groups.title'),
    empty: t('restrictions.sections.groups.empty'),
    placeholder: t('restrictions.sections.groups.placeholder'),
    values: data.value?.restrictions.allowed_groups ?? [],
  },
  {
    kind: 'users' as Kind,
    title: t('restrictions.sections.users.title'),
    empty: t('restrictions.sections.users.empty'),
    placeholder: t('restrictions.sections.users.placeholder'),
    values: data.value?.restrictions.allowed_users ?? [],
  },
])

async function add(kind: Kind) {
  const value = drafts.value[kind].trim()
  if (!value) return
  busy.value = true
  try {
    const result = (await api.restrictions.update('add', kind, value)) as { warning?: string | null }
    await load()
    drafts.value[kind] = ''
    if (result?.warning) toast.warning(t('restrictions.toast.added'), result.warning)
    else toast.success(t('restrictions.toast.added'))
  } catch (err) {
    toast.error(t('restrictions.toast.addFailed'), errorMessage(err))
  } finally {
    busy.value = false
  }
}

async function remove(kind: Kind, value: string) {
  busy.value = true
  try {
    const result = (await api.restrictions.update('remove', kind, value)) as { warning?: string | null }
    await load()
    if (result?.warning) toast.warning(t('restrictions.toast.removed'), result.warning)
    else toast.success(t('restrictions.toast.removed'))
  } catch (err) {
    toast.error(t('restrictions.toast.removeFailed'), errorMessage(err))
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="space-y-6">
    <PageHeader :title="t('restrictions.title')" :description="t('restrictions.description')" />

    <div v-if="loading" class="space-y-2">
      <Skeleton class="h-32 w-full" />
      <Skeleton class="h-32 w-full" />
    </div>

    <Alert v-else-if="failure" variant="destructive">
      <AlertTitle>{{ t('restrictions.loadFailedTitle') }}</AlertTitle>
      <AlertDescription>{{ failure }}</AlertDescription>
    </Alert>

    <template v-else>
      <Alert v-if="!data?.access.write" variant="warning">
        <AlertTitle>{{ t('restrictions.readOnly.title') }}</AlertTitle>
        <AlertDescription>
          {{ t('restrictions.readOnly.description') }}
        </AlertDescription>
      </Alert>
      <Alert v-else-if="!data?.access.writable" variant="warning">
        <AlertTitle>{{ t('restrictions.notWritable.title') }}</AlertTitle>
        <AlertDescription>
          {{ t('restrictions.notWritable.description') }}
        </AlertDescription>
      </Alert>

      <Card v-for="section in sections" :key="section.kind">
        <CardHeader>
          <CardTitle class="text-base">{{ section.title }}</CardTitle>
          <CardDescription>
            {{ t('restrictions.allowEveryoneHint') }}
          </CardDescription>
        </CardHeader>
        <CardContent class="space-y-3">
          <p v-if="!section.values.length" class="text-muted-foreground text-sm">
            {{ section.empty }}
          </p>

          <div
            v-for="value in section.values"
            :key="value"
            class="flex items-center justify-between rounded-md border p-3"
          >
            <code class="text-sm">
              {{ section.kind === 'domains' ? `*@${value}` : value }}
            </code>
            <Button
              size="sm"
              variant="ghost"
              class="text-destructive"
              :disabled="busy || !writable"
              @click="remove(section.kind, value)"
            >
              {{ t('common.remove') }}
            </Button>
          </div>

          <form class="flex gap-2" @submit.prevent="add(section.kind)">
            <Input
              v-model="drafts[section.kind]"
              :placeholder="section.placeholder"
              :disabled="!writable"
            />
            <Button type="submit" variant="outline" :disabled="busy || !writable || !drafts[section.kind].trim()">
              <Plus />
              {{ t('common.add') }}
            </Button>
          </form>
        </CardContent>
      </Card>
    </template>
  </div>
</template>
