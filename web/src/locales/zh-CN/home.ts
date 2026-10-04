export default {
  title: '你的网络',
  subtitle: '使用 Tailscale 客户端将设备连接到你的网络。',
  checkingAccount: '正在检查你的账户…',
  link: {
    title: '关联你的 Headscale 账户',
    description: '选择此登录所属的 Headscale 用户。Sailplane 会根据关联关系决定你可以管理哪些设备。',
    userLabel: 'Headscale 用户',
    userPlaceholder: '选择用户',
    submit: '关联并继续',
    success: '账户已关联',
    errorTitle: '无法关联账户',
  },
  notLinked: {
    title: '你的账户尚未关联',
    description:
      'Sailplane 无法将你的登录匹配到 Headscale 用户，因此你暂时无法管理设备。管理员可以在用户页面进行关联。',
  },
  linked: {
    title: '已关联账户',
    description: '此登录已关联到 Headscale 用户 {name}。',
  },
  access: {
    title: '接入你的网络',
    description: '安装 Tailscale 并登录，即可加入该网络。',
  },
}
