export default {
  description: '管理 Sailplane 与你的 Headscale 部署。',
  sections: {
    authKeys: {
      title: '预授权密钥',
      description: '创建和失效设备加入网络所使用的密钥。',
    },
    agent: {
      title: 'Sailplane 代理',
      description: '采集 Headscale API 未提供的每台设备信息。',
    },
    restrictions: {
      title: '身份验证限制',
      description: '限制哪些身份可以通过 OIDC 登录 Headscale。',
    },
    apiKeys: {
      title: 'Headscale API 密钥',
      description: '查看并吊销能够管理这个控制面的凭据。',
    },
    audit: {
      title: '审计日志',
      description: '记录通过 Sailplane 做出的每一次变更及其操作者。',
    },
    oidc: {
      title: '单点登录',
      description: '为 Headscale 配置 OpenID Connect 身份提供方。',
    },
    derp: {
      title: 'DERP',
      description: '中继来源，以及地图的刷新频率。',
    },
    logs: {
      title: 'Headscale 日志',
      description: '无需登录主机即可查看容器日志。',
    },
  },
  nothingToConfigure: {
    title: '没有可配置的内容',
    description: '你的账户无权访问任何设置板块。',
  },
  deployment: {
    title: '部署',
    description: '此 Sailplane 实例的部署方式。',
    sailplaneVersion: 'Sailplane 版本',
    headscaleVersion: 'Headscale 版本',
    reloadIntegration: '重载集成',
    headscaleConfig: 'Headscale 配置',
    singleSignOn: '单点登录',
    agentBackend: '代理后端',
  },
  configState: {
    readWrite: '读写',
    readOnly: '只读',
    unavailable: '不可用',
  },
}
