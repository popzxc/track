import { describe, expect, it, vi } from 'vitest'
import { computed, effectScope, nextTick, ref } from 'vue'

import { useProjectViewState } from './useProjectViewState'
import { buildProject } from '../testing/factories'

describe('useProjectViewState', () => {
  it('keeps project selection anchored to real projects and opens the details page explicitly', async () => {
    const closeTaskDrawer = vi.fn()
    const currentPage = ref<'tasks' | 'reviews' | 'runs' | 'projects' | 'settings'>('tasks')
    const selectedProjectFilter = ref('missing-project')
    const availableProjects = ref([
      buildProject({ canonicalName: 'project-a' }),
      buildProject({ canonicalName: 'project-b' }),
    ])

    const scope = effectScope()
    const state = scope.run(() =>
      useProjectViewState({
        availableProjects: computed(() => availableProjects.value),
        closeTaskDrawer,
        currentPage,
        selectedProjectFilter,
      }),
    )

    if (!state) {
      throw new Error('Expected project view state')
    }

    await nextTick()

    expect(state.selectedProjectDetailsId.value).toBe('project-a')
    expect(selectedProjectFilter.value).toBe('')

    state.selectProjectDetails(availableProjects.value[1])

    expect(state.selectedProjectDetailsId.value).toBe('project-b')
    expect(currentPage.value).toBe('projects')
    expect(closeTaskDrawer).toHaveBeenCalledTimes(1)

    scope.stop()
  })
})
