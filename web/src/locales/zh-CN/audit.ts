export default {
  title: '审计日志',
  description:
    '所有通过 Sailplane 做出的变更，最新的在最前。Headscale 自身不保留历史记录，因此这里是"谁做了什么"的唯一凭据。',
  refresh: '刷新',
  loadFailed: '无法加载审计日志',
  count: '{count} 条记录',
  failedCount: '{count} 条失败',
  rowsPerPage: '每页行数',
  previous: '上一页',
  next: '下一页',
  pageOf: '第 {page} / {pages} 页',
  empty: '暂无记录',
  emptyDescription: '通过 Sailplane 做出的变更会显示在这里。',
  table: {
    when: '时间',
    actor: '操作者',
    action: '操作',
    result: '结果',
  },
}
