export default {
  title: 'Audit log',
  description:
    'Every change made through Sailplane, newest first. Headscale keeps no history of its own, so this is the only record of who did what.',
  refresh: 'Refresh',
  loadFailed: 'Could not load the audit log',
  count: '{count} entries',
  failedCount: '{count} failed',
  rowsPerPage: 'Rows per page',
  previous: 'Previous page',
  next: 'Next page',
  pageOf: 'Page {page} of {pages}',
  empty: 'Nothing recorded yet',
  emptyDescription: 'Changes made through Sailplane will appear here.',
  table: {
    when: 'When',
    actor: 'Actor',
    action: 'Action',
    result: 'Result',
  },
}
