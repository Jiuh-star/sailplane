export default {
  title: 'DERP',
  description:
    'Where the tailnet gets its relays. Headscale builds this map from its configuration file. Every change here rewrites that file and reloads Headscale.',
  loadFailed: 'Could not read the DERP configuration',
  unavailableTitle: 'No configuration file',
  unavailableBody:
    'Set `headscale.config_path` so Sailplane can read and write the Headscale configuration.',
  readOnlyTitle: 'Read-only',
  readOnlyBody: 'Sailplane can read the configuration but not write it. Check the file permissions.',
  warningTitle: 'Saved, with a warning',
  failedTitle: 'Could not save the change',
  urls: {
    title: 'Relay map URLs',
    description: 'Headscale and every client fetch these URLs. The Tailscale default lives at controlplane.tailscale.com.',
    empty: 'No URL configured.',
  },
  paths: {
    title: 'Local map files',
    description: 'Paths on the Headscale host, for a map you maintain yourself.',
    empty: 'No file configured.',
    hint: 'Headscale reads the path, so the file must exist inside its container or on its host. The file can be absent here.',
  },
  updates: {
    title: 'Updates',
    description: 'How often Headscale refetches the relay map.',
    auto: 'Automatic updates',
    autoHint: 'Refetch the map on a schedule instead of only at startup.',
    frequency: 'Interval',
  },
  server: {
    title: 'Embedded DERP server',
    description: 'Run a relay inside Headscale itself, for a tailnet with no other reachable relay.',
    hint: 'To enable this, you must set `derp.server.stun_listen_addr`, a private key path and reachable ports. Edit those in the config file.',
  },
  relays: {
    title: 'Relay servers',
    description: 'The regions and servers the configured sources resolve to. Sailplane fetches the URLs live.',
    empty: 'No relay servers found',
    emptyDescription: 'Add a map URL, a local map file, or enable the embedded server above.',
    unnamed: 'Unnamed region',
    count: '{count} server | {count} servers',
    derpPort: 'DERP {port}',
    stunPort: 'STUN {port}',
  },
  toast: {
    urlAdded: 'Relay URL added',
    urlRemoved: 'Relay URL removed',
    pathAdded: 'Map file added',
    pathRemoved: 'Map file removed',
    updated: 'DERP configuration updated',
  },
}
