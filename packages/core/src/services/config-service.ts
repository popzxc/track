import { readFile } from 'node:fs/promises'

import { trackConfigSchema, type TrackConfig } from '@track/shared'

import { TrackError } from '../errors'
import { collapseHomePath, getConfigPath, resolvePathFromConfig } from '../utils/path-utils'

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
