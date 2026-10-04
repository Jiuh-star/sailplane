export default {
  description: 'Manage Sailplane and your Headscale deployment.',
  sections: {
    authKeys: {
      title: 'Pre-authentication keys',
      description: 'Create and expire the keys machines use to join your tailnet.',
    },
    agent: {
      title: 'Sailplane agent',
      description: 'Collect per-machine information that the Headscale API does not expose.',
    },
    restrictions: {
      title: 'Authentication restrictions',
      description: 'Limit which identities may sign in to Headscale through OIDC.',
    },
    apiKeys: {
      title: 'Headscale API keys',
      description: 'Review and revoke the credentials that can administer this control plane.',
    },
    audit: {
      title: 'Audit log',
      description: 'Every change made through Sailplane, and by whom.',
    },
    oidc: {
      title: 'Single sign-on',
      description: 'Point Headscale at an OpenID Connect provider.',
    },
    derp: {
      title: 'DERP',
      description: 'Where the relays come from, and how often the map is refetched.',
    },
    logs: {
      title: 'Headscale logs',
      description: 'Read the container log without a shell on the host.',
    },
  },
  nothingToConfigure: {
    title: 'Nothing to configure',
    description: 'Your account does not have access to any settings sections.',
  },
  deployment: {
    title: 'Deployment',
    description: 'How this Sailplane instance is wired up.',
    sailplaneVersion: 'Sailplane version',
    headscaleVersion: 'Headscale version',
    reloadIntegration: 'Reload integration',
    headscaleConfig: 'Headscale config',
    singleSignOn: 'Single sign-on',
    agentBackend: 'Agent backend',
  },
  configState: {
    readWrite: 'Read/write',
    readOnly: 'Read-only',
    unavailable: 'Unavailable',
  },
}
