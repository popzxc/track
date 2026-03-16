export class TrackError extends Error {
  readonly code: string
  readonly status: number

  constructor(code: string, message: string, options?: { cause?: unknown; status?: number }) {
    super(message, options)
    this.name = 'TrackError'
    this.code = code
    this.status = options?.status ?? 400
  }
}

export function isTrackError(error: unknown): error is TrackError {
  return error instanceof TrackError
}

export function getErrorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message
  }

  return 'An unexpected error occurred.'
}
