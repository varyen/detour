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
    <!-- Заливка задаётся на самой фигуре: у svg выше fill="none", а «играть» и
         «стоп» узнаются именно сплошными. -->
    <template v-else-if="name === 'play'">
      <path d="M8.5 5.7l10 6.3-10 6.3z" fill="currentColor" stroke-linejoin="round" />
    </template>
    <template v-else-if="name === 'stop'">
      <rect x="7" y="7" width="10" height="10" rx="2" fill="currentColor" />
    </template>
    <template v-else-if="name === 'grip'">
      <path d="M9 6h.01M15 6h.01M9 12h.01M15 12h.01M9 18h.01M15 18h.01" />
    </template>
    <!-- Глобус — кнопка «определить страны эндпоинтов». -->
    <template v-else-if="name === 'globe'">
      <circle cx="12" cy="12" r="9" />
      <path d="M3 12h18" />
      <path d="M12 3c2.5 2.6 3.8 5.7 3.8 9s-1.3 6.4-3.8 9c-2.5-2.6-3.8-5.7-3.8-9s1.3-6.4 3.8-9z" />
    </template>
    <!-- Иконки областей (Правила, Журнал, Сервисы, плитки «Обзора»). -->
    <template v-else-if="name === 'tunnel'">
      <path d="M3 19V11a9 9 0 0 1 18 0v8" />
      <path d="M7 19v-7a5 5 0 0 1 10 0v7" />
      <path d="M3 19h18" />
    </template>
    <template v-else-if="name === 'shield'">
      <path d="M12 3l7 3v5c0 4.6-3 8.3-7 10-4-1.7-7-5.4-7-10V6z" />
      <path d="M9 12l2 2 4-4" />
    </template>
    <template v-else-if="name === 'direct'">
      <path d="M4 12h15" />
      <path d="M14 6l6 6-6 6" />
    </template>
    <template v-else-if="name === 'dpi'">
      <path d="M13 3L5 13h6l-1 8 8-10h-6z" />
    </template>
    <template v-else-if="name === 'devices'">
      <rect x="2.5" y="5" width="13" height="10" rx="1.5" />
      <path d="M6 19h6M9 15v4" />
      <rect x="17.5" y="9" width="4" height="10" rx="1" />
    </template>
    <template v-else-if="name === 'intercept'">
      <path d="M4 5h16l-6 7v6l-4 2v-8z" />
    </template>
    <template v-else-if="name === 'waves'">
      <path d="M3 9c2-2 4-2 6 0s4 2 6 0 4-2 6 0" />
      <path d="M3 15c2-2 4-2 6 0s4 2 6 0 4-2 6 0" />
    </template>
    <template v-else-if="name === 'block'">
      <circle cx="12" cy="12" r="8.5" />
      <path d="M6 6l12 12" />
    </template>
    <template v-else-if="name === 'flag'">
      <path d="M5 21V4" />
      <path d="M5 4h12l-2 4 2 4H5" />
    </template>
    <template v-else-if="name === 'dns'">
      <circle cx="12" cy="12" r="8.5" />
      <path d="M8 12h8M12 8v8" />
      <path d="M3.5 12h1M19.5 12h1" />
    </template>
    <template v-else-if="name === 'torrent'">
      <path d="M12 3v11" />
      <path d="M7 9l5 5 5-5" />
      <path d="M5 20h14" />
    </template>
    <template v-else-if="name === 'logs'">
      <path d="M7 3h8l4 4v14H7z" />
      <path d="M15 3v4h4M10 11h6M10 15h6M10 19h3" />
    </template>
    <template v-else-if="name === 'code'">
      <path d="M8 7l-5 5 5 5M16 7l5 5-5 5M14 4l-4 16" />
    </template>
    <template v-else-if="name === 'sliders'">
      <path d="M4 7h10M18 7h2M4 17h4M12 17h8" />
      <circle cx="16" cy="7" r="2" />
      <circle cx="10" cy="17" r="2" />
    </template>
    <template v-else-if="name === 'firewall'">
      <rect x="3" y="5" width="18" height="14" rx="1.5" />
      <path d="M3 10h18M3 15h18M9 5v5M15 10v5M9 15v4" />
    </template>
    <template v-else-if="name === 'download'">
      <path d="M12 4v11" />
      <path d="M7 10l5 5 5-5" />
      <path d="M4 20h16" />
    </template>
    <template v-else-if="name === 'pulse'">
      <path d="M3 12h4l2-6 4 12 2-6h6" />
    </template>
    <template v-else-if="name === 'plug'">
      <path d="M9 3v5M15 3v5" />
      <path d="M6 8h12v3a6 6 0 0 1-12 0z" />
      <path d="M12 17v4" />
    </template>
    <template v-else-if="name === 'lock'">
      <rect x="5" y="10.5" width="14" height="10" rx="2" />
      <path d="M8 10.5V8a4 4 0 0 1 8 0v2.5" />
    </template>
    <template v-else-if="name === 'bell'">
      <path d="M6 16V11a6 6 0 0 1 12 0v5l2 2H4z" />
      <path d="M10 20a2 2 0 0 0 4 0" />
    </template>
    <template v-else-if="name === 'cpu'">
      <rect x="6" y="6" width="12" height="12" rx="2" />
      <rect x="9.5" y="9.5" width="5" height="5" />
      <path d="M9 3v3M15 3v3M9 18v3M15 18v3M3 9h3M3 15h3M18 9h3M18 15h3" />
    </template>
    <template v-else-if="name === 'bolt'">
      <path d="M13 2L4.5 14H11l-1 8L19.5 10H13z" />
    </template>
    <template v-else-if="name === 'memory'">
      <rect x="3" y="7" width="18" height="10" rx="1.5" />
      <path d="M7 11v2M11 11v2M15 11v2M6 17v2M10 17v2M14 17v2M18 17v2" />
    </template>
    <template v-else-if="name === 'archive'">
      <rect x="3" y="4" width="18" height="5" rx="1" />
      <path d="M5 9v10h14V9M10 13h4" />
    </template>
    <template v-else-if="name === 'key'">
      <circle cx="8" cy="15" r="4" />
      <path d="M11 12l9-9M16 7l3 3M14 9l2 2" />
    </template>
    <template v-else-if="name === 'power'">
      <path d="M12 3v8" />
      <path d="M6.3 7a8 8 0 1 0 11.4 0" />
    </template>
    <template v-else-if="name === 'chart'">
      <path d="M4 20V4M4 20h16" />
      <path d="M7 15l4-4 3 3 5-6" />
    </template>
    <template v-else-if="name === 'link'">
      <path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1" />
      <path d="M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1" />
    </template>
    <template v-else-if="name === 'target'">
      <circle cx="12" cy="12" r="8.5" />
      <circle cx="12" cy="12" r="4.5" />
      <circle cx="12" cy="12" r="1" />
    </template>
    <template v-else-if="name === 'heart'">
      <path d="M12 20s-7-4.4-7-10a4 4 0 0 1 7-2.6A4 4 0 0 1 19 10c0 5.6-7 10-7 10z" />
    </template>
    <template v-else-if="name === 'tag'">
      <path d="M3 12V4h8l10 10-8 8z" />
      <circle cx="7.5" cy="8.5" r="1.3" />
    </template>
    <template v-else-if="name === 'uplinks'">
      <path d="M5 20V9M12 20V4M19 20v-7" />
      <path d="M3 20h18" />
    </template>
    <template v-else-if="name === 'chain'">
      <rect x="2.5" y="9" width="8" height="6" rx="3" />
      <rect x="13.5" y="9" width="8" height="6" rx="3" />
      <path d="M9 12h6" />
    </template>
    <template v-else-if="name === 'rss'">
      <path d="M5 5a14 14 0 0 1 14 14M5 11a8 8 0 0 1 8 8" />
      <circle cx="6" cy="18" r="1.4" />
    </template>
    <template v-else-if="name === 'cloud'">
      <path d="M7 18a4.5 4.5 0 0 1-.6-9A6 6 0 0 1 18 9.5a4.3 4.3 0 0 1-.5 8.5z" />
    </template>
    <template v-else-if="name === 'qr'">
      <rect x="4" y="4" width="6" height="6" rx="1" />
      <rect x="14" y="4" width="6" height="6" rx="1" />
      <rect x="4" y="14" width="6" height="6" rx="1" />
      <path d="M14 14h2v2h-2zM18 14h2M14 18v2h2M18 18h2v2" />
    </template>
  </svg>
</template>
