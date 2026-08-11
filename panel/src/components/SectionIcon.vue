<script setup lang="ts">
/* Иконки разделов одним компонентом: линейные, 24×24, stroke=currentColor —
   так они одинаково ведут себя в рейке, нижнем баре и палитре команд. */
defineProps<{ name: string; size?: number }>();
</script>

<template>
  <svg
    :width="size ?? 19"
    :height="size ?? 19"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="1.7"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <template v-if="name === 'gauge'">
      <path d="M12 20a8 8 0 1 1 8-8" />
      <path d="M12 12l5-3" />
    </template>
    <template v-else-if="name === 'list'">
      <path d="M4 6h16M4 12h16M4 18h10" />
    </template>
    <template v-else-if="name === 'route'">
      <circle cx="6" cy="6" r="2.4" />
      <circle cx="18" cy="18" r="2.4" />
      <path d="M8.4 6H14a4 4 0 0 1 0 8h-4a4 4 0 0 0 0 4h5.6" />
    </template>
    <template v-else-if="name === 'server'">
      <rect x="3" y="4" width="18" height="7" rx="2" />
      <rect x="3" y="13" width="18" height="7" rx="2" />
      <path d="M7 7.5h.01M7 16.5h.01" />
    </template>
    <template v-else-if="name === 'terminal'">
      <path d="M5 8l4 4-4 4M12 16h7" />
    </template>
    <template v-else-if="name === 'search'">
      <circle cx="11" cy="11" r="7" />
      <path d="M20 20l-3.5-3.5" />
    </template>
    <template v-else-if="name === 'sun'">
      <circle cx="12" cy="12" r="4" />
      <path d="M12 3v2M12 19v2M3 12h2M19 12h2M5.6 5.6l1.4 1.4M17 17l1.4 1.4M18.4 5.6L17 7M7 17l-1.4 1.4" />
    </template>
    <template v-else-if="name === 'moon'">
      <path d="M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z" />
    </template>
    <template v-else-if="name === 'logout'">
      <path d="M15 5H6a1 1 0 0 0-1 1v12a1 1 0 0 0 1 1h9" />
      <path d="M18 15l3-3-3-3M21 12h-9" />
    </template>
    <template v-else-if="name === 'refresh'">
      <path d="M20 11a8 8 0 1 0-1.6 5.6" />
      <path d="M20 5v6h-6" />
    </template>
    <template v-else-if="name === 'close'">
      <path d="M6 6l12 12M18 6L6 18" />
    </template>
    <template v-else-if="name === 'grip'">
      <path d="M9 6h.01M15 6h.01M9 12h.01M15 12h.01M9 18h.01M15 18h.01" />
    </template>
  </svg>
</template>
