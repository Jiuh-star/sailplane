export default {
  title: 'SSH',
  unavailable: '此 Sailplane 实例未启用浏览器 SSH。',
  enable: {
    title: '如何启用浏览器 SSH',
    disabled:
      '浏览器 SSH 已关闭。请在「设置 → SSH」中把 integration.ssh.enabled 设为 true。再把 integration.ssh.proxy 指向 tailscaled sidecar 的 SOCKS5 代理地址（即 TS_SOCKS5_SERVER 的值）。',
    proxy:
      '配置的 SSH 代理不可达。请启动 tailscaled sidecar，并确保它开放了 SOCKS5 代理（TS_SOCKS5_SERVER）。再把 integration.ssh.proxy 设为该地址。',
    version:
      '当前 Headscale 版本不支持浏览器 SSH。请升级到 Headscale 0.28.0 或更高版本。0.29.0 与 0.29.1 存在 /ts2021 缺陷，请使用 0.29.2 及以后。',
    offline: '设备处于离线状态。请先使其上线，然后重试。',
    no_ipv4: '该设备没有可用于连接的 Tailscale IPv4 地址。',
  },
  connecting: '正在连接 {host}…',
  connectionFailed: 'SSH 连接失败。',
  failedTitle: '无法打开会话',
  sessionClosed: '会话已结束。',
  open: '打开 SSH 会话',
  hostKeyUnverified: '主机密钥未验证',
  hostKeyHint:
    '服务器接受目标主机提供的任意主机密钥。Tailscale SSH 通过 WireGuard 识别对端，没有主机密钥。普通 sshd 目标由管理员主动选择，视为可信。',
  prompt: {
    title: '登录',
    description: '输入登录用户名。Tailscale SSH 通过设备身份鉴权，无需密码。',
    user: 'SSH 用户',
    connect: '连接',
  },
  status: {
    connecting: '连接中…',
    connected: '已连接',
    closed: '已断开',
  },
}
