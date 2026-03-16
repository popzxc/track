import { homedir } from 'node:os'
import { isAbsolute, join, resolve } from 'node:path'

import { DEFAULT_CONFIG_PATH, DEFAULT_DATA_DIR } from '@track/shared'

export function expandHomePath(pathValue: string): string {
  if (pathValue === '~') {
    return homedir()
  }

  if (pathValue.startsWith('~/')) {
    return join(homedir(), pathValue.slice(2))
  }

  return pathValue
}

export function resolvePathFromConfig(pathValue: string): string {
  const expandedPath = expandHomePath(pathValue)
  return isAbsolute(expandedPath) ? expandedPath : resolve(expandedPath)
}

export function getConfigPath(overridePath = process.env.TRACK_CONFIG_PATH): string {
  return resolvePathFromConfig(overridePath ?? DEFAULT_CONFIG_PATH)
}

export function getDataDir(overridePath = process.env.TRACK_DATA_DIR): string {
  return resolvePathFromConfig(overridePath ?? DEFAULT_DATA_DIR)
}

export function collapseHomePath(pathValue: string): string {
  const userHome = homedir()
  return pathValue.startsWith(userHome) ? `~${pathValue.slice(userHome.length)}` : pathValue
}
