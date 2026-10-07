export default {
  title: 'Your tailnet',
  subtitle: 'Connect machines to your tailnet with the Tailscale client.',
  checkingAccount: 'Checking your account…',
  link: {
    title: 'Link your Headscale account',
    description:
      'Choose the Headscale user this login belongs to. Sailplane uses the link to decide which machines you can manage.',
    userLabel: 'Headscale user',
    userPlaceholder: 'Choose a user',
    submit: 'Link and continue',
    success: 'Account linked',
    errorTitle: 'Could not link the account',
  },
  notLinked: {
    title: 'Your account is not linked',
    description:
      'Sailplane could not match your login to a Headscale user, so you cannot manage machines yet. An administrator can link it from the Users page.',
  },
  linked: {
    title: 'Linked account',
    description: 'This login is linked to the Headscale user {name}.',
  },
  access: {
    title: 'Access your tailnet',
    description: 'Install Tailscale and sign in to join this tailnet.',
  },
}
