<script setup lang="ts">
import { ref, watch } from 'vue'

import type {
  RemoteAgentPreferredTool,
  RemoteAgentSettings,
  RemoteAgentSettingsUpdateInput,
} from '../types/task'

const props = defineProps<{
  busy?: boolean
  open: boolean
  requiredForDispatch?: boolean
  settings: RemoteAgentSettings | null
}>()

const emit = defineEmits<{
  cancel: []
  save: [payload: RemoteAgentSettingsUpdateInput]
}>()

const preferredTool = ref<RemoteAgentPreferredTool>('codex')
const shellPrelude = ref('')
const reviewFollowUpEnabled = ref(false)
const mainUser = ref('')
const defaultReviewPrompt = ref('')

watch(
  () => props.settings,
  (settings) => {
    preferredTool.value = settings?.preferredTool ?? 'codex'
    shellPrelude.value = settings?.shellPrelude ?? ''
    reviewFollowUpEnabled.value = settings?.reviewFollowUp?.enabled ?? false
    mainUser.value = settings?.reviewFollowUp?.mainUser ?? ''
    defaultReviewPrompt.value = settings?.reviewFollowUp?.defaultReviewPrompt ?? ''
  },
  { immediate: true },
)

watch(
  () => props.open,
  (open, _previousOpen, onCleanup) => {
    if (!open) {
      return
    }

    const handleKeydown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        emit('cancel')
      }
    }

    window.addEventListener('keydown', handleKeydown)
    onCleanup(() => {
      window.removeEventListener('keydown', handleKeydown)
    })
  },
  { immediate: true },
)

function submit() {
  emit('save', {
    preferredTool: preferredTool.value,
    shellPrelude: shellPrelude.value.trim(),
    reviewFollowUp: {
      enabled: reviewFollowUpEnabled.value,
      mainUser: mainUser.value.trim() || undefined,
      defaultReviewPrompt: defaultReviewPrompt.value.trim() || undefined,
    },
  })
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      data-testid="remote-agent-setup-modal"
      class="fixed inset-0 z-50 overflow-y-auto bg-bg0/80 px-4 py-6"
      @click.self="emit('cancel')"
    >
      <div class="flex min-h-full items-start justify-center">
        <div
          role="dialog"
          aria-modal="true"
          class="my-auto w-full min-w-0 max-w-4xl border border-fg2/20 bg-bg1 p-6 shadow-panel"
        >
          <div class="flex items-start justify-between gap-4 border-b border-fg2/10 pb-4">
            <div>
              <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
                Remote runner
              </p>
              <h3 class="mt-2 font-display text-2xl text-fg0 sm:text-3xl">
                Runner setup
              </h3>
              <p
                v-if="requiredForDispatch"
                class="mt-3 border border-yellow/25 bg-yellow/8 px-3 py-2 text-sm leading-6 text-yellow"
              >
                Dispatch needs these shell instructions before the remote agent can start.
              </p>
            </div>
            <button
              type="button"
              class="border border-fg2/20 bg-bg0 px-3 py-2 text-xs font-semibold tracking-[0.08em] text-fg2 transition hover:border-fg1/45 hover:text-fg0"
              @click="emit('cancel')"
            >
              Close
            </button>
          </div>

          <div class="mt-5 space-y-4">
            <p class="text-sm leading-7 text-fg2">
              Make sure to paste everything you need to configure PATH and other variables for the
              runner, as it runs in a non-interactive shell. You can check your
              <code>.bashrc</code> / <code>.zshrc</code> for reference if unsure.
            </p>

            <p class="text-sm leading-7 text-fg3">
              These commands run before every remote command. Keep them quiet on stdout so automated
              SSH calls can still parse command output reliably.
            </p>

            <section class="border border-fg2/15 bg-bg0/60 p-4">
              <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
                Preferred tool
              </p>
            <p class="mt-2 text-sm leading-7 text-fg2">
              Codex stays the default. Choose Claude when you want remote runs to launch through
              the Claude CLI instead.
            </p>

              <label class="mt-4 block text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
                Runner
                <select
                  v-model="preferredTool"
                  data-testid="preferred-tool"
                  class="mt-2 w-full border border-fg2/20 bg-bg0 px-4 py-3 text-sm leading-6 text-fg0 outline-none transition hover:border-fg2/40 focus:border-aqua/50 focus:ring-1 focus:ring-aqua/50"
                >
                  <option value="codex">
                    Codex
                  </option>
                  <option value="claude">
                    Claude
                  </option>
                  <option value="opencode">
                    opencode
                  </option>
                </select>
              </label>
            </section>

            <p
              v-if="!settings?.configured"
              class="border border-red/25 bg-red/8 px-4 py-3 text-sm leading-6 text-red"
            >
              Remote dispatch itself is not configured yet. Run
              <code>track remote-agent configure --host &lt;host&gt; --user &lt;user&gt; --identity-file ~/.ssh/track_remote_agent</code>
              locally first.
            </p>

            <label class="block text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
              Shell prelude
              <textarea
                v-model="shellPrelude"
                rows="12"
                class="mt-2 w-full border border-fg2/20 bg-bg0 px-4 py-3 font-mono text-sm leading-6 text-fg0 outline-none transition hover:border-fg2/40 focus:border-aqua/50 focus:ring-1 focus:ring-aqua/50"
                placeholder="export NVM_DIR=&quot;$HOME/.nvm&quot;&#10;[ -s &quot;$NVM_DIR/nvm.sh&quot; ] &amp;&amp; . &quot;$NVM_DIR/nvm.sh&quot;&#10;. &quot;$HOME/.cargo/env&quot;"
              />
            </label>

            <section class="border border-fg2/15 bg-bg0/60 p-4">
              <div class="flex items-start justify-between gap-4">
                <div>
                  <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
                    Review follow-up
                  </p>
                  <p class="mt-2 text-sm leading-7 text-fg2">
                    These settings are shared between manual PR reviews from the web UI and automatic
                    follow-up after PR updates. Manual reviews need the main GitHub user, while
                    automatic follow-up also requires the toggle below.
                  </p>
                </div>

                <label class="flex items-center gap-2 text-sm font-semibold text-fg1">
                  <input
                    v-model="reviewFollowUpEnabled"
                    type="checkbox"
                    class="h-4 w-4 border border-fg2/30 bg-bg0 accent-aqua"
                  >
                  Enabled
                </label>
              </div>

              <label class="mt-4 block text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
                Main GitHub user
                <input
                  v-model="mainUser"
                  type="text"
                  class="mt-2 w-full border border-fg2/20 bg-bg0 px-4 py-3 font-mono text-sm leading-6 text-fg0 outline-none transition hover:border-fg2/40 focus:border-aqua/50 focus:ring-1 focus:ring-aqua/50"
                  placeholder="octocat"
                >
              </label>

              <p class="mt-3 text-sm leading-7 text-fg3">
                Only this user can trigger automatic follow-ups. Approved reviews are ignored.
              </p>

              <label class="mt-4 block text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
                Default review prompt
                <textarea
                  v-model="defaultReviewPrompt"
                  data-testid="default-review-prompt"
                  rows="6"
                  class="mt-2 w-full border border-fg2/20 bg-bg0 px-4 py-3 text-sm leading-6 text-fg0 outline-none transition hover:border-fg2/40 focus:border-aqua/50 focus:ring-1 focus:ring-aqua/50"
                  placeholder="Focus on bugs, regressions, risky behavior changes, missing tests, and edge cases."
                />
              </label>

              <p class="mt-3 text-sm leading-7 text-fg3">
                This reusable prompt is appended to every manual PR review request before any extra
                instructions you type for that specific review.
              </p>
            </section>
          </div>

          <div class="mt-6 flex justify-end gap-3">
            <button
              type="button"
              class="border border-fg2/20 bg-bg0 px-4 py-2 text-xs font-semibold tracking-[0.08em] text-fg1 transition hover:border-fg1/45 hover:text-fg0"
              @click="emit('cancel')"
            >
              Cancel
            </button>
            <button
              type="button"
              data-testid="save-runner-setup"
              class="border border-aqua/35 bg-aqua/10 px-5 py-2 text-xs font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15 disabled:opacity-60"
              :disabled="
                busy ||
                !settings?.configured ||
                shellPrelude.trim().length === 0 ||
                (reviewFollowUpEnabled && mainUser.trim().length === 0)
              "
              @click="submit"
            >
              {{ busy ? 'Saving...' : 'Save runner setup' }}
            </button>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>
