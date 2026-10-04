export default {
  title: 'Headscale API 密钥',
  description: 'Sailplane 及其他自动化工具访问控制面所用的凭据。',
  warningTitle: '这些密钥等同于 root 权限',
  warningBody:
    '持有者可以管理这个 Headscale 上的所有用户、设备和策略。请吊销任何你不认识的密钥，并尽量为每个集成使用单独的密钥，以便单独吊销。',
  copyCommand: '复制命令',
  copied: '已复制',
  summary: '共 {total} 个密钥，{active} 个有效',
  revoke: '吊销',
  expired: '已过期',
  never: '永不过期',
  empty: '没有 API 密钥',
  emptyDescription: 'Sailplane 正在使用的密钥没有出现在这个列表里。',
  loadFailed: '无法加载 API 密钥',
  createTitle: '创建密钥',
  createBody:
    'Headscale 只显示一次新密钥，之后不再展示。请保存到集成方能够读取的地方，然后重启 Sailplane。',
  table: {
    title: '现有密钥',
    prefix: '前缀',
    created: '创建时间',
    lastSeen: '最近使用',
    expires: '过期时间',
    actions: '操作',
  },
  revokeDialog: {
    title: '吊销这把 API 密钥？',
    description: '{prefix} 会立即失效。仍在使用它的集成会报错，直到换上新的密钥。',
    confirm: '吊销密钥',
  },
  toast: {
    revoked: 'API 密钥已吊销',
    revokeFailed: '无法吊销该密钥',
  },
}
