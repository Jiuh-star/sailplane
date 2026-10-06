/**
 * Typed client for the Sailplane JSON API. URLs resolve against
 * `document.baseURI`, which the server sets to the base path (`/admin/` by
 * default), so one build works at any mount point.
 */

export class ApiError extends Error {
  readonly status: number
  readonly code: string

  constructor(message: string, status: number, code: string) {
    super(message)
    this.name = 'ApiError'
    this.status = status
    this.code = code
  }

  get isUnauthorized(): boolean {
    return this.status === 401
  }

  get isForbidden(): boolean {
    return this.status === 403
  }
}

function resolve(path: string): string {
  const base = document.baseURI ?? `${window.location.origin}/`
  return new URL(path.replace(/^\//, ''), base).toString()
}

async function request<T>(
  method: string,
  path: string,
  body?: unknown,
  headers?: Record<string, string>,
): Promise<T> {
  const response = await fetch(resolve(path), {
    method,
    credentials: 'same-origin',
    headers: {
      ...(body === undefined ? {} : { 'Content-Type': 'application/json' }),
      ...headers,
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  })

  if (response.status === 204) {
    return undefined as T
  }

  const text = await response.text()
  let payload: unknown = null
  if (text) {
    try {
      payload = JSON.parse(text)
    } catch {
      payload = null
    }
  }

  if (!response.ok) {
    const detail = (payload as { error?: { message?: string; code?: string } } | null)?.error
    throw new ApiError(
      detail?.message ?? (text.slice(0, 300) || response.statusText),
      response.status,
      detail?.code ?? 'http_error',
    )
  }

  return payload as T
}

const get = <T>(path: string) => request<T>('GET', path)
const post = <T>(path: string, body?: unknown, headers?: Record<string, string>) =>
  request<T>('POST', path, body ?? {}, headers)
const put = <T>(path: string, body?: unknown) => request<T>('PUT', path, body ?? {})
const del = <T>(path: string) => request<T>('DELETE', path)

// --- Types ---

export interface AccessView {
  ui: boolean
  machines: boolean
  machines_write: boolean
  users: boolean
  users_write: boolean
  policy: boolean
  policy_write: boolean
  network: boolean
  network_write: boolean
  feature: boolean
  feature_write: boolean
  iam: boolean
  auth_keys: boolean
  auth_keys_own: boolean
  owner: boolean
}

export interface UserView {
  name: string
  email: string | null
  username: string | null
  picture: string | null
  role: string
  role_label: string
  is_owner: boolean
  headscale_user_id: string | null
}

export interface ConfigView {
  prefix: string
  baseUrl: string
  headscaleUrl: string
  configAvailable: boolean
  configWritable: boolean
  oidcEnabled: boolean
  apiKeyLoginDisabled: boolean
  cookieSecure: boolean
  integration: string
  agentEnabled: boolean
  agentBackend: string
  debug: boolean
  version: string
  /** Absent on older servers; treat as supported when missing. */
  grantsSupported?: boolean
  /** True until first-run onboarding completes. */
  setupRequired: boolean
}

export interface HeadscaleVersionView {
  raw: string
  canonical: string
  unknown: boolean
  below_minimum: boolean
  browser_ssh_supported: boolean
  capabilities: {
    pre_auth_keys_have_stable_ids: boolean
    node_tags_are_flat: boolean
    node_owner_is_immutable: boolean
    register_key_includes_auth_req_prefix: boolean
    key_expiry_can_be_disabled: boolean
  }
}

export interface SessionView {
  authenticated: boolean
  user: UserView | null
  principal: 'user' | 'api_key' | 'proxy' | null
  access: AccessView | null
  config: ConfigView
  headscale: { healthy: boolean; version: HeadscaleVersionView }
}

export interface StatusTag {
  /** Stable identifier for translation; absent on older servers. */
  key?: string
  /** English fallback computed by the server. */
  label: string
  kind: 'success' | 'warning' | 'danger' | 'info' | 'muted'
  /** Route the badge refers to, interpolated into the translation. */
  subject?: string
}

export interface Machine {
  id: string
  machineKey: string
  nodeKey: string
  discoKey: string
  ipAddresses: string[]
  name: string
  givenName: string
  user: HeadscaleUser | null
  lastSeen: string
  expiry: string | null
  createdAt: string
  registerMethod: string
  online: boolean
  approvedRoutes: string[]
  availableRoutes: string[]
  subnetRoutes: string[]
  tags: string[]
  ipv4: string | null
  ipv6: string | null
  expired: boolean
  expiry_disabled: boolean
  exit_node: boolean
  exit_approved: boolean
  status_tags: StatusTag[]
  host_info: Record<string, unknown> | null
  version: string | null
  os: string | null
  endpoints: string[]
  ssh_host_keys: string[]
}

export interface HeadscaleUser {
  id: string
  name: string
  createdAt: string
  displayName: string | null
  email: string | null
  providerId: string | null
  provider: string | null
  profilePicUrl: string | null
}

export interface PreAuthKey {
  id: string
  key: string
  user: HeadscaleUser | null
  reusable: boolean
  ephemeral: boolean
  used: boolean
  expiration: string
  createdAt: string
  aclTags: string[]
}

/** One state-changing request, as Sailplane recorded it. */
export interface AuditEntry {
  id: number
  at: string
  actor: string
  role: string
  method: string
  path: string
  status: number
  detail: string | null
}

/** A Headscale API key. Only the masked prefix is retrievable. */
export interface HeadscaleApiKey {
  id: string
  prefix: string
  expiration: string | null
  createdAt: string | null
  lastSeen: string | null
}

export interface MachineListResponse {
  machines: Machine[]
  users: HeadscaleUser[]
  nodesVersion: number
  usersVersion: number
  policyTags: string[]
  magic: string | null
  access: { read: boolean; write: boolean }
  supports: { nodeOwnerChange: boolean; disablingKeyExpiry: boolean }
  agent: AgentStatus
  server: string
}

export interface AgentStatus {
  enabled: boolean
  reason?: string
  backend: string
  synced_at: string | null
  node_count: number
  error: string | null
  auth_url: string | null
}

export interface AccountView {
  id: string
  name: string
  email: string | null
  picture: string | null
  role: string
  role_label: string
  is_owner: boolean
  headscale_user_id: string | null
  headscale_user_name: string | null
  created_at: string
  last_login_at: string | null
  online: boolean
  last_seen: string | null
  machine_count: number
  groups: string[]
}

export interface UserListResponse {
  accounts: AccountView[]
  headscaleUsers: HeadscaleUser[]
  unlinkedUsers: HeadscaleUser[]
  groups: Record<string, string[]>
  policy: { available: boolean; groups: string[] }
  access: {
    read: boolean
    write: boolean
    policy_write: boolean
    owner: boolean
    editable_groups: boolean
  }
  currentAccountId: string | null
  roles: { value: string; label: string; description: string }[]
}

export interface AclRule {
  action: string
  src: string[]
  dst: string[]
  proto?: string
  srcPosture?: string[]
  /** Unknown per-rule keys (for example `acceptEnv`) round-trip here. */
  [key: string]: unknown
}

export interface SshRule {
  action: string
  src: string[]
  dst: string[]
  users: string[]
  checkPeriod?: string
  srcPosture?: string[]
  /** Unknown per-rule keys round-trip here. */
  [key: string]: unknown
}

/** A `grants` rule, the newer access syntax. */
export interface GrantRule {
  src: string[]
  dst: string[]
  ip?: string[]
  via?: string[]
  app?: unknown[]
  srcPosture?: string[]
  /** Unknown per-rule keys round-trip here. */
  [key: string]: unknown
}

/** Automatic route and exit-node approval by selector. */
export interface AutoApprovers {
  routes?: Record<string, string[]>
  exitNode?: string[]
  [key: string]: unknown
}

/** A `nodeAttrs` entry: attributes applied to the nodes a selector names. */
export interface NodeAttr {
  target: string[]
  attr: string[]
  [key: string]: unknown
}

export interface ParsedPolicy {
  acls: AclRule[]
  ssh: SshRule[]
  hosts: Record<string, string>
  groups: Record<string, string[]>
  tagOwners: Record<string, string[]>
  /** May be a raw non-array value in a malformed policy; guard before use. */
  grants?: GrantRule[]
  autoApprovers?: AutoApprovers
  nodeAttrs?: NodeAttr[]
  postures?: Record<string, string[]>
  tests?: unknown[]
  sshTests?: unknown[]
  randomizeClientPort?: boolean
  /** Unknown top-level keys are flattened here, so saving never drops them. */
  [key: string]: unknown
}

/** Everything a policy selector may name, for the access check's suggestions. */
export interface AclSelectors {
  users: string[]
  groups: string[]
  hosts: string[]
  tags: string[]
  machines: string[]
  /** Dynamic selectors, offered as text: who they name depends on the caller. */
  autogroups: {
    source: string[]
    destination: string[]
  }
}

export interface AclResponse {
  policy: string
  updatedAt: string | null
  writable: boolean
  hasComments: boolean
  parsed: ParsedPolicy | null
  parseError: string | null
  users: string[]
  tagUsage: Record<string, string[]>
  declaredTags: string[]
  selectors: AclSelectors
  access: { read: boolean; write: boolean }
}

export interface AclMachineRef {
  id: string
  name: string
  addresses: string[]
  user: string | null
  tags: string[]
}

export interface AclRuleOutcome {
  index: number
  kind: 'acl' | 'grant' | 'ssh'
  action: string
  src: string[]
  dst: string[]
  /** Present on grant rules only: the protocols and ports they allow. */
  ip?: string[]
  matched_src: string | null
  matched_dst: string | null
  matched: boolean
}

export interface AccessReport {
  allowed: boolean
  src: string
  dst: string
  port: number
  protocol: string
  source_machines: AclMachineRef[]
  destination_machines: AclMachineRef[]
  decision: {
    index: number
    kind: 'acl' | 'grant'
    action: string
    src: string[]
    dst: string[]
    matched_src: string
    matched_dst: string
  } | null
  rules: AclRuleOutcome[]
  ssh_rules: AclRuleOutcome[]
  notes: string[]
}

export interface AccessQuery {
  src: string
  /** One or more destinations; each is evaluated on its own. */
  dsts: string[]
  port: number
  protocol: string
  /** Tests the editor's working copy instead of the saved policy. */
  policy?: string
}

/** One identity a rule names, with what it resolves to. */
export interface TopologyIdentity {
  selector: string
  kind: 'any' | 'tag' | 'group' | 'autogroup' | 'address' | 'user' | 'host'
  machines: number
  sample: string[]
}

/** One rule, seen as an edge between two identities. */
export interface TopologyEdge {
  rule: number
  kind: 'acl' | 'grant' | 'ssh'
  action: string
  src: string
  dst: string
  ports: string
  src_machines: number
  dst_machines: number
  users: string[]
}

export interface TopologyRoute {
  cidr: string
  approved: boolean
  advertisers: { id: string; name: string; online: boolean }[]
  /** A single node stands between the tailnet and that network. */
  sole: boolean
  exit_node: boolean
}

export interface TopologyRelay {
  region: string
  machines: { id: string; name: string; online: boolean }[]
}

/** Policy keys that do not form edges. Each value may be absent or null. */
export interface TopologyPolicy {
  autoApprovers?: AutoApprovers | null
  nodeAttrs?: NodeAttr[] | null
  postures?: Record<string, string[]> | null
  tests?: unknown[] | null
  sshTests?: unknown[] | null
  randomizeClientPort?: boolean | null
}

export interface TopologyResponse {
  identities: TopologyIdentity[]
  edges: TopologyEdge[]
  routes: TopologyRoute[]
  relays: TopologyRelay[]
  policy: TopologyPolicy
  totals: { machines: number; online: number; rules: number }
}

/** Headscale's own OIDC settings, as the form needs them. */
export interface OidcSettings {
  configured: boolean
  issuer: string
  clientId: string
  clientSecretSet: boolean
  clientSecretPath: string
  scope: string[]
  legacyExpiry: boolean
  useExpiryFromToken: boolean
  pkceEnabled: boolean
  onlyStartIfAvailable: boolean
}

/** The DERP section of the Headscale config file. */
export interface DerpConfig {
  urls: string[]
  paths: string[]
  autoUpdateEnabled: boolean
  updateFrequency: string
  serverEnabled: boolean
}

export interface DnsRecord {
  name: string
  type: string
  value: string
}

export interface DnsResponse {
  dns: {
    magic_dns: boolean
    base_domain: string | null
    nameservers: string[]
    split_dns: Record<string, string[]>
    search_domains: string[]
    override_dns: boolean
    extra_records: DnsRecord[]
  }
  access: { read: boolean; write: boolean; writable: boolean; available: boolean }
  integration: string
}

export interface AuthKeysResponse {
  keys: PreAuthKey[]
  missing: string[]
  users: HeadscaleUser[]
  selfServiceOnly: boolean
  access: { any: boolean; own: boolean; linkedHeadscaleUserId: string | null }
  server: string
}

export interface RestrictionsResponse {
  restrictions: {
    issuer: string | null
    allowed_domains: string[]
    allowed_groups: string[]
    allowed_users: string[]
  }
  access: { read: boolean; write: boolean; writable: boolean }
}

/** How a setting value is edited. */
export type SettingKind = 'text' | 'number' | 'bool' | 'path' | 'list' | 'url'

/** Where the effective value comes from. */
export type SettingSource = 'default' | 'database' | 'environment' | null

/** One editable key in Sailplane's own configuration. */
export interface SettingsEntry {
  /** Dotted key, for example `headscale.url`. */
  key: string
  /** Section the key belongs to: `server`, `headscale`, `oidc`, and so on. */
  group: string
  kind: SettingKind
  secret: boolean
  restartRequired: boolean
  /** Effective value. Always null for secrets, which are never returned. */
  value: unknown
  /** True when a value is stored. For secrets, the value itself stays hidden. */
  set: boolean
  source: SettingSource
}

export interface SettingsResponse {
  settings: SettingsEntry[]
}

/** First-run onboarding state. `GET /api/setup/status` needs no session. */
export interface SetupStatus {
  required: boolean
  headscaleUrl: string
  hasApiKey: boolean
}

// --- Endpoints ---

export const api = {
  session: () => get<SessionView>('api/session'),

  login: (apiKey: string) => post<{ ok: boolean }>('api/auth/login', { api_key: apiKey }),
  logout: () => post<{ ok: boolean; redirect: string | null }>('api/auth/logout'),

  machines: {
    list: () => get<MachineListResponse>('api/machines'),
    detail: (id: string) => get<{ machine: Machine; magic: string | null; access: { read: boolean; write: boolean }; supports: MachineListResponse['supports'] }>(`api/machines/${encodeURIComponent(id)}`),
    register: (registerKey: string, user: string) =>
      post<{ machine: Machine }>('api/machines/register', { register_key: registerKey, user }),
    rename: (id: string, name: string) => post(`api/machines/${encodeURIComponent(id)}/rename`, { name }),
    remove: (id: string) => del(`api/machines/${encodeURIComponent(id)}`),
    expire: (id: string) => post(`api/machines/${encodeURIComponent(id)}/expire`),
    setExpiry: (id: string, disableExpiry: boolean) =>
      post(`api/machines/${encodeURIComponent(id)}/expiry`, { disableExpiry }),
    setTags: (id: string, tags: string[]) => post(`api/machines/${encodeURIComponent(id)}/tags`, { tags }),
    setRoute: (id: string, route: string, enabled: boolean) =>
      post(`api/machines/${encodeURIComponent(id)}/routes`, { route, enabled }),
    setOwner: (id: string, user: string) => post(`api/machines/${encodeURIComponent(id)}/owner`, { user }),
  },

  users: {
    list: () => get<UserListResponse>('api/users'),
    create: (username: string, displayName?: string, email?: string) =>
      post('api/users', { username, display_name: displayName, email }),
    remove: (id: string) => del(`api/users/${encodeURIComponent(id)}`),
    rename: (id: string, newName: string) =>
      post(`api/users/${encodeURIComponent(id)}/rename`, { new_name: newName }),
    setGroups: (name: string, groups: string[]) =>
      put(`api/users/${encodeURIComponent(name)}/groups`, { groups }),
  },

  accounts: {
    setRole: (id: string, role: string) => post(`api/accounts/${id}/role`, { role }),
    link: (id: string, headscaleUserId: string) =>
      post(`api/accounts/${id}/link`, { headscale_user_id: headscaleUserId }),
    transferOwnership: (id: string) => post(`api/accounts/${id}/transfer-ownership`),
    remove: (id: string) => del(`api/accounts/${id}`),
  },

  acl: {
    get: () => get<AclResponse>('api/acl'),
    set: (policy: string) =>
      put<{ ok: boolean; policy: string; updatedAt: string; warnings: string[] }>('api/acl', {
        policy,
      }),
    simulate: (query: AccessQuery) =>
      post<{ reports: AccessReport[]; policy: 'saved' | 'draft' }>('api/acl/simulate', query),
  },

  topology: {
    get: () => get<TopologyResponse>('api/topology'),
  },

  oidc: {
    get: () =>
      get<{
        oidc: OidcSettings
        access: { read: boolean; write: boolean; writable: boolean; available: boolean }
        integration: string | null
      }>('api/oidc'),
    update: (patch: Record<string, unknown>) =>
      post<{ ok: boolean; warning: string | null }>('api/oidc', patch),
  },

  derp: {
    get: () =>
      get<{
        derp: DerpConfig
        access: { read: boolean; write: boolean; writable: boolean; available: boolean }
        integration: string | null
      }>('api/derp'),
    update: (patch: Partial<{
      urls: string[]
      paths: string[]
      auto_update_enabled: boolean
      update_frequency: string
      server_enabled: boolean
    }>) => post<{ ok: boolean; warning: string | null }>('api/derp', patch),
  },

  dns: {
    get: () => get<DnsResponse>('api/dns'),
    renameTailnet: (newName: string) => post('api/dns/tailnet', { new_name: newName }),
    toggleMagic: (enabled: boolean) => post('api/dns/magic', { enabled }),
    addNameserver: (ns: string, splitName?: string) =>
      post('api/dns/nameservers', { ns, split_name: splitName }),
    removeNameserver: (ns: string, splitName?: string) =>
      post('api/dns/nameservers/remove', { ns, split_name: splitName }),
    addSearchDomain: (domain: string) => post('api/dns/search-domains', { domain }),
    removeSearchDomain: (domain: string) => post('api/dns/search-domains/remove', { domain }),
    addRecord: (record: DnsRecord) =>
      post('api/dns/records', { name: record.name, type: record.type, value: record.value }),
    removeRecord: (record: DnsRecord) =>
      post('api/dns/records/remove', { name: record.name, type: record.type }),
    setOverride: (enabled: boolean) => post('api/dns/override', { enabled }),
  },

  authKeys: {
    list: () => get<AuthKeysResponse>('api/auth-keys'),
    create: (payload: {
      user_id?: string | null
      acl_tags: string[]
      expiry_days: number
      reusable: boolean
      ephemeral: boolean
    }) => post<{ key: PreAuthKey; command: string }>('api/auth-keys', payload),
    expire: (payload: { key_id: string; key: string; user_id?: string | null }) =>
      post('api/auth-keys/expire', payload),
    remove: (payload: { key_id: string; key: string; user_id?: string | null }) =>
      post('api/auth-keys/delete', payload),
  },

  apiKeys: {
    list: () => get<{ keys: HeadscaleApiKey[]; canRevoke: boolean }>('api/api-keys'),
    revoke: (id: string) => post('api/api-keys/revoke', { id }),
  },

  audit: {
    list: (before?: number) =>
      get<{ entries: AuditEntry[]; next: number | null }>(
        before === undefined ? 'api/audit' : `api/audit?before=${before}`,
      ),
  },

  restrictions: {
    get: () => get<RestrictionsResponse>('api/restrictions'),
    update: (action: 'add' | 'remove', kind: 'domains' | 'groups' | 'users', value: string) =>
      post('api/restrictions', { action, kind, value }),
  },

  agent: {
    status: () => get<{ agent: AgentStatus }>('api/agent'),
    sync: () => post<{ ok: boolean; error?: string; nodeCount?: number; agent: AgentStatus }>('api/agent/sync'),
  },

  settings: {
    get: () => get<SettingsResponse>('api/settings'),
    /** A null value deletes the setting. */
    update: (values: Record<string, unknown>) =>
      put<{ ok: boolean; restartRequired: string[] }>('api/settings', { values }),
    validate: (values: Record<string, unknown>) =>
      post<{ valid: boolean; error?: string }>('api/settings/validate', { values }),
    import: (yaml: string) => post<{ ok: boolean }>('api/settings/import', { yaml }),
  },

  setup: {
    status: () => get<SetupStatus>('api/setup/status'),
    /** The setup token goes in `x-setup-token`; loopback callers may omit it. */
    testHeadscale: (url: string, apiKey: string, token?: string) =>
      post<{ ok: boolean; version: string | null }>(
        'api/setup/test-headscale',
        { url, api_key: apiKey },
        token ? { 'x-setup-token': token } : undefined,
      ),
    complete: (
      payload: { url: string; api_key: string; base_url?: string; cookie_secure?: boolean },
      token?: string,
    ) =>
      post<{ ok: boolean }>(
        'api/setup/complete',
        payload,
        token ? { 'x-setup-token': token } : undefined,
      ),
  },
}

/** Where OIDC sign-in starts. */
export const oidcStartUrl = () => resolve('oidc/start')
