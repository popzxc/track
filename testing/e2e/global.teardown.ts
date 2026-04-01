import { spawnSync } from 'node:child_process'

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

  // The SSH fixture may have created files owned by a different UID inside the
  // temp tree.  Use the shell rm so that the process inherits any elevated
  // capabilities that the host runner may have, and ignore any residual
  // failures so teardown does not block CI even when some files cannot be
  // deleted (the ephemeral runner will be discarded anyway).
  spawnSync('rm', ['-rf', state.tempRoot])
  clearFrontendE2EState()
}

export default teardownFrontendE2EEnvironment
