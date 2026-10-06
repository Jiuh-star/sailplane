/**
 * English messages, split by area. Adding a language means adding a sibling
 * directory with the same file names and key structure.
 */
import acls from './acls'
import agent from './agent'
import apiKeys from './api-keys'
import audit from './audit'
import authKeys from './auth-keys'
import common from './common'
import derp from './derp'
import dns from './dns'
import home from './home'
import login from './login'
import logs from './logs'
import machine from './machine'
import machines from './machines'
import nav from './nav'
import oidc from './oidc'
import restrictions from './restrictions'
import settings from './settings'
import setup from './setup'
import ssh from './ssh'
import topology from './topology'
import users from './users'

export default {
  common,
  nav,
  login,
  home,
  machines,
  machine,
  users,
  acls,
  derp,
  dns,
  settings,
  ssh,
  topology,
  authKeys,
  restrictions,
  agent,
  apiKeys,
  audit,
  logs,
  oidc,
  setup,
}
