export default {
  title: 'Sailplane agent',
  description: 'Reports the details of each machine that the Headscale API does not expose.',
  loading: 'Loading agent status…',
  loadFailedTitle: 'Could not load the agent status',
  disabled: {
    title: 'Agent not enabled',
    description:
      'Enable it under {config} to see per-machine versions, endpoints and client connectivity.',
  },
  status: {
    healthy: 'Healthy',
    error: 'Error',
    pendingApproval: 'Waiting for approval',
  },
  lastSynced: 'Last synced {time}',
  nodesReported: '{count} machines reported | {count} machine reported | {count} machines reported',
  syncNow: 'Sync now',
  approval: {
    title: 'Agent needs approval',
    description: "The agent's own machine has not been approved on the tailnet yet.",
    action: 'Approve the agent',
  },
  syncError: {
    title: 'Sync error',
  },
  toast: {
    synced: 'Agent synced',
    syncFailed: 'Agent sync failed',
  },
}
