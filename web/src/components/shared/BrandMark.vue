<script setup lang="ts">
/**
 * The Sailplane mark: a nine-dot grid whose lit dots read as a glider. The
 * wing row and tail use the text color, the nose is a hollow ring in the brand
 * color, and the four corners are the inactive grid, drawn faintly.
 */
withDefaults(defineProps<{ label?: string }>(), { label: '' })
</script>

<template>
  <svg
    viewBox="0 0 24 24"
    :role="label ? 'img' : undefined"
    :aria-label="label || undefined"
    :aria-hidden="label ? undefined : 'true'"
  >
    <g fill="none" stroke="currentColor" stroke-opacity=".28" stroke-width=".75">
      <circle cx="3" cy="3" r="2.625" />
      <circle cx="21" cy="3" r="2.625" />
      <circle cx="3" cy="21" r="2.625" />
      <circle cx="21" cy="21" r="2.625" />
    </g>
    <g fill="currentColor">
      <circle cx="3" cy="12" r="3" />
      <circle cx="12" cy="12" r="3" />
      <circle cx="21" cy="12" r="3" />
      <circle cx="12" cy="21" r="3" />
    </g>
    <circle cx="12" cy="3" r="2.325" fill="none" stroke="var(--brand)" stroke-width="1.35" />
  </svg>
</template>
