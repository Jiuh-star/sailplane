export default {
  title: 'Headscale API keys',
  description: 'The credentials Sailplane and other automation use to reach the control plane.',
  warningTitle: 'These keys are root-equivalent',
  warningBody:
    'Anyone holding one can administer every user, machine and policy on this Headscale. Revoke any key you do not recognize. Prefer one key per integration, so you can revoke it on its own.',
  copyCommand: 'Copy the command',
  copied: 'Copied',
  summary: '{active} active of {total}',
  revoke: 'Revoke',
  expired: 'Expired',
  never: 'Never',
  empty: 'No API keys',
  emptyDescription: 'Sailplane authenticates with a key that no longer appears here.',
  loadFailed: 'Could not load the API keys',
  createTitle: 'Creating a key',
  createBody:
    'Headscale prints a new key once and never shows it again. Store it where the integration can read it, then reload Sailplane.',
  table: {
    title: 'Existing keys',
    prefix: 'Prefix',
    created: 'Created',
    lastSeen: 'Last used',
    expires: 'Expires',
    actions: 'Actions',
  },
  revokeDialog: {
    title: 'Revoke this API key?',
    description:
      '{prefix} stops working immediately. Any integration still using it will fail until you give it a new key.',
    confirm: 'Revoke key',
  },
  toast: {
    revoked: 'API key revoked',
    revokeFailed: 'Could not revoke the key',
  },
}
