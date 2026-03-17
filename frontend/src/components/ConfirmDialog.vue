<script setup lang="ts">
defineProps<{
  busy?: boolean
  description: string
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
      class="fixed inset-0 z-50 flex items-center justify-center bg-bg0/80 px-4 py-6"
    >
      <div class="w-full max-w-md border border-red/30 bg-bg1 p-6 shadow-panel">
        <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
          Destructive action
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
            class="border border-fg2/20 bg-bg0 px-4 py-2 text-xs font-semibold tracking-[0.08em] text-fg1 transition hover:border-fg1/45 hover:text-fg0"
            @click="emit('cancel')"
          >
            Cancel
          </button>
          <button
            type="button"
            class="border border-red/35 bg-red/10 px-4 py-2 text-xs font-semibold tracking-[0.08em] text-red transition hover:bg-red/15 disabled:opacity-60"
            :disabled="busy"
            @click="emit('confirm')"
          >
            {{ busy ? 'Deleting...' : 'Delete forever' }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>
