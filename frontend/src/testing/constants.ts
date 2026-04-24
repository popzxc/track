import type { RemoteAgentPreferredTool } from '../api/types'

export const TOOL_CONSTANTS = {
  CODEX: 'codex' as const,
  CLAUDE: 'claude' as const,
} as const

export const ALL_TOOLS: RemoteAgentPreferredTool[] = [
  TOOL_CONSTANTS.CODEX,
  TOOL_CONSTANTS.CLAUDE,
]
