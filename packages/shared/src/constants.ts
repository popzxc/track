import type { Priority, Status } from './task-types'

export const DEFAULT_CONFIG_PATH = '~/.config/track/config.json'
export const DEFAULT_DATA_DIR = '~/.track/issues'
export const OPENAI_TASK_MODEL = 'gpt-4.1-nano'
export const DEFAULT_LLAMA_CPP_BINARY = 'llama-cli'
export const TASK_FILE_EXTENSION = '.md'
export const TASK_SLUG_MAX_LENGTH = 60

export const PRIORITY_RANK: Record<Priority, number> = {
  high: 0,
  medium: 1,
  low: 2,
}

export const STATUS_RANK: Record<Status, number> = {
  open: 0,
  closed: 1,
}
