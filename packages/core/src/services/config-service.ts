import { readFile } from 'node:fs/promises'

import { trackConfigSchema, type TrackConfig } from '@track/shared'

import { TrackError } from '../errors'
import { collapseHomePath, getConfigPath, resolvePathFromConfig } from '../utils/path-utils'

// =============================================================================
// Config Normalization
// =============================================================================
//
// The config file is the shared contract between the CLI and the API, so this
// service does more than just "read JSON":
// - it turns user-facing paths like `~/...` into absolute runtime paths
// - it resolves the selected AI provider into a concrete discriminated shape
// - it fails early with user-friendly messages before the rest of the app starts
//
function resolveOptionalCommandPath(pathValue: string | undefined): string | undefined {
  if (!pathValue) {
    return undefined
  }

  if (pathValue.startsWith('~/') || pathValue.startsWith('./') || pathValue.startsWith('../') || pathValue.includes('/')) {
    return resolvePathFromConfig(pathValue)
  }

  return pathValue
}

export class ConfigService {
  private readonly configPath: string

  constructor(options?: { configPath?: string }) {
    this.configPath = getConfigPath(options?.configPath)
  }

  getResolvedPath(): string {
    return this.configPath
  }

  async loadConfig(): Promise<TrackConfig> {
    let rawConfig: string

    // First, we load the raw file and preserve a "config not found" error that
    // points to the human-facing path the user is expected to edit.
    try {
      rawConfig = await readFile(this.configPath, 'utf8')
    } catch (error) {
      if (typeof error === 'object' && error !== null && 'code' in error && error.code === 'ENOENT') {
        throw new TrackError(
          'CONFIG_NOT_FOUND',
          `Config file not found at ${collapseHomePath(this.configPath)}`,
          { cause: error },
        )
      }

      throw new TrackError('CONFIG_READ_FAILED', 'Could not read the track config file.', { cause: error })
    }

    let parsedJson: unknown
    try {
      parsedJson = JSON.parse(rawConfig)
    } catch (error) {
      throw new TrackError('INVALID_CONFIG', 'Config file is not valid JSON.', { cause: error })
    }

    const parsedConfig = trackConfigSchema.safeParse(parsedJson)
    if (!parsedConfig.success) {
      throw new TrackError('INVALID_CONFIG', 'Config file does not match the expected format.')
    }

    if (parsedConfig.data.ai.provider === 'llama-cpp' && !parsedConfig.data.ai.llamaCpp) {
      throw new TrackError('INVALID_CONFIG', 'Config file does not match the expected format.')
    }

    // Finally, we normalize the config into the runtime representation used by
    // the rest of the app. The goal is that downstream services never need to
    // remember path-expansion rules or provider-specific compatibility aliases.
    return {
      projectRoots: parsedConfig.data.projectRoots
        .map((projectRoot) => projectRoot.trim())
        .filter(Boolean)
        .map((projectRoot) => resolvePathFromConfig(projectRoot)),
      projectAliases: parsedConfig.data.projectAliases,
      ai: parsedConfig.data.ai.provider === 'llama-cpp'
        ? {
            provider: 'llama-cpp',
            llamaCpp: {
              modelPath: resolvePathFromConfig(parsedConfig.data.ai.llamaCpp!.modelPath),
              llamaCliPath: resolveOptionalCommandPath(
                parsedConfig.data.ai.llamaCpp!.llamaCliPath ?? parsedConfig.data.ai.llamaCpp!.binaryPath,
              ),
            },
          }
        : {
            provider: 'openai',
            openai: {
              model: parsedConfig.data.ai.openai?.model,
            },
          },
    }
  }
}
