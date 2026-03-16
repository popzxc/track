export interface ProjectInfo {
  canonicalName: string
  path: string
  aliases: string[]
}

export type TrackAiProvider = 'openai' | 'llama-cpp'

export interface OpenAiProviderConfig {
  provider: 'openai'
  openai: {
    model?: string
  }
}

export interface LlamaCppProviderConfig {
  provider: 'llama-cpp'
  llamaCpp: {
    modelPath: string
    llamaCliPath?: string
  }
}

export type TrackAiConfig = OpenAiProviderConfig | LlamaCppProviderConfig

export interface TrackConfig {
  projectRoots: string[]
  projectAliases: Record<string, string>
  ai: TrackAiConfig
}
