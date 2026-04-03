<script setup lang="ts">
import ConfirmDialog from './ConfirmDialog.vue'
import FollowUpModal from './FollowUpModal.vue'
import ProjectMetadataModal from './ProjectMetadataModal.vue'
import ReviewDrawer from './ReviewDrawer.vue'
import ReviewFollowUpModal from './ReviewFollowUpModal.vue'
import ReviewRequestModal from './ReviewRequestModal.vue'
import RemoteAgentSetupModal from './RemoteAgentSetupModal.vue'
import TaskDrawer from './TaskDrawer.vue'
import TaskEditorModal from './TaskEditorModal.vue'
import { drawerPrimaryAction } from '../features/tasks/presentation'
import { taskTitle } from '../features/tasks/description'
import type {
  CreateReviewInput,
  ProjectInfo,
  ProjectMetadataUpdateInput,
  RemoteAgentPreferredTool,
  RemoteAgentSettings,
  RemoteAgentSettingsUpdateInput,
  ReviewFollowUpInput,
  ReviewRecord,
  ReviewRunRecord,
  RunRecord,
  Task,
  TaskDispatch,
  TaskFollowUpInput,
} from '../types/task'

type TaskLifecycleMutation = 'closing' | 'reopening' | 'deleting'

const props = defineProps<{
  availableProjects: ProjectInfo[]
  cancelingDispatchTaskId: string | null
  cancelingReviewId: string | null
  cleanupPendingConfirmation: boolean
  cleaningUpRemoteArtifacts: boolean
  creatingReview: boolean
  creatingTask: boolean
  defaultCreateProject: string
  defaultRemoteAgentPreferredTool: RemoteAgentPreferredTool
  dispatchingTaskId: string | null
  discardingDispatchTaskId: string | null
  editingProject: ProjectInfo | null
  editingRemoteAgentSetup: boolean
  editingTask: Task | null
  followingUpDispatch?: TaskDispatch
  followingUpReview: ReviewRecord | null
  followingUpReviewId: string | null
  followingUpTask: Task | null
  followingUpTaskId: string | null
  remoteAgentSettings: RemoteAgentSettings | null
  resetPendingConfirmation: boolean
  resettingRemoteWorkspace: boolean
  reviewPendingDeletion: ReviewRecord | null
  runnerSetupRequiredForDispatch: boolean
  saving: boolean
  selectedReview: ReviewRecord | null
  selectedReviewCanCancel: boolean
  selectedReviewCanReReview: boolean
  selectedReviewLatestRun: ReviewRunRecord | null
  selectedReviewRuns: ReviewRunRecord[]
  selectedTask: Task | null
  selectedTaskCanContinue: boolean
  selectedTaskCanDiscardHistory: boolean
  selectedTaskCanStartFresh: boolean
  selectedTaskDispatchDisabledReason?: string
  selectedTaskDispatchTool: RemoteAgentPreferredTool
  selectedTaskLatestDispatch: TaskDispatch | null
  selectedTaskLatestReusablePullRequest: string | null
  selectedTaskLifecycleMessage: string
  selectedTaskLifecycleMutation: TaskLifecycleMutation | null
  selectedTaskPinnedTool: RemoteAgentPreferredTool | null
  selectedTaskPrimaryActionDisabled: boolean
  selectedTaskProject: ProjectInfo | null
  selectedTaskRuns: RunRecord[]
  showReviewDrawer: boolean
  showTaskDrawer: boolean
  taskPendingDeletion: Task | null
}>()

const emit = defineEmits<{
  'cancel-cleanup': []
  'cancel-project-editor': []
  'cancel-reset': []
  'cancel-review-delete': []
  'cancel-review-editor': []
  'cancel-review-follow-up': []
  'cancel-runner-setup': []
  'cancel-task-delete': []
  'cancel-task-drawer': []
  'cancel-task-editor': []
  'cancel-task-follow-up': []
  'cancel-review-drawer': []
  'confirm-cleanup': []
  'confirm-reset': []
  'confirm-review-delete': []
  'confirm-task-delete': []
  'request-cancel-review-run': [review: ReviewRecord]
  'request-delete-review': [review: ReviewRecord]
  'request-edit-task': [task: Task]
  'request-open-task-project': []
  'request-open-url': [url: string]
  'request-review-follow-up': [review: ReviewRecord]
  'request-save-project': [payload: ProjectMetadataUpdateInput]
  'request-save-review': [payload: CreateReviewInput]
  'request-save-review-follow-up': [payload: ReviewFollowUpInput]
  'request-save-runner-setup': [payload: RemoteAgentSettingsUpdateInput]
  'request-save-task': [payload: { description: string; priority: Task['priority']; project: string }]
  'request-save-task-follow-up': [payload: TaskFollowUpInput]
  'request-selected-task-close': [task: Task]
  'request-selected-task-delete': [task: Task]
  'request-selected-task-discard-history': [task: Task]
  'request-selected-task-primary-action': []
  'request-selected-task-start-fresh': [task: Task]
  'update:task-start-tool': [value: RemoteAgentPreferredTool]
}>()

// This component is intentionally "dumb shell chrome": it mounts every drawer,
// modal, and confirm dialog in one place, while App.vue still owns the state
// machines and mutation handlers behind those overlays.
function drawerPrimaryActionLabel(task: Task, dispatch?: TaskDispatch | null): string {
  switch (drawerPrimaryAction(task, dispatch)) {
    case 'reopen':
      return props.selectedTaskLifecycleMutation === 'reopening' ? 'Reopening...' : 'Reopen task'
    case 'cancel':
      return props.cancelingDispatchTaskId === task.id ? 'Canceling...' : 'Cancel run'
    case 'continue':
      return props.followingUpTaskId === task.id ? 'Continuing...' : 'Continue run'
    case 'start':
      return props.dispatchingTaskId === task.id ? 'Starting...' : 'Start agent'
  }
}

function drawerPrimaryActionClass(task: Task, dispatch?: TaskDispatch | null): string {
  switch (drawerPrimaryAction(task, dispatch)) {
    case 'reopen':
      return 'border border-yellow/30 bg-yellow/10 text-yellow hover:bg-yellow/15'
    case 'cancel':
      return 'border border-orange/30 bg-orange/10 text-orange hover:bg-orange/15'
    case 'continue':
      return 'border border-aqua/30 bg-aqua/10 text-aqua hover:bg-aqua/15'
    case 'start':
      return 'border border-blue/30 bg-blue/10 text-blue hover:bg-blue/15'
  }
}
</script>

<template>
  <TaskDrawer
    v-if="showTaskDrawer && selectedTask"
    :can-continue="selectedTaskCanContinue"
    :can-discard-history="selectedTaskCanDiscardHistory"
    :can-start-fresh="selectedTaskCanStartFresh"
    :dispatch-disabled-reason="selectedTaskDispatchDisabledReason"
    :is-discarding-history="discardingDispatchTaskId === selectedTask.id"
    :is-dispatching="dispatchingTaskId === selectedTask.id"
    :latest-dispatch="selectedTaskLatestDispatch"
    :latest-reusable-pull-request="selectedTaskLatestReusablePullRequest"
    :lifecycle-mutation="selectedTaskLifecycleMutation"
    :lifecycle-progress-message="selectedTaskLifecycleMessage"
    :pinned-tool="selectedTaskPinnedTool"
    :primary-action-class="drawerPrimaryActionClass(selectedTask, selectedTaskLatestDispatch)"
    :primary-action-disabled="selectedTaskPrimaryActionDisabled"
    :primary-action-label="drawerPrimaryActionLabel(selectedTask, selectedTaskLatestDispatch)"
    :start-tool="selectedTaskDispatchTool"
    :task="selectedTask"
    :task-project="selectedTaskProject"
    :task-runs="selectedTaskRuns"
    @close="emit('cancel-task-drawer')"
    @request-close-task="emit('request-selected-task-close', selectedTask)"
    @request-delete-task="emit('request-selected-task-delete', selectedTask)"
    @request-discard-history="emit('request-selected-task-discard-history', selectedTask)"
    @request-edit-task="emit('request-edit-task', selectedTask)"
    @request-open-project="emit('request-open-task-project')"
    @request-open-url="emit('request-open-url', $event)"
    @request-primary-action="emit('request-selected-task-primary-action')"
    @request-start-fresh="emit('request-selected-task-start-fresh', selectedTask)"
    @update:start-tool="emit('update:task-start-tool', $event)"
  />

  <ReviewDrawer
    v-if="showReviewDrawer && selectedReview"
    :can-cancel="selectedReviewCanCancel"
    :can-re-review="selectedReviewCanReReview"
    :canceling-review-id="cancelingReviewId"
    :following-up-review-id="followingUpReviewId"
    :latest-run="selectedReviewLatestRun"
    :review="selectedReview"
    :review-runs="selectedReviewRuns"
    :saving="saving"
    @close="emit('cancel-review-drawer')"
    @request-cancel-review-run="emit('request-cancel-review-run', $event)"
    @request-delete-review="emit('request-delete-review', $event)"
    @request-open-url="emit('request-open-url', $event)"
    @request-rereview="emit('request-review-follow-up', $event)"
  />

  <TaskEditorModal
    :busy="saving"
    :default-project="defaultCreateProject"
    :mode="creatingTask ? 'create' : 'edit'"
    :open="creatingTask || editingTask !== null"
    :projects="availableProjects"
    :task="editingTask"
    @cancel="emit('cancel-task-editor')"
    @save="emit('request-save-task', $event)"
  />

  <ReviewRequestModal
    :busy="saving"
    :default-preferred-tool="defaultRemoteAgentPreferredTool"
    :main-user="remoteAgentSettings?.reviewFollowUp?.mainUser"
    :open="creatingReview"
    @cancel="emit('cancel-review-editor')"
    @save="emit('request-save-review', $event)"
  />

  <ReviewFollowUpModal
    :busy="followingUpReviewId !== null"
    :open="followingUpReview !== null"
    :review="followingUpReview"
    @cancel="emit('cancel-review-follow-up')"
    @save="emit('request-save-review-follow-up', $event)"
  />

  <ProjectMetadataModal
    :busy="saving"
    :open="editingProject !== null"
    :project="editingProject"
    @cancel="emit('cancel-project-editor')"
    @save="emit('request-save-project', $event)"
  />

  <RemoteAgentSetupModal
    :busy="saving"
    :open="editingRemoteAgentSetup"
    :required-for-dispatch="runnerSetupRequiredForDispatch"
    :settings="remoteAgentSettings"
    @cancel="emit('cancel-runner-setup')"
    @save="emit('request-save-runner-setup', $event)"
  />

  <FollowUpModal
    :busy="followingUpTaskId !== null"
    :dispatch="followingUpDispatch"
    :open="followingUpTask !== null"
    :task="followingUpTask"
    @cancel="emit('cancel-task-follow-up')"
    @save="emit('request-save-task-follow-up', $event)"
  />

  <ConfirmDialog
    :busy="saving"
    confirm-busy-label="Deleting..."
    confirm-label="Delete forever"
    confirm-variant="danger"
    :description="taskPendingDeletion ? `Delete ${taskTitle(taskPendingDeletion)} permanently? This cannot be undone.` : ''"
    eyebrow="Destructive action"
    :open="taskPendingDeletion !== null"
    title="Delete task"
    @cancel="emit('cancel-task-delete')"
    @confirm="emit('confirm-task-delete')"
  />

  <ConfirmDialog
    :busy="saving"
    confirm-busy-label="Deleting..."
    confirm-label="Delete review"
    confirm-variant="danger"
    :description="reviewPendingDeletion ? `Delete the saved review for ${reviewPendingDeletion.repositoryFullName} PR #${reviewPendingDeletion.pullRequestNumber}? This removes local history and remote review artifacts.` : ''"
    eyebrow="Destructive action"
    :open="reviewPendingDeletion !== null"
    title="Delete PR review"
    @cancel="emit('cancel-review-delete')"
    @confirm="emit('confirm-review-delete')"
  />

  <ConfirmDialog
    :busy="cleaningUpRemoteArtifacts"
    confirm-busy-label="Cleaning up..."
    confirm-label="Run cleanup"
    confirm-variant="primary"
    description="Sweep the remote workspace and remove stale worktrees plus orphaned dispatch artifacts using the same rules as task close/delete."
    eyebrow="Maintenance action"
    :open="cleanupPendingConfirmation"
    title="Clean up remote artifacts"
    @cancel="emit('cancel-cleanup')"
    @confirm="emit('confirm-cleanup')"
  />

  <ConfirmDialog
    :busy="resettingRemoteWorkspace"
    confirm-busy-label="Resetting..."
    confirm-label="Reset workspace"
    confirm-variant="danger"
    description="Delete the entire remote workspace managed by track and remove the remote projects registry. Local tasks and local dispatch history will stay intact, but the next dispatch will need to rebuild the remote environment from scratch."
    eyebrow="Destructive remote action"
    :open="resetPendingConfirmation"
    title="Reset remote workspace"
    @cancel="emit('cancel-reset')"
    @confirm="emit('confirm-reset')"
  />
</template>
