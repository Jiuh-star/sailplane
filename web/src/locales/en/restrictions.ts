export default {
  title: 'Authentication restrictions',
  description: 'Limit which identities can sign in to Headscale through OpenID Connect.',
  loadFailedTitle: 'Could not load the restrictions',
  readOnly: {
    title: 'Read-only',
    description: 'Your account can view but not change the authentication restrictions.',
  },
  notWritable: {
    title: 'Headscale configuration is not writable',
    description:
      'Restrictions are stored in the Headscale configuration file, which Sailplane cannot write to.',
  },
  allowEveryoneHint: 'Leave empty to let everyone from your identity provider sign in.',
  sections: {
    domains: {
      title: 'Permitted domains',
      empty: 'All domains can authenticate',
      placeholder: 'example.com',
    },
    groups: {
      title: 'Permitted groups',
      empty: 'All groups can authenticate',
      placeholder: 'engineering',
    },
    users: {
      title: 'Permitted users',
      empty: 'All users can authenticate',
      placeholder: "alice{'@'}example.com",
    },
  },
  toast: {
    added: 'Restriction added',
    addFailed: 'Could not add the restriction',
    removed: 'Restriction removed',
    removeFailed: 'Could not remove the restriction',
  },
}
