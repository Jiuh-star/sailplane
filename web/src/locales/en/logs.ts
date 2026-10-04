export default {
  title: 'Headscale logs',
  description:
    'The container log, straight from the runtime. Only the docker integration can read it: Headscale exposes no log endpoint of its own.',
  history: 'History',
  lines: '{count} lines',
  follow: 'Follow',
  start: 'Start',
  stop: 'Stop',
  reload: 'Reload',
  clear: 'Clear',
  lineCount: '{count} lines',
  failedTitle: 'Could not read the logs',
  noStream: 'The server returned no log stream.',
  waiting: 'Waiting for output…',
  idle: 'Press Start to read the log.',
}
