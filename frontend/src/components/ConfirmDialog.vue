<script setup lang="ts">
defineProps<{
  busy?: boolean
  description: string
  confirmLabel?: string
  confirmBusyLabel?: string
  confirmVariant?: 'danger' | 'primary'
  eyebrow?: string
  open: boolean
  title: string
}>()

const emit = defineEmits<{
  cancel: []
  confirm: []
}>()
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      data-testid="confirm-dialog"
      class="fixed inset-0 z-50 flex items-center justify-center bg-bg0/80 px-4 py-6"
    >
      <div
        class="w-full max-w-md border bg-bg1 p-6 shadow-panel"
        :class="
          confirmVariant === 'primary'
            ? 'border-aqua/30'
            : 'border-red/30'
        "
      >
        <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
          {{ eyebrow ?? 'Confirm action' }}
        </p>
        <h3 class="mt-2 font-display text-2xl text-fg0">
          {{ title }}
        </h3>
        <p class="mt-3 text-sm leading-6 text-fg2">
          {{ description }}
        </p>

        <div class="mt-6 flex justify-end gap-3">
          <button
            type="button"
            data-testid="confirm-cancel"
            class="border border-fg2/20 bg-bg0 px-4 py-2 text-xs font-semibold tracking-[0.08em] text-fg1 transition hover:border-fg1/45 hover:text-fg0"
            @click="emit('cancel')"
          >
            Cancel
          </button>
          <button
            type="button"
            data-testid="confirm-submit"
            class="px-4 py-2 text-xs font-semibold tracking-[0.08em] transition disabled:opacity-60"
            :class="
              confirmVariant === 'primary'
                ? 'border border-aqua/35 bg-aqua/10 text-aqua hover:bg-aqua/15'
                : 'border border-red/35 bg-red/10 text-red hover:bg-red/15'
            "
            :disabled="busy"
            @click="emit('confirm')"
          >
            {{ busy ? (confirmBusyLabel ?? 'Working...') : (confirmLabel ?? 'Confirm') }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>
