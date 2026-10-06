export default {
  title: '初始化 Sailplane',
  subtitle: '将 Sailplane 连接到你的 Headscale 服务器。',
  errorTitle: '初始化失败',
  back: '上一步',
  next: '下一步',
  steps: {
    welcome: {
      title: '欢迎',
      description: '一次性的简短初始化。',
    },
    headscale: {
      title: 'Headscale',
      description: '将 Sailplane 指向你的控制服务器。',
    },
    sailplane: {
      title: 'Sailplane',
      description: '告诉 Sailplane 如何被访问。',
    },
    finish: {
      title: '完成',
      description: '确认并保存。',
    },
  },
  welcome: {
    noteTitle: '开始之前',
    noteBody:
      '这些页面只在初始化完成前开放。Sailplane 将配置保存在自己的数据库中，请妥善保管数据目录。',
    tokenLabel: '初始化令牌',
    tokenPlaceholder: '一次性令牌',
    tokenHint: '服务器在首次启动时会把令牌打印到日志。若你在运行 Sailplane 的机器上打开本页，可留空。',
  },
  headscale: {
    urlLabel: 'Headscale URL',
    apiKeyLabel: 'Headscale API 密钥',
    apiKeyPlaceholder: 'hskey-api-…',
    apiKeyHint: '使用 `headscale apikeys create` 创建一个。',
    test: '测试连接',
    testing: '测试中…',
    testOk: '连接成功。',
    testOkVersion: '连接成功。Headscale {version}。',
  },
  sailplane: {
    baseUrlLabel: '公开基础 URL',
    baseUrlHint: '可选。当 Sailplane 运行在反向代理之后时设置。',
    cookieSecureLabel: '安全 Cookie',
    cookieSecureHint: '仅通过 HTTPS 发送会话 Cookie。使用明文 HTTP 时请关闭。',
  },
  finish: {
    summaryUrl: 'Headscale URL',
    summaryBaseUrl: '基础 URL',
    summaryCookies: '安全 Cookie',
    note: 'Headscale 保留自己的设置。之后可在“部署设置”中再次修改这些值。',
    submit: '完成初始化',
    submitting: '正在完成…',
    none: '未设置',
  },
}
