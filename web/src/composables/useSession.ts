/**
 * Shared session and configuration state. Loaded once from `GET /api/session`
 * and refreshed after login or logout; consumers read from here rather than
 * refetch.
 */

import { computed, readonly, ref } from 'vue'
import { api, type AccessView, type ConfigView, type SessionView, type UserView } from '@/lib/api'

const session = ref<SessionView | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)

const emptyAccess: AccessView = {
  ui: false,
  machines: false,
  machines_write: false,
  users: false,
  users_write: false,
  policy: false,
  policy_write: false,
  network: false,
  network_write: false,
  feature: false,
  feature_write: false,
  iam: false,
  auth_keys: false,
  auth_keys_own: false,
  owner: false,
}

const fallbackConfig: ConfigView = {
  prefix: '',
  baseUrl: '',
  headscaleUrl: '',
  configAvailable: false,
  configWritable: false,
  oidcEnabled: false,
  apiKeyLoginDisabled: false,
  cookieSecure: true,
  integration: 'none',
  agentEnabled: false,
  agentBackend: 'none',
  debug: false,
  version: '',
}

export function useSession() {
  async function refresh(): Promise<SessionView | null> {
    loading.value = true
    error.value = null
    try {
      session.value = await api.session()
      return session.value
    } catch (err) {
      error.value = err instanceof Error ? err.message : String(err)
      return null
    } finally {
      loading.value = false
    }
  }

  async function login(apiKey: string) {
    await api.login(apiKey)
    await refresh()
  }

  async function logout() {
    const result = await api.logout()
    session.value = null
    if (result.redirect) {
      window.location.assign(result.redirect)
      return
    }
    await refresh()
  }

  const authenticated = computed(() => session.value?.authenticated ?? false)
  const user = computed<UserView | null>(() => session.value?.user ?? null)
  const access = computed<AccessView>(() => session.value?.access ?? emptyAccess)
  const config = computed<ConfigView>(() => session.value?.config ?? fallbackConfig)
  const headscale = computed(
    () => session.value?.headscale ?? { healthy: true, version: null },
  )

  const isApiKeySession = computed(() => session.value?.principal === 'api_key')

  /** Default landing route for the signed-in principal. */
  const landingRoute = computed(() => {
    if (access.value.ui) return { name: 'machines' }
    return { name: 'home' }
  })

  return {
    session: readonly(session),
    loading: readonly(loading),
    error: readonly(error),
    authenticated,
    user,
    access,
    config,
    headscale,
    isApiKeySession,
    landingRoute,
    refresh,
    login,
    logout,
  }
}
