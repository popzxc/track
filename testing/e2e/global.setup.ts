import fs from 'node:fs'
import { mkdir, mkdtemp, writeFile, copyFile, chmod, readFile } from 'node:fs/promises'
import path from 'node:path'
import net from 'node:net'
import { spawn, spawnSync } from 'node:child_process'
import { Database } from 'bun:sqlite'

import {
  DISPATCH_TASK_TITLE,
  E2E_GIT_URL,
  ORPHAN_CLEANUP_DISPATCH_ID,
  ORPHAN_CLEANUP_TASK_ID,
  E2E_PR_URL,
  E2E_PROJECT_NAME,
  E2E_REPO_URL,
  FIXTURECTL_PATH,
  FIXTURE_HOST,
  FIXTURE_IMAGE,
  FIXTURE_PROJECTS_REGISTRY_PATH,
  FIXTURE_SHELL_PRELUDE,
  FIXTURE_USER,
  FIXTURE_WORKSPACE_ROOT,
  FRONTEND_ROOT,
  REPO_ROOT,
} from './support/constants'
import { saveFrontendE2EState } from './support/state'

const FOLLOW_UP_TASK_TITLE = 'Continue browser smoke test'

// =============================================================================
// Browser E2E Environment Bootstrap
// =============================================================================
//
// The expensive end-to-end suite should exercise the same production contracts
// as the Rust live integration tests: a real SSH fixture, a real `track-api`
// process, and the built frontend assets served by that API. Keeping setup here
// makes the browser tests a consumer of that shared contract instead of a
// separate universe with browser-only mocks.
export async function setupFrontendE2EEnvironment(): Promise<void> {
  ensureFrontendBuild()

  const tempRoot = await mkdtemp('/tmp/track-frontend-e2e-')
  const runtimeRoot = path.join(tempRoot, 'fixture-runtime')
  const keyPrefix = path.join(tempRoot, 'fixture-key', 'id_ed25519')
  const localTrackRoot = path.join(tempRoot, 'track')
  const issuesRoot = path.join(localTrackRoot, 'issues')
  const remoteAgentRoot = path.join(localTrackRoot, 'remote-agent')
  const configPath = path.join(tempRoot, 'config', 'config.json')
  const containerName = `track-frontend-e2e-${Date.now()}`
  const fixturePort = await reserveLocalPort()
  const apiPort = await reserveLocalPort()
  const apiBaseUrl = `http://127.0.0.1:${apiPort}`

  await mkdir(runtimeRoot, { recursive: true })
  await mkdir(path.dirname(keyPrefix), { recursive: true })
  await mkdir(remoteAgentRoot, { recursive: true })

  ensureFixtureImage()
  runFixtureCtl([
    'generate-key',
    '--output-prefix',
    keyPrefix,
  ])
  await chmod(keyPrefix, 0o600)
  runFixtureCtl([
    'run',
    '--image',
    FIXTURE_IMAGE,
    '--name',
    containerName,
    '--port',
    String(fixturePort),
    '--runtime-dir',
    runtimeRoot,
    '--authorized-key',
    `${keyPrefix}.pub`,
  ])
  runFixtureCtl([
    'wait-for-ssh',
    '--host',
    FIXTURE_HOST,
    '--user',
    FIXTURE_USER,
    '--port',
    String(fixturePort),
    '--private-key',
    keyPrefix,
    '--known-hosts',
    path.join(runtimeRoot, 'known_hosts'),
    '--timeout-seconds',
    '20',
  ])
  runFixtureCtl([
    'seed-repo',
    '--runtime-dir',
    runtimeRoot,
    '--repo-url',
    E2E_REPO_URL,
    '--base-branch',
    'main',
    '--fork-owner',
    'fixture-user',
    '--login',
    'fixture-user',
  ])

  await writeCodexState(runtimeRoot)
  await copyFile(keyPrefix, path.join(remoteAgentRoot, 'id_ed25519'))
  await chmod(path.join(remoteAgentRoot, 'id_ed25519'), 0o600)
  await writeFile(path.join(remoteAgentRoot, 'known_hosts'), '', 'utf-8')
  await writeConfigFile(configPath, fixturePort, apiPort)

  const apiLogPath = path.join(tempRoot, 'track-api.log')
  const apiLog = fs.createWriteStream(apiLogPath, { flags: 'a' })
  const apiProcess = spawn('cargo', ['run', '-p', 'track-api'], {
    cwd: REPO_ROOT,
    detached: true,
    env: {
      ...process.env,
      PORT: String(apiPort),
      TRACK_CONFIG_PATH: configPath,
      TRACK_DATA_DIR: issuesRoot,
      TRACK_STATE_DIR: localTrackRoot,
      TRACK_STATIC_ROOT: path.join(REPO_ROOT, 'frontend', 'dist'),
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  apiProcess.stdout?.pipe(apiLog)
  apiProcess.stderr?.pipe(apiLog)
  apiProcess.unref()

  await waitForHealth(`${apiBaseUrl}/health`, apiLogPath)
  await configureRemoteAgent(apiBaseUrl, fixturePort, keyPrefix, runtimeRoot)
  await seedApplicationData(apiBaseUrl)
  await seedOrphanedCleanupArtifacts({
    fixturePort,
    keyPath: keyPrefix,
    runtimeRoot,
    stateDir: localTrackRoot,
  })

  saveFrontendE2EState({
    apiBaseUrl,
    apiPid: apiProcess.pid ?? 0,
    apiPort,
    containerName,
    fixturePort,
    runtimeRoot,
    tempRoot,
  })
}

export default setupFrontendE2EEnvironment

function ensureFrontendBuild(): void {
  runCommand('bun', ['run', 'build'], { cwd: FRONTEND_ROOT })
}

function ensureFixtureImage(): void {
  const inspectResult = spawnSync('docker', ['image', 'inspect', FIXTURE_IMAGE], {
    cwd: REPO_ROOT,
    encoding: 'utf-8',
  })

  if (inspectResult.status === 0) {
    return
  }

  runFixtureCtl(['build-image', '--image', FIXTURE_IMAGE])
}

function runFixtureCtl(args: string[]): void {
  runCommand('python3', [FIXTURECTL_PATH, ...args], { cwd: REPO_ROOT })
}

function runRemoteCommand(
  options: { fixturePort: number; keyPath: string; knownHostsPath: string },
  script: string,
): void {
  runCommand(
    'ssh',
    [
      '-i',
      options.keyPath,
      '-p',
      String(options.fixturePort),
      '-o',
      'BatchMode=yes',
      '-o',
      'IdentitiesOnly=yes',
      '-o',
      'StrictHostKeyChecking=accept-new',
      '-o',
      `UserKnownHostsFile=${options.knownHostsPath}`,
      `${FIXTURE_USER}@${FIXTURE_HOST}`,
      'bash',
      '-lc',
      script,
    ],
    { cwd: REPO_ROOT },
  )
}

function runCommand(
  command: string,
  args: string[],
  options: { cwd: string },
): void {
  const completed = spawnSync(command, args, {
    cwd: options.cwd,
    encoding: 'utf-8',
  })

  if (completed.status === 0) {
    return
  }

  throw new Error([
    `Command failed: ${command} ${args.join(' ')}`,
    completed.stdout?.trim() ? `stdout:\n${completed.stdout.trim()}` : '',
    completed.stderr?.trim() ? `stderr:\n${completed.stderr.trim()}` : '',
  ].filter(Boolean).join('\n\n'))
}

async function reserveLocalPort(): Promise<number> {
  return await new Promise((resolve, reject) => {
    const server = net.createServer()
    server.on('error', reject)
    server.listen(0, '127.0.0.1', () => {
      const address = server.address()
      if (!address || typeof address === 'string') {
        reject(new Error('Could not reserve a local TCP port.'))
        return
      }

      const { port } = address
      server.close((error) => {
        if (error) {
          reject(error)
          return
        }

        resolve(port)
      })
    })
  })
}

async function writeCodexState(runtimeRoot: string): Promise<void> {
  const codexStatePath = path.join(runtimeRoot, 'state', 'codex.json')
  await mkdir(path.dirname(codexStatePath), { recursive: true })
  await writeFile(
    codexStatePath,
    `${JSON.stringify({
      mode: 'success',
      sleepSeconds: 1,
      status: 'succeeded',
      summary: 'Mock Codex completed the task and opened a PR.',
      pullRequestUrl: E2E_PR_URL,
      reviewSubmitted: true,
      reviewBody: '@octocat requested me to review this PR.\n\nI did not find blocking issues in the fixture diff.',
      notes: 'Generated by the frontend browser e2e harness.',
      createCommit: {
        message: 'fix: apply mocked codex change',
        files: [
          {
            path: 'MOCK_CODEX_CHANGE.md',
            contents: '# Mock change\n\nCreated by the browser e2e fixture.\n',
          },
        ],
      },
    }, null, 2)}\n`,
    'utf-8',
  )
}

async function writeConfigFile(
  configPath: string,
  fixturePort: number,
  apiPort: number,
): Promise<void> {
  await mkdir(path.dirname(configPath), { recursive: true })
  await writeFile(
    configPath,
    `${JSON.stringify({
      projectRoots: [],
      projectAliases: {},
      api: {
        port: apiPort,
      },
      remoteAgent: {
        host: FIXTURE_HOST,
        user: FIXTURE_USER,
        port: fixturePort,
        workspaceRoot: FIXTURE_WORKSPACE_ROOT,
        projectsRegistryPath: FIXTURE_PROJECTS_REGISTRY_PATH,
        shellPrelude: FIXTURE_SHELL_PRELUDE,
        reviewFollowUp: {
          enabled: false,
          mainUser: 'octocat',
          defaultReviewPrompt: 'Focus on regressions and missing tests.',
        },
      },
    }, null, 2)}\n`,
    'utf-8',
  )
}

async function waitForHealth(healthUrl: string, apiLogPath: string): Promise<void> {
  const deadline = Date.now() + 90_000

  while (Date.now() < deadline) {
    try {
      const response = await fetch(healthUrl)
      if (response.ok) {
        return
      }
    } catch {
      // The process is still starting. We retry until either it becomes ready
      // or the timeout expires with the captured API log attached.
    }

    await new Promise((resolve) => setTimeout(resolve, 250))
  }

  const apiLog = fs.existsSync(apiLogPath)
    ? fs.readFileSync(apiLogPath, 'utf-8').trim()
    : '(no API log captured)'
  throw new Error(`track-api did not become healthy in time.\n\n${apiLog}`)
}

async function configureRemoteAgent(
  apiBaseUrl: string,
  fixturePort: number,
  keyPath: string,
  runtimeRoot: string,
): Promise<void> {
  const sshPrivateKey = await readFile(keyPath, 'utf-8')
  const knownHosts = await readFile(path.join(runtimeRoot, 'known_hosts'), 'utf-8').catch(() => '')

  const response = await fetch(`${apiBaseUrl}/api/remote-agent`, {
    method: 'PUT',
    headers: {
      'content-type': 'application/json',
    },
    body: JSON.stringify({
      host: FIXTURE_HOST,
      user: FIXTURE_USER,
      port: fixturePort,
      workspaceRoot: FIXTURE_WORKSPACE_ROOT,
      projectsRegistryPath: FIXTURE_PROJECTS_REGISTRY_PATH,
      shellPrelude: FIXTURE_SHELL_PRELUDE,
      reviewFollowUp: {
        enabled: false,
        mainUser: 'octocat',
        defaultReviewPrompt: 'Focus on regressions and missing tests.',
      },
      sshPrivateKey,
      knownHosts,
    }),
  })

  if (!response.ok) {
    throw new Error(`Failed to configure remote agent: ${await response.text()}`)
  }
}

async function seedApplicationData(apiBaseUrl: string): Promise<void> {
  await upsertProject(apiBaseUrl)
  await createTask(apiBaseUrl, DISPATCH_TASK_TITLE)
  await createTask(apiBaseUrl, FOLLOW_UP_TASK_TITLE)
}

async function seedOrphanedCleanupArtifacts(options: {
  fixturePort: number
  keyPath: string
  runtimeRoot: string
  stateDir: string
}): Promise<void> {
  const orphanWorktreePath =
    `${FIXTURE_WORKSPACE_ROOT}/${E2E_PROJECT_NAME}/worktrees/${ORPHAN_CLEANUP_DISPATCH_ID}`
  const orphanRunDirectory =
    `${FIXTURE_WORKSPACE_ROOT}/${E2E_PROJECT_NAME}/dispatches/${ORPHAN_CLEANUP_DISPATCH_ID}`

  // Seed the orphan dispatch record directly in SQLite so the API cleanup can
  // find it.  We insert a task row to satisfy any application-level checks,
  // then delete it — leaving the dispatch record as an orphan that the cleanup
  // endpoint is meant to detect and remove.
  const dbPath = path.join(options.stateDir, 'track.sqlite')
  const db = new Database(dbPath)
  try {
    db.run(
      `INSERT OR IGNORE INTO tasks
         (id, project, priority, status, description, created_at, updated_at)
       VALUES (?, ?, ?, ?, ?, ?, ?)`,
      [
        ORPHAN_CLEANUP_TASK_ID,
        E2E_PROJECT_NAME,
        'low',
        'open',
        'Orphaned task seeded for browser cleanup e2e.',
        '2026-03-23T12:00:00.000Z',
        '2026-03-23T12:00:00.000Z',
      ],
    )
    db.run(
      `INSERT OR IGNORE INTO task_dispatches
         (dispatch_id, task_id, project, status, created_at, updated_at, finished_at,
          remote_host, branch_name, worktree_path, preferred_tool, summary)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
      [
        ORPHAN_CLEANUP_DISPATCH_ID,
        ORPHAN_CLEANUP_TASK_ID,
        E2E_PROJECT_NAME,
        'succeeded',
        '2026-03-23T12:05:00.000Z',
        '2026-03-23T12:06:00.000Z',
        '2026-03-23T12:06:00.000Z',
        FIXTURE_HOST,
        `track/${ORPHAN_CLEANUP_DISPATCH_ID}`,
        orphanWorktreePath,
        'codex',
        'Left behind on purpose for the browser cleanup e2e.',
      ],
    )
    // Delete the task so the dispatch record becomes an orphan.  Because the
    // task_dispatches table no longer has an ON DELETE CASCADE constraint, the
    // dispatch record survives the task deletion and is eligible for cleanup.
    db.run('DELETE FROM tasks WHERE id = ?', [ORPHAN_CLEANUP_TASK_ID])
  } finally {
    db.close()
  }

  runRemoteCommand(
    {
      fixturePort: options.fixturePort,
      keyPath: options.keyPath,
      knownHostsPath: path.join(options.runtimeRoot, 'known_hosts'),
    },
    `
      set -eu
      mkdir -p "${orphanWorktreePath}" "${orphanRunDirectory}"
      printf 'orphaned worktree\\n' > "${orphanWorktreePath}/README.txt"
      printf 'completed\\n' > "${orphanRunDirectory}/status.txt"
    `,
  )
}

async function upsertProject(apiBaseUrl: string): Promise<void> {
  const response = await fetch(`${apiBaseUrl}/api/projects/${encodeURIComponent(E2E_PROJECT_NAME)}`, {
    method: 'PUT',
    headers: {
      'content-type': 'application/json',
    },
    body: JSON.stringify({
      repoUrl: E2E_REPO_URL,
      gitUrl: E2E_GIT_URL,
      baseBranch: 'main',
      description: 'Seed metadata for browser e2e tests.',
    }),
  })

  if (!response.ok) {
    throw new Error(`Could not seed project metadata: ${await response.text()}`)
  }
}

async function createTask(apiBaseUrl: string, title: string): Promise<void> {
  const response = await fetch(`${apiBaseUrl}/api/tasks`, {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
    },
    body: JSON.stringify({
      project: E2E_PROJECT_NAME,
      priority: 'high',
      description: `${title}\n\n## Summary\nExercise the browser-driven frontend flow.`,
    }),
  })

  if (!response.ok) {
    throw new Error(`Could not create the seeded task "${title}": ${await response.text()}`)
  }
}
