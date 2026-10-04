export default {
  title: 'SSH',
  unavailable: '此 Sailplane 实例未启用浏览器 SSH。',
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
