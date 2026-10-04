/**
 * Client-side routes. The base path comes from `document.baseURI`, which the
 * server sets to the mount point, so one build works at `/` or `/admin/`.
 */

import { createRouter, createWebHistory, type RouteRecordRaw } from 'vue-router'

import { useSession } from '@/composables/useSession'

export const routes: RouteRecordRaw[] = [
  {
    path: '/login',
    name: 'login',
    component: () => import('@/views/LoginView.vue'),
    meta: { public: true, bare: true },
  },
  {
    path: '/',
    component: () => import('@/layout/AppShell.vue'),
    children: [
      {
        path: '',
        name: 'home',
        component: () => import('@/views/HomeView.vue'),
        meta: { title: 'Home' },
      },
      {
        path: 'machines',
        name: 'machines',
        component: () => import('@/views/MachinesView.vue'),
        meta: { title: 'Machines', requires: 'machines' },
      },
      {
        path: 'machines/:id',
        name: 'machine',
        component: () => import('@/views/MachineView.vue'),
        meta: { title: 'Machine', requires: 'machines' },
      },
      {
        path: 'topology',
        name: 'topology',
        component: () => import('@/views/TopologyView.vue'),
        meta: { title: 'Topology', requires: 'machines' },
      },
      {
        path: 'users',
        name: 'users',
        component: () => import('@/views/UsersView.vue'),
        meta: { title: 'Users', requires: 'users' },
      },
      {
        path: 'acls',
        name: 'acls',
        component: () => import('@/views/AclsView.vue'),
        meta: { title: 'Access Control', requires: 'policy' },
      },
      {
        path: 'dns',
        name: 'dns',
        component: () => import('@/views/DnsView.vue'),
        meta: { title: 'DNS', requires: 'network', needsConfig: true },
      },
      {
        path: 'derp',
        name: 'derp',
        component: () => import('@/views/DerpView.vue'),
        meta: { title: 'DERP', requires: 'network', needsConfig: true },
      },
      {
        path: 'settings',
        name: 'settings',
        component: () => import('@/views/SettingsView.vue'),
        meta: { title: 'Settings', requires: 'feature' },
      },
      {
        path: 'settings/auth-keys',
        name: 'auth-keys',
        component: () => import('@/views/AuthKeysView.vue'),
        meta: { title: 'Pre-Auth Keys', requiresAny: ['auth_keys', 'auth_keys_own'] },
      },
      {
        path: 'settings/oidc',
        name: 'oidc',
        component: () => import('@/views/OidcView.vue'),
        meta: { title: 'Single sign-on', requires: 'network', needsConfig: true },
      },
      {
        path: 'settings/restrictions',
        name: 'restrictions',
        component: () => import('@/views/RestrictionsView.vue'),
        meta: { title: 'Authentication Restrictions', requires: 'iam', needsConfig: true },
      },
      {
        path: 'settings/logs',
        name: 'logs',
        component: () => import('@/views/LogsView.vue'),
        meta: { title: 'Logs', requires: 'feature' },
      },
      {
        path: 'settings/audit',
        name: 'audit',
        component: () => import('@/views/AuditView.vue'),
        meta: { title: 'Audit Log', requires: 'iam' },
      },
      {
        path: 'settings/api-keys',
        name: 'api-keys',
        component: () => import('@/views/ApiKeysView.vue'),
        meta: { title: 'API Keys', requires: 'owner' },
      },
      {
        path: 'settings/agent',
        name: 'agent',
        component: () => import('@/views/AgentView.vue'),
        meta: { title: 'Sailplane Agent', requires: 'feature' },
      },
    ],
  },
  {
    path: '/ssh/:id',
    name: 'ssh',
    component: () => import('@/views/SshView.vue'),
    meta: { title: 'SSH' },
  },
  {
    path: '/:pathMatch(.*)*',
    name: 'not-found',
    component: () => import('@/views/NotFoundView.vue'),
    meta: { public: true, bare: true },
  },
]

function basePath(): string {
  try {
    return new URL(document.baseURI).pathname
  } catch {
    return '/'
  }
}

export const router = createRouter({
  history: createWebHistory(basePath()),
  routes,
  scrollBehavior: () => ({ top: 0 }),
})

router.beforeEach(async (to) => {
  const session = useSession()

  if (session.session.value === null) {
    await session.refresh()
  }

  const isPublic = to.meta.public === true

  if (!session.authenticated.value && !isPublic) {
    return { name: 'login', query: { returnTo: to.fullPath } }
  }

  if (session.authenticated.value && to.name === 'login') {
    return session.landingRoute.value
  }

  const access = session.access.value

  const required = to.meta.requires as string | undefined
  if (required && !access[required as keyof typeof access]) {
    return { name: 'home' }
  }

  const anyOf = to.meta.requiresAny as string[] | undefined
  if (anyOf && !anyOf.some((key) => access[key as keyof typeof access])) {
    return { name: 'home' }
  }

  if (to.meta.needsConfig && !session.config.value.configAvailable) {
    return { name: 'home' }
  }

  return true
})
