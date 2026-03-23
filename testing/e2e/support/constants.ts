import path from 'node:path'
import { fileURLToPath } from 'node:url'

const SUPPORT_ROOT = path.dirname(fileURLToPath(import.meta.url))

export const REPO_ROOT = path.resolve(SUPPORT_ROOT, '../../..')
export const FRONTEND_ROOT = path.join(REPO_ROOT, 'frontend')
export const FIXTURECTL_PATH = path.join(REPO_ROOT, 'testing/support/fixturectl.py')
export const FRONTEND_E2E_STATE_PATH = '/tmp/track-frontend-e2e-state.json'

export const FIXTURE_IMAGE = 'track-testing/ssh-fixture:local'
export const FIXTURE_HOST = '127.0.0.1'
export const FIXTURE_USER = 'track'
export const FIXTURE_WORKSPACE_ROOT = '/home/track/workspace'
export const FIXTURE_PROJECTS_REGISTRY_PATH = '/srv/track-testing/state/track-projects.json'
export const FIXTURE_SHELL_PRELUDE =
  'export PATH="/opt/track-testing/bin:$PATH"\nexport TRACK_TESTING_RUNTIME_DIR="/srv/track-testing"'

export const E2E_PROJECT_NAME = 'project-a'
export const E2E_REPO_URL = 'https://github.com/acme/project-a'
// The SSH fixture does not impersonate GitHub itself. Instead, it exposes the
// seeded upstream repository as a local bare repo inside the mounted runtime
// directory. Keeping the browser e2e on that same contract avoids re-solving
// GitHub authentication inside the mock box and matches the live Rust fixture.
export const E2E_GIT_URL = `/srv/track-testing/git/upstream/${E2E_PROJECT_NAME}.git`
export const E2E_PR_URL = 'https://github.com/acme/project-a/pull/42'

export const DISPATCH_TASK_TITLE = 'Dispatch browser smoke test'
export const FOLLOW_UP_TASK_TITLE = 'Continue browser smoke test'
export const ORPHAN_CLEANUP_TASK_ID = '20260323-120500-clean-browser-orphans'
export const ORPHAN_CLEANUP_DISPATCH_ID = 'dispatch-browser-orphan-cleanup'
