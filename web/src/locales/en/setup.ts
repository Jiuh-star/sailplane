export default {
  title: 'Set up Sailplane',
  subtitle: 'Connect Sailplane to your Headscale server.',
  errorTitle: 'Setup failed',
  back: 'Back',
  next: 'Next',
  steps: {
    welcome: {
      title: 'Welcome',
      description: 'A short, one-time setup.',
    },
    headscale: {
      title: 'Headscale',
      description: 'Point Sailplane at your control server.',
    },
    sailplane: {
      title: 'Sailplane',
      description: 'Tell Sailplane how it is reached.',
    },
    finish: {
      title: 'Finish',
      description: 'Review and save.',
    },
  },
  welcome: {
    noteTitle: 'Before you start',
    noteBody:
      'These pages are open only until setup finishes. Sailplane stores the configuration in its own database, so keep the data directory safe.',
    tokenLabel: 'Setup token',
    tokenPlaceholder: 'one-time token',
    tokenHint:
      'The server prints the token to its log at first boot. Leave this empty when you open this page on the machine that runs Sailplane.',
  },
  headscale: {
    urlLabel: 'Headscale URL',
    apiKeyLabel: 'Headscale API key',
    apiKeyPlaceholder: 'hskey-api-…',
    apiKeyHint: 'Create one with `headscale apikeys create`.',
    test: 'Test connection',
    testing: 'Testing…',
    testOk: 'Connection succeeded.',
    testOkVersion: 'Connection succeeded. Headscale {version}.',
  },
  sailplane: {
    baseUrlLabel: 'Public base URL',
    baseUrlHint: 'Optional. Set it when Sailplane runs behind a reverse proxy.',
    cookieSecureLabel: 'Secure cookies',
    cookieSecureHint: 'Send session cookies over HTTPS only. Disable it for plain HTTP.',
  },
  finish: {
    summaryUrl: 'Headscale URL',
    summaryBaseUrl: 'Base URL',
    summaryCookies: 'Secure cookies',
    note: 'Headscale itself keeps its own settings. You can change these again later in Deployment settings.',
    submit: 'Finish setup',
    submitting: 'Finishing…',
    none: 'Not set',
  },
}
