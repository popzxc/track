import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'

import ProjectsPage from './ProjectsPage.vue'
import { buildProject } from '../testing/factories'

describe('ProjectsPage', () => {
  it('emits selection and edit events for the visible project details', async () => {
    const projectA = buildProject({ canonicalName: 'project-a' })
    const projectB = buildProject({ canonicalName: 'project-b', aliases: ['proj-b'] })

    const wrapper = mount(ProjectsPage, {
      props: {
        projects: [projectA, projectB],
        selectedProjectDetails: projectB,
        selectedProjectId: projectB.canonicalName,
      },
    })

    expect(wrapper.get(`[data-project-id="${projectB.canonicalName}"]`).classes()).toContain('bg-bg0/55')

    await wrapper.get(`[data-project-id="${projectA.canonicalName}"]`).trigger('click')
    await wrapper.get('[data-testid="edit-project-button"]').trigger('click')

    expect(wrapper.emitted('request-select-project')).toEqual([[projectA.canonicalName]])
    expect(wrapper.emitted('request-edit-project')).toEqual([[projectB]])
    expect(wrapper.text()).toContain('Aliases')
  })

  it('shows the empty state when no projects exist', () => {
    const wrapper = mount(ProjectsPage, {
      props: {
        projects: [],
        selectedProjectDetails: null,
        selectedProjectId: null,
      },
    })

    expect(wrapper.text()).toContain('No projects yet.')
    expect(wrapper.findAll('[data-testid="project-row"]')).toHaveLength(0)
  })
})
