<script setup lang="ts">
import { onMounted } from 'vue'
import { RouterView } from 'vue-router'

import Toaster from '@/components/shared/Toaster.vue'
import { applyStoredTheme } from '@/composables/useTheme'

onMounted(applyStoredTheme)
</script>

<template>
  <RouterView />
  <Toaster />
</template>
