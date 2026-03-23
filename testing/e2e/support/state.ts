import fs from 'node:fs'

import { FRONTEND_E2E_STATE_PATH } from './constants'

export interface FrontendE2EState {
  apiBaseUrl: string
  apiPid: number
  apiPort: number
  containerName: string
  fixturePort: number
  runtimeRoot: string
  tempRoot: string
}

export function saveFrontendE2EState(state: FrontendE2EState): void {
  fs.writeFileSync(FRONTEND_E2E_STATE_PATH, `${JSON.stringify(state, null, 2)}\n`, 'utf-8')
}

export function loadFrontendE2EState(): FrontendE2EState {
  return JSON.parse(fs.readFileSync(FRONTEND_E2E_STATE_PATH, 'utf-8')) as FrontendE2EState
}

export function clearFrontendE2EState(): void {
  if (!fs.existsSync(FRONTEND_E2E_STATE_PATH)) {
    return
  }

  fs.rmSync(FRONTEND_E2E_STATE_PATH, { force: true })
}
