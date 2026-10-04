<script setup lang="ts">
/**
 * The Sailplane mark. The body uses the text color; the nose ring is hollow in
 * the brand color.
 */
withDefaults(defineProps<{ label?: string }>(), { label: '' })
</script>

<template>
  <svg
    viewBox="0 0 30 30"
    :role="label ? 'img' : undefined"
    :aria-label="label || undefined"
    :aria-hidden="label ? undefined : 'true'"
  >
    <g fill="currentColor">
      <rect x="13.85" y="11.6" width="2.3" height="12.8" rx="1.15" />
      <g transform="translate(15 12.9)">
        <g transform="rotate(-8)">
          <rect x="-13.6" y="-1.1" width="6.6" height="2.2" rx="1.1" />
          <rect x="-6.4" y="-1.1" width="5.8" height="2.2" rx="1.1" />
        </g>
        <g transform="rotate(8)">
          <rect x="0.6" y="-1.1" width="5.8" height="2.2" rx="1.1" />
          <rect x="7.0" y="-1.1" width="6.6" height="2.2" rx="1.1" />
        </g>
      </g>
      <g transform="translate(15 22.1)">
        <rect x="-4.6" width="3.9" height="2.1" rx="1.05" />
        <rect x="0.7" width="3.9" height="2.1" rx="1.05" />
      </g>
    </g>
    <circle cx="15" cy="9" r="2.4" fill="none" stroke="var(--brand)" stroke-width="2.1" />
  </svg>
</template>
