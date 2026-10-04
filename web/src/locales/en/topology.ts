export default {
  title: 'Topology',
  description:
    'How the tailnet is wired: which identities the policy connects, which networks rest on a single node, and where each machine’s relay is.',
  loadFailed: 'Could not load the topology',
  noRules: 'The policy has no rules',
  noRulesDescription: 'With no ACL rules, nothing in this tailnet can reach anything.',
  summary: {
    nodes: '{count} machines',
    online: '{count} online',
    rules: '{count} rules',
    soleRoutes: '{count} single-homed routes',
  },
  sole: {
    title: 'One node is the only way through',
    body: '{routes} is advertised by a single machine. If that machine goes offline, the network stays unreachable until another node advertises the route.',
  },
  graph: {
    title: 'Access graph',
    description:
      'Each line is a rule, labelled with the ports it allows. Dotted lines are Tailscale SSH rules, which apply to port 22 only.',
    label: 'Access graph',
    machineCount: '{count} machine | {count} machines',
    ruleNumber: 'rule {index}',
    portLabel: 'ports {ports}',
    reach: 'reaches {to} machines from {from}',
    users: 'login names {users}',
    hint: 'Select a line for the rule behind it.',
    acl: 'ACL',
    ssh: 'SSH',
    legendAcl: '{count} ACL rules',
    legendSsh: '{count} SSH rules',
  },
  routes: {
    title: 'Advertised routes',
    description: 'Networks a machine offers to route, and whether anything else could route them.',
    empty: 'No routes advertised',
    emptyDescription:
      'A machine becomes a subnet router by advertising routes. Run `tailscale set --advertise-routes=…` on it.',
    approved: 'approved',
    pending: 'pending approval',
    sole: 'single point of failure',
    exitNode: 'exit node',
  },
  relays: {
    title: 'Relay regions',
    description:
      'Each machine keeps a connection to its home relay and uses it when no direct path exists. This comes from the netmap the agent reports.',
    region: 'Region {id}',
    unknown: 'No relay reported',
    count: '{count} machines',
    endpoints: '{count} endpoints',
  },
  empty: 'No machines yet',
  emptyDescription: 'Register a machine to see the tailnet take shape.',
}
