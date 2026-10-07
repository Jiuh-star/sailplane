export default {
  title: 'SSH',
  unavailable: 'Browser SSH is not available on this Sailplane instance.',
  enable: {
    title: 'How to enable browser SSH',
    disabled:
      "Browser SSH is off. In Settings → SSH, set integration.ssh.enabled to true. Point integration.ssh.proxy at the tailscaled sidecar's SOCKS5 proxy (the same address as TS_SOCKS5_SERVER).",
    proxy:
      'The configured SSH proxy is not reachable. Start the tailscaled sidecar and make sure that it exposes a SOCKS5 proxy (TS_SOCKS5_SERVER). Then set integration.ssh.proxy to that address.',
    version:
      'This Headscale version does not support browser SSH. Upgrade to Headscale 0.28.0 or newer. Versions 0.29.0 and 0.29.1 have a /ts2021 regression, so use 0.29.2 or later.',
    offline: 'The machine is offline. Bring it online, then retry.',
    no_ipv4: 'This machine has no tailnet IPv4 address to reach.',
  },
  connecting: 'Connecting to {host}…',
  connectionFailed: 'The SSH connection failed.',
  failedTitle: 'Could not open the session',
  sessionClosed: 'The session ended.',
  open: 'Open SSH session',
  hostKeyUnverified: 'host key unverified',
  hostKeyHint:
    'The server accepts the host key that the target presents. Tailscale SSH identifies peers through WireGuard and has no host key. For a plain sshd target, the operator chose to trust it.',
  prompt: {
    title: 'Sign in',
    description:
      'Enter the user to sign in as. Tailscale SSH authorizes the machine, so no password is necessary.',
    user: 'SSH user',
    connect: 'Connect',
  },
  status: {
    connecting: 'connecting…',
    connected: 'connected',
    closed: 'disconnected',
  },
}
