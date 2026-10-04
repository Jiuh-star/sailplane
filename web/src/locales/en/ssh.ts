export default {
  title: 'SSH',
  unavailable: 'Browser SSH is not available on this Sailplane instance.',
  connecting: 'Connecting to {host}…',
  connectionFailed: 'The SSH connection failed.',
  failedTitle: 'Could not open the session',
  sessionClosed: 'The session has ended.',
  open: 'Open SSH session',
  hostKeyUnverified: 'host key unverified',
  hostKeyHint:
    'The server accepts the host key the target presents. Tailscale SSH identifies peers through WireGuard and has no host key. For a plain sshd target, the operator chose to trust it.',
  prompt: {
    title: 'Sign in',
    description:
      'Enter the user to sign in as. Tailscale SSH authorises the machine, so no password is needed.',
    user: 'SSH user',
    connect: 'Connect',
  },
  status: {
    connecting: 'connecting…',
    connected: 'connected',
    closed: 'disconnected',
  },
}
