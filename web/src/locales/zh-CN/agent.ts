export default {
  title: 'Sailplane 代理',
  description: '上报 Headscale API 未提供的每台设备详细信息。',
  loading: '正在加载代理状态…',
  loadFailedTitle: '无法加载代理状态',
  disabled: {
    title: '代理未启用',
    description:
      '在「设置」的 {config} 中打开「启用 Sailplane 代理」并保存，即可查看每台设备的版本、端点和客户端连接状态，保存后立即生效。',
  },
  status: {
    healthy: '正常',
    error: '错误',
    pendingApproval: '等待批准',
  },
  lastSynced: '上次同步：{time}',
  nodesReported: '{count} 台设备已上报',
  syncNow: '立即同步',
  approval: {
    title: '代理需要批准',
    description: '代理自身的设备尚未在网络中获得批准。',
    action: '批准代理',
  },
  syncError: {
    title: '同步错误',
  },
  toast: {
    synced: '代理已同步',
    syncFailed: '代理同步失败',
  },
}
