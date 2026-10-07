<script setup lang="ts">
/**
 * Simulates a policy decision and shows the rule that decided it. Headscale
 * only enforces the policy, so the server evaluates the same semantics the data
 * plane applies.
 */
import { computed, ref, watch } from 'vue'
import { Check, LoaderCircle, Search, TriangleAlert, X } from '@lucide/vue'
import { useI18n } from 'vue-i18n'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { errorMessage } from '@/composables/useToast'
import { api, type AccessReport, type AclSelectors } from '@/lib/api'
import { cn } from '@/lib/utils'

const props = defineProps<{
  selectors: AclSelectors | null
  /** The editor's working copy, when it has unsaved changes. */
  draft: string | null
}>()

const { t } = useI18n()

const src = ref('')
/** One destination per line, or several per line separated by commas. */
const dst = ref('')
const port = ref(22)
const protocol = ref('tcp')
const useDraft = ref(true)

const reports = ref<AccessReport[]>([])
/** The batch row whose detail panels appear below. */
const selected = ref(0)
const evaluated = ref<'saved' | 'draft' | null>(null)
const failure = ref<string | null>(null)
const running = ref(false)

/** Anything the source field could name, including dynamic autogroups. */
const sourceSuggestions = computed(() => {
  const s = props.selectors
  if (!s) return []
  return [
    ...s.users,
    ...s.groups,
    ...s.tags,
    ...s.hosts,
    ...s.machines,
    ...(s.autogroups?.source ?? []),
  ]
})

/** The destination field names hosts, tags and machines, not users. */
const destinationSuggestions = computed(() => {
  const s = props.selectors
  if (!s) return []
  return [...s.tags, ...s.hosts, ...s.machines, ...(s.autogroups?.destination ?? [])]
})

const destinations = computed(() =>
  dst.value
    .split(/[\n,]/)
    .map((entry) => entry.trim())
    .filter(Boolean),
)

const canRun = computed(() => src.value.trim() !== '' && destinations.value.length > 0)
const testingDraft = computed(() => useDraft.value && props.draft !== null)

/** The one row whose detail is on screen. */
const report = computed<AccessReport | null>(() => reports.value[selected.value] ?? null)
const allowedCount = computed(() => reports.value.filter((entry) => entry.allowed).length)

// A stale verdict is worse than none: the inputs changed, so the answer did.
watch([src, dst, port, protocol, useDraft], () => {
  reports.value = []
  selected.value = 0
  failure.value = null
})

async function run() {
  if (!canRun.value || running.value) return
  running.value = true
  failure.value = null
  try {
    const response = await api.acl.simulate({
      src: src.value.trim(),
      dsts: destinations.value,
      port: port.value,
      protocol: protocol.value,
      policy: testingDraft.value ? (props.draft ?? undefined) : undefined,
    })
    reports.value = response.reports
    selected.value = 0
    evaluated.value = response.policy
  } catch (err) {
    failure.value = errorMessage(err)
    reports.value = []
  } finally {
    running.value = false
  }
}

function machineLabel(machine: { name: string; addresses: string[]; user: string | null }) {
  const address = machine.addresses[0] ?? ''
  return address ? `${machine.name} (${address})` : machine.name
}
</script>

<template>
  <div class="space-y-4">
    <Card>
      <CardHeader>
        <CardTitle>{{ t('acls.check.title') }}</CardTitle>
        <p class="text-muted-foreground text-sm">{{ t('acls.check.description') }}</p>
      </CardHeader>
      <CardContent class="space-y-4">
        <div class="grid gap-4 sm:grid-cols-2">
          <div class="space-y-2">
            <Label for="check-src">{{ t('acls.check.source') }}</Label>
            <Input
              id="check-src"
              v-model="src"
              list="acl-source-selectors"
              autocomplete="off"
              spellcheck="false"
              :placeholder="t('acls.check.sourcePlaceholder')"
              @keyup.enter="run"
            />
          </div>
          <div class="space-y-2">
            <Label for="check-dst">{{ t('acls.check.destination') }}</Label>
            <Input
              id="check-dst"
              v-model="dst"
              list="acl-dest-selectors"
              autocomplete="off"
              spellcheck="false"
              :placeholder="t('acls.check.destinationPlaceholder')"
              @keyup.enter="run"
            />
            <p v-if="destinations.length > 1" class="text-muted-foreground text-xs">
              {{ t('acls.check.destinationCount', { count: destinations.length }) }}
            </p>
          </div>
        </div>

        <datalist id="acl-source-selectors">
          <option v-for="option in sourceSuggestions" :key="option" :value="option" />
        </datalist>
        <datalist id="acl-dest-selectors">
          <option v-for="option in destinationSuggestions" :key="option" :value="option" />
        </datalist>

        <div class="flex flex-wrap items-end gap-4">
          <div class="space-y-2">
            <Label for="check-port">{{ t('acls.check.port') }}</Label>
            <Input
              id="check-port"
              v-model.number="port"
              type="number"
              min="1"
              max="65535"
              class="w-28"
              @keyup.enter="run"
            />
          </div>

          <div class="space-y-2">
            <Label for="check-proto">{{ t('acls.check.protocol') }}</Label>
            <select
              id="check-proto"
              v-model="protocol"
              class="border-input bg-background h-9 rounded-md border px-3 text-sm"
            >
              <option value="tcp">TCP</option>
              <option value="udp">UDP</option>
            </select>
          </div>

          <Button :disabled="!canRun || running" @click="run">
            <LoaderCircle v-if="running" class="animate-spin" />
            <Search v-else />
            {{ t('acls.check.run') }}
          </Button>

          <label
            v-if="draft !== null"
            class="text-muted-foreground flex items-center gap-2 pb-2 text-sm"
          >
            <input v-model="useDraft" type="checkbox" class="accent-primary size-4" />
            {{ t('acls.check.useDraft') }}
          </label>
        </div>
      </CardContent>
    </Card>

    <Alert v-if="failure" variant="destructive">
      <TriangleAlert />
      <AlertTitle>{{ t('acls.check.failedTitle') }}</AlertTitle>
      <AlertDescription>{{ failure }}</AlertDescription>
    </Alert>

    <Card v-if="reports.length > 1">
      <CardHeader>
        <CardTitle class="text-sm">
          {{ t('acls.check.batchTitle', { allowed: allowedCount, total: reports.length }) }}
        </CardTitle>
        <p class="text-muted-foreground text-xs">{{ t('acls.check.batchHint') }}</p>
      </CardHeader>
      <CardContent>
        <div
          v-for="(entry, index) in reports"
          :key="entry.dst"
          class="flex cursor-pointer items-center gap-3 rounded-md px-2 py-1.5 text-sm"
          :class="index === selected ? 'bg-accent' : 'hover:bg-accent/50'"
          @click="selected = index"
        >
          <span
            :class="
              cn(
                'size-2 shrink-0 rounded-full',
                entry.allowed ? 'bg-success' : 'bg-destructive',
              )
            "
          />
          <code class="truncate text-xs">{{ entry.dst }}</code>
          <span class="text-muted-foreground ml-auto shrink-0 text-xs">
            {{ entry.allowed ? t('acls.check.allowed') : t('acls.check.denied') }}
          </span>
        </div>
      </CardContent>
    </Card>

    <template v-if="report">
      <Card
        :class="
          cn(
            'border-2',
            report.allowed
              ? 'border-[color-mix(in_oklab,var(--success)_55%,transparent)]'
              : 'border-[color-mix(in_oklab,var(--destructive)_55%,transparent)]',
          )
        "
      >
        <CardContent class="flex items-start gap-3">
          <span
            :class="
              cn(
                'grid size-9 shrink-0 place-items-center rounded-full',
                report.allowed
                  ? 'bg-[color-mix(in_oklab,var(--success)_18%,transparent)] text-success'
                  : 'bg-[color-mix(in_oklab,var(--destructive)_18%,transparent)] text-destructive',
              )
            "
          >
            <Check v-if="report.allowed" class="size-5" />
            <X v-else class="size-5" />
          </span>

          <div class="space-y-1">
            <p class="font-semibold">
              {{ report.allowed ? t('acls.check.allowed') : t('acls.check.denied') }}
              <span v-if="reports.length > 1" class="text-muted-foreground font-mono text-sm">
                {{ report.dst }}
              </span>
            </p>
            <p class="text-muted-foreground text-sm">
              <template v-if="report.decision">
                {{
                  t('acls.check.decidedBy', {
                    index: report.decision.index + 1,
                    src: report.decision.matched_src,
                    dst: report.decision.matched_dst,
                  })
                }}
              </template>
              <template v-else>{{ t('acls.check.defaultDeny') }}</template>
            </p>
            <Badge v-if="evaluated === 'draft'" variant="warning">
              {{ t('acls.check.testedDraft') }}
            </Badge>
          </div>
        </CardContent>
      </Card>

      <div v-if="report.notes.length" class="space-y-1">
        <p
          v-for="note in report.notes"
          :key="note"
          class="text-muted-foreground flex items-start gap-2 text-sm"
        >
          <TriangleAlert class="mt-0.5 size-3.5 shrink-0" />
          {{ note }}
        </p>
      </div>

      <div class="grid gap-4 lg:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle class="text-sm">
              {{ t('acls.check.sourceMachines', { count: report.source_machines.length }) }}
            </CardTitle>
          </CardHeader>
          <CardContent class="space-y-1 text-sm">
            <p v-if="!report.source_machines.length" class="text-muted-foreground">
              {{ t('acls.check.nothingMatched') }}
            </p>
            <p v-for="machine in report.source_machines" :key="machine.id">
              {{ machineLabel(machine) }}
            </p>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle class="text-sm">
              {{
                t('acls.check.destinationMachines', {
                  count: report.destination_machines.length,
                })
              }}
            </CardTitle>
          </CardHeader>
          <CardContent class="space-y-1 text-sm">
            <p v-if="!report.destination_machines.length" class="text-muted-foreground">
              {{ t('acls.check.nothingMatched') }}
            </p>
            <p v-for="machine in report.destination_machines" :key="machine.id">
              {{ machineLabel(machine) }}
            </p>
          </CardContent>
        </Card>
      </div>

      <Card class="hp-cards">
        <CardHeader>
          <CardTitle class="text-sm">{{ t('acls.check.ruleBreakdown') }}</CardTitle>
        </CardHeader>
        <CardContent>
          <table class="w-full text-sm">
            <thead class="text-muted-foreground text-left">
              <tr>
                <th class="w-10 pb-2 font-medium">#</th>
                <th class="pb-2 font-medium">{{ t('acls.check.ruleAction') }}</th>
                <th class="pb-2 font-medium">src</th>
                <th class="pb-2 font-medium">dst</th>
                <th class="pb-2 text-right font-medium">{{ t('acls.check.ruleResult') }}</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="rule in report.rules" :key="`${rule.kind}-${rule.index}`" class="border-t">
                <td class="py-2 font-mono" :data-label="t('acls.check.ruleNumber')">{{ rule.index + 1 }}</td>
                <td class="py-2" :data-label="t('acls.check.ruleAction')">
                  <!-- Index alone is not unique across sections. The kind disambiguates. -->
                  <Badge variant="outline" class="mr-1.5">{{ rule.kind.toUpperCase() }}</Badge>
                  {{ rule.action }}
                </td>
                <td class="py-2 font-mono text-xs" :data-label="t('acls.check.ruleSrc')">{{ rule.src.join(', ') }}</td>
                <td class="py-2 font-mono text-xs" :data-label="t('acls.check.ruleDst')">{{ rule.dst.join(', ') }}</td>
                <td class="py-2 text-right" :data-label="t('acls.check.ruleResult')">
                  <Badge v-if="rule.matched" variant="success">match</Badge>
                  <span v-else class="text-muted-foreground">—</span>
                </td>
              </tr>
              <tr v-if="!report.rules.length">
                <td colspan="5" class="text-muted-foreground py-2">
                  {{ t('acls.check.noRules') }}
                </td>
              </tr>
            </tbody>
          </table>
        </CardContent>
      </Card>

      <Card v-if="report.ssh_rules.length" class="hp-cards">
        <CardHeader>
          <CardTitle class="text-sm">{{ t('acls.check.sshRules') }}</CardTitle>
          <p class="text-muted-foreground text-xs">{{ t('acls.check.sshHint') }}</p>
        </CardHeader>
        <CardContent>
          <table class="w-full text-sm">
            <tbody>
              <tr v-for="rule in report.ssh_rules" :key="`${rule.kind}-${rule.index}`" class="border-t">
                <td class="w-10 py-2 font-mono" :data-label="t('acls.check.ruleNumber')">{{ rule.index + 1 }}</td>
                <td class="py-2" :data-label="t('acls.check.ruleAction')">{{ rule.action }}</td>
                <td class="py-2 font-mono text-xs" :data-label="t('acls.check.ruleSrc')">{{ rule.src.join(', ') }}</td>
                <td class="py-2 font-mono text-xs" :data-label="t('acls.check.ruleDst')">{{ rule.dst.join(', ') }}</td>
                <td class="py-2 text-right" :data-label="t('acls.check.ruleResult')">
                  <Badge v-if="rule.matched" variant="success">match</Badge>
                  <span v-else class="text-muted-foreground">—</span>
                </td>
              </tr>
            </tbody>
          </table>
        </CardContent>
      </Card>
    </template>
  </div>
</template>
