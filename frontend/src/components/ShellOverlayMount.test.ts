import { describe, expect, it } from 'vitest'
import { shallowMount } from '@vue/test-utils'

import ShellOverlayMount from './ShellOverlayMount.vue'
import {
  buildDispatch,
  buildProject,
  buildRemoteAgentSettings,
  buildReview,
  buildReviewRun,
  buildTask,
} from '../testing/factories'

function createProps() {
  const selectedTask = buildTask()
  const selectedReview = buildReview()
  const selectedTaskLatestDispatch = buildDispatch({
    taskId: selectedTask.id,
    project: selectedTask.project,
  })
  const selectedReviewLatestRun = buildReviewRun({
    reviewId: selectedReview.id,
  })

  return {
    availableProjects: [buildProject()],
    cancelingDispatchTaskId: null,
    cancelingReviewId: null,
    cleanupPendingConfirmation: false,
    cleaningUpRemoteArtifacts: false,
    creatingReview: false,
    creatingTask: false,
    defaultCreateProject: 'project-a',
    defaultRemoteAgentPreferredTool: 'codex' as const,
    dispatchingTaskId: null,
    discardingDispatchTaskId: null,
    editingProject: null,
    editingRemoteAgentSetup: false,
    editingTask: null,
    followingUpDispatch: selectedTaskLatestDispatch,
    followingUpReview: null,
    followingUpReviewId: null,
    followingUpTask: null,
    followingUpTaskId: null,
    remoteAgentSettings: buildRemoteAgentSettings(),
    resetPendingConfirmation: false,
    resettingRemoteWorkspace: false,
    reviewPendingDeletion: null,
    runnerSetupRequiredForDispatch: false,
    saving: false,
    selectedReview,
    selectedReviewCanCancel: true,
    selectedReviewCanReReview: true,
    selectedReviewLatestRun,
    selectedReviewRuns: [selectedReviewLatestRun],
    selectedTask,
    selectedTaskCanContinue: true,
    selectedTaskCanDiscardHistory: true,
    selectedTaskCanStartFresh: true,
    selectedTaskDispatchDisabledReason: undefined,
    selectedTaskDispatchTool: 'codex' as const,
    selectedTaskLatestDispatch,
    selectedTaskLatestReusablePullRequest: selectedTaskLatestDispatch.pullRequestUrl ?? null,
    selectedTaskLifecycleMessage: '',
    selectedTaskLifecycleMutation: null,
    selectedTaskPinnedTool: 'codex' as const,
    selectedTaskPrimaryActionDisabled: false,
    selectedTaskProject: buildProject(),
    selectedTaskRuns: [{ task: selectedTask, dispatch: selectedTaskLatestDispatch }],
    showReviewDrawer: false,
    showTaskDrawer: false,
    taskPendingDeletion: null,
  }
}

describe('ShellOverlayMount', () => {
  it('forwards task drawer events using the selected task context', () => {
    const props = createProps()
    const wrapper = shallowMount(ShellOverlayMount, {
      props: {
        ...props,
        showTaskDrawer: true,
      },
    })

    const taskDrawer = wrapper.findComponent({ name: 'TaskDrawer' })
    expect(taskDrawer.exists()).toBe(true)

    taskDrawer.vm.$emit('request-close-task')
    taskDrawer.vm.$emit('request-delete-task')
    taskDrawer.vm.$emit('request-discard-history')
    taskDrawer.vm.$emit('request-edit-task')
    taskDrawer.vm.$emit('request-open-project')
    taskDrawer.vm.$emit('request-open-url', 'https://example.com/task')
    taskDrawer.vm.$emit('request-primary-action')
    taskDrawer.vm.$emit('request-start-fresh')
    taskDrawer.vm.$emit('update:start-tool', 'claude')

    expect(wrapper.emitted('request-selected-task-close')).toEqual([[props.selectedTask]])
    expect(wrapper.emitted('request-selected-task-delete')).toEqual([[props.selectedTask]])
    expect(wrapper.emitted('request-selected-task-discard-history')).toEqual([[props.selectedTask]])
    expect(wrapper.emitted('request-edit-task')).toEqual([[props.selectedTask]])
    expect(wrapper.emitted('request-open-task-project')).toEqual([[]])
    expect(wrapper.emitted('request-open-url')).toEqual([['https://example.com/task']])
    expect(wrapper.emitted('request-selected-task-primary-action')).toEqual([[]])
    expect(wrapper.emitted('request-selected-task-start-fresh')).toEqual([[props.selectedTask]])
    expect(wrapper.emitted('update:task-start-tool')).toEqual([['claude']])
  })

  it('forwards review drawer, modal, and confirmation events', () => {
    const props = createProps()
    const wrapper = shallowMount(ShellOverlayMount, {
      props: {
        ...props,
        creatingReview: true,
        reviewPendingDeletion: props.selectedReview,
        showReviewDrawer: true,
      },
    })

    wrapper.findComponent({ name: 'ReviewDrawer' }).vm.$emit('request-cancel-review-run', props.selectedReview)
    wrapper.findComponent({ name: 'ReviewDrawer' }).vm.$emit('request-delete-review', props.selectedReview)
    wrapper.findComponent({ name: 'ReviewDrawer' }).vm.$emit('request-rereview', props.selectedReview)
    wrapper.findComponent({ name: 'ReviewRequestModal' }).vm.$emit('save', {
      pullRequestUrl: props.selectedReview.pullRequestUrl,
      preferredTool: 'codex',
    })
    wrapper.findAllComponents({ name: 'ConfirmDialog' })[1]?.vm.$emit('confirm')

    expect(wrapper.emitted('request-cancel-review-run')).toEqual([[props.selectedReview]])
    expect(wrapper.emitted('request-delete-review')).toEqual([[props.selectedReview]])
    expect(wrapper.emitted('request-review-follow-up')).toEqual([[props.selectedReview]])
    expect(wrapper.emitted('request-save-review')).toEqual([[
      {
        pullRequestUrl: props.selectedReview.pullRequestUrl,
        preferredTool: 'codex',
      },
    ]])
    expect(wrapper.emitted('confirm-review-delete')).toEqual([[]])
  })
})
