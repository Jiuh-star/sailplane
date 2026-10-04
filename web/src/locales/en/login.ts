export default {
  title: 'Welcome to Sailplane',
  subtitle: 'Sign in to manage your Headscale deployment',
  cardTitle: 'Sign in',
  cardDescription: 'Use a Headscale API key, or continue with your identity provider.',
  signIn: 'Sign in',
  signingIn: 'Signing in…',
  or: 'or',
  continueWithSso: 'Continue with single sign-on',
  apiKeyLabel: 'Headscale API key',
  apiKeyHint: 'Create one with {command}.',
  couldNotSignIn: 'Could not sign in',
  signedOutTitle: 'Signed out',
  signedOutNotice: 'You have been signed out.',
  signInProblemTitle: 'Sign-in problem',
  insecureCookies: {
    title: 'Secure cookies over plain HTTP',
    body: 'Sailplane is served over plain HTTP but {setting} is enabled, so the browser will discard the session cookie. Serve the UI over HTTPS or disable secure cookies.',
  },
  oidc: {
    errorNoQuery: 'The identity provider returned no authorization code.',
    errorNoSession: 'The sign-in attempt expired. Try again.',
    errorInvalidSession: 'The sign-in session was invalid. Try again.',
    errorNoSub: 'The identity provider did not return a subject identifier.',
    errorAuthFailed: 'Sign-in failed. Try again.',
    discoveryFailed:
      'Sailplane cannot reach the OpenID Connect discovery document. Single sign-on stays unavailable until the provider responds.',
    missingEndpoints:
      'The OpenID Connect provider is missing required endpoints (authorization, token or JWKS).',
    invalidApiKey: 'Single sign-on requires a Headscale API key to link accounts.',
    notConfigured: 'Single sign-on is not configured.',
  },
}
