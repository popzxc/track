import { describe, expect, it } from 'vitest'

import {
  dispatchStatusLabel,
  drawerPrimaryAction,
  getRunStartDisabledReason,
  groupTasksByProject,
  latestDispatchByTaskId,
  mergeProjects,
} from './presentation'
import {
  buildDispatch,
  buildProject,
  buildRemoteAgentSettings,
  buildRunRecord,
  buildTask,
} from '../../testing/factories'

describe('groupTasksByProject', () => {
  it('creates alphabetical project sections while preserving task order within each section', () => {
    const grouped = groupTasksByProject([
      buildTask({ id: 'zeta/open/one.md', project: 'zeta', description: 'Zeta one' }),
      buildTask({ id: 'alpha/open/one.md', project: 'alpha', description: 'Alpha one' }),
      buildTask({ id: 'zeta/open/two.md', project: 'zeta', description: 'Zeta two' }),
    ])

    expect(grouped.map((group) => group.project)).toEqual(['alpha', 'zeta'])
    expect(grouped[1].tasks.map((task) => task.id)).toEqual(['zeta/open/one.md', 'zeta/open/two.md'])
  })
})

describe('latestDispatchByTaskId', () => {
  it('keeps the newest dispatch for each task', () => {
    const task = buildTask()
    const latestByTaskId = latestDispatchByTaskId([
      buildRunRecord(task, { dispatchId: 'dispatch-old', createdAt: '2026-03-23T12:05:00.000Z' }),
      buildRunRecord(task, { dispatchId: 'dispatch-new', createdAt: '2026-03-23T12:07:00.000Z' }),
    ])

    expect(latestByTaskId[task.id]?.dispatchId).toBe('dispatch-new')
  })
})

describe('dispatchStatusLabel', () => {
  it('distinguishes recent and historical failures', () => {
    const recentFailure = buildDispatch({
      status: 'failed',
      finishedAt: '2026-03-23T12:10:00.000Z',
      updatedAt: '2026-03-23T12:10:00.000Z',
    })
    const historicalFailure = buildDispatch({
      status: 'failed',
      finishedAt: '2026-03-23T11:00:00.000Z',
      updatedAt: '2026-03-23T11:00:00.000Z',
    })
    const now = Date.parse('2026-03-23T12:20:00.000Z')

    expect(dispatchStatusLabel(recentFailure, now)).toBe('Failed')
    expect(dispatchStatusLabel(historicalFailure, now)).toMatch(/^Failed on /)
  })
})

describe('drawerPrimaryAction', () => {
  it.each([
    {
      message: 'closed tasks reopen instead of dispatching',
      task: buildTask({ status: 'closed' }),
      dispatch: undefined,
      expected: 'reopen',
    },
    {
      message: 'active dispatches expose cancellation',
      task: buildTask(),
      dispatch: buildDispatch({ status: 'running' }),
      expected: 'cancel',
    },
    {
      message: 'completed reusable runs continue from existing context',
      task: buildTask(),
      dispatch: buildDispatch({ status: 'succeeded', branchName: 'track/dispatch-1', worktreePath: '/tmp/worktree' }),
      expected: 'continue',
    },
    {
      message: 'new tasks start fresh dispatches',
      task: buildTask(),
      dispatch: undefined,
      expected: 'start',
    },
  ])('$message', ({ task, dispatch, expected }) => {
    expect(drawerPrimaryAction(task, dispatch)).toBe(expected)
  })
})

describe('getRunStartDisabledReason', () => {
  it('requires complete project metadata before dispatching', () => {
    const task = buildTask()
    const projects = [buildProject({ metadata: undefined })]

    expect(getRunStartDisabledReason(task, projects, buildRemoteAgentSettings())).toBe(
      'Project details are not available yet.',
    )
  })

  it('guides the user toward local remote-agent setup when it is missing', () => {
    const task = buildTask()

    expect(
      getRunStartDisabledReason(task, [buildProject()], buildRemoteAgentSettings({ configured: false })),
    ).toBe('Configure the remote agent with `track` locally before dispatching tasks.')
  })
})

describe('mergeProjects', () => {
  it('keeps persisted metadata when task-derived placeholders arrive later', () => {
    const merged = mergeProjects(
      [buildProject()],
      [buildProject({
        aliases: [],
        metadata: {
          repoUrl: '',
          gitUrl: '',
          baseBranch: '',
          description: undefined,
        },
      })],
    )

    expect(merged).toEqual([buildProject()])
  })
})
