import { spawnSync } from 'node:child_process'
import fs from 'node:fs'

import { FIXTURECTL_PATH, REPO_ROOT } from './support/constants'
import {
  clearFrontendE2EState,
  loadFrontendE2EState,
} from './support/state'

export async function teardownFrontendE2EEnvironment(): Promise<void> {
  let state
  try {
    state = loadFrontendE2EState()
  } catch {
    clearFrontendE2EState()
    return
  }

  if (state.apiPid > 0) {
    try {
      process.kill(-state.apiPid, 'SIGTERM')
    } catch {
      try {
        process.kill(state.apiPid, 'SIGTERM')
      } catch {
        // The API may already be gone if the test failed early. Teardown should
        // stay best-effort so the fixture still gets cleaned up.
      }
    }
  }

  spawnSync('python3', [FIXTURECTL_PATH, 'stop', '--name', state.containerName], {
    cwd: REPO_ROOT,
    encoding: 'utf-8',
  })

  // The SSH fixture may have created files owned by a different UID. Ensure
  // the entire temp tree is world-writable before removing it so the host
  // runner can delete container-owned files.
  spawnSync('chmod', ['-R', '777', state.tempRoot])
  fs.rmSync(state.tempRoot, { force: true, recursive: true })
  clearFrontendE2EState()
}

export default teardownFrontendE2EEnvironment
