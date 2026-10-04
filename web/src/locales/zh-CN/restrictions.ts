export default {
  title: '身份验证限制',
  description: '限制哪些身份可以通过 OpenID Connect 登录 Headscale。',
  loadFailedTitle: '无法加载身份验证限制',
  readOnly: {
    title: '只读',
    description: '你的账户只能查看身份验证限制，无法修改。',
  },
  notWritable: {
    title: 'Headscale 配置不可写',
    description: '限制存储在 Headscale 配置文件中，Sailplane 无法写入该文件。',
  },
  allowEveryoneHint: '留空表示允许身份提供商中的所有用户。',
  sections: {
    domains: {
      title: '允许的域名',
      empty: '允许所有域名进行身份验证',
      placeholder: 'example.com',
    },
    groups: {
      title: '允许的用户组',
      empty: '允许所有用户组进行身份验证',
      placeholder: 'engineering',
    },
    users: {
      title: '允许的用户',
      empty: '允许所有用户进行身份验证',
      placeholder: "alice{'@'}example.com",
    },
  },
  toast: {
    added: '限制已添加',
    addFailed: '无法添加限制',
    removed: '限制已移除',
    removeFailed: '无法移除限制',
  },
}
