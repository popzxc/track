import { computed, nextTick, ref } from 'vue'
import { describe, expect, it, vi } from 'vitest'
import { shallowMount } from '@vue/test-utils'

import ProjectsScreen from './ProjectsScreen.vue'
import {
  buildProject,
} from '../testing/factories'

function createContext() {
  const project = buildProject()
  const editingProject = ref<ReturnType<typeof buildProject> | null>(null)
  const saveProjectEdits = vi.fn().mockResolvedValue(undefined)

  return {
    active: true,
    context: {
      availableProjects: computed(() => [project]),
      editingProject,
      saveProjectEdits,
      saving: ref(false),
      selectedProjectDetails: computed(() => project),
      selectedProjectDetailsId: ref(project.canonicalName),
    },
  }
}

describe('ProjectsScreen', () => {
  it('opens the metadata modal from the page surface', async () => {
    const wrapper = shallowMount(ProjectsScreen, {
      props: createContext(),
    })

    wrapper.findComponent({ name: 'ProjectsPage' }).vm.$emit('request-edit-project')
    await nextTick()

    expect(wrapper.findComponent({ name: 'ProjectMetadataModal' }).props('open')).toBe(true)
  })

  it('saves edits through the shared mutation handler', async () => {
    const props = createContext()
    props.context.editingProject.value = props.context.selectedProjectDetails.value
    const wrapper = shallowMount(ProjectsScreen, {
      props,
    })

    wrapper.findComponent({ name: 'ProjectMetadataModal' }).vm.$emit('save', {
      baseBranch: 'main',
      gitUrl: 'git@github.com:acme/project.git',
      repoUrl: 'https://github.com/acme/project',
    })
    await nextTick()

    expect(props.context.saveProjectEdits).toHaveBeenCalledWith({
      baseBranch: 'main',
      gitUrl: 'git@github.com:acme/project.git',
      repoUrl: 'https://github.com/acme/project',
    })
  })
})
