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
      class="fixed inset-0 z-50 flex items-center justify-center bg-ink/35 px-4 backdrop-blur-sm"
    >
      <div class="w-full max-w-md rounded-[28px] border border-white/70 bg-white/95 p-6 shadow-panel">
        <h3 class="font-display text-2xl text-ink">
          {{ title }}
        </h3>
        <p class="mt-3 text-sm leading-6 text-ink/70">
          {{ description }}
        </p>

        <div class="mt-6 flex justify-end gap-3">
          <button
            type="button"
            class="rounded-full border border-ink/15 px-4 py-2 text-sm font-medium text-ink transition hover:border-ink/30"
            @click="emit('cancel')"
          >
            Cancel
          </button>
          <button
            type="button"
            class="rounded-full bg-berry px-4 py-2 text-sm font-semibold text-white transition hover:bg-berry/90 disabled:cursor-not-allowed disabled:opacity-60"
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
