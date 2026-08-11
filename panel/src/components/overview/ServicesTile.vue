<script setup lang="ts">
/* Плитка «Сервисы и доступ»: три вещи, про которые вспоминают внезапно —
   действует ли сертификат, что открыто наружу и дойдёт ли уведомление. Свои
   данные карточка грузит сама и только один раз при показе: в `status` их нет, а
   держать их в общем опросе ради выключенной по умолчанию плитки незачем.
   Запросы «щадящие» (tolerant): на платформе, где действия нет, карточка
   показывает прочерк, а не ошибку. */
import { computed, onMounted, ref } from "vue";
import TileCard from "@/components/TileCard.vue";
import UiButton from "@/components/UiButton.vue";
import { services } from "@/api";
import type { CertStatus, PortmapStatus, PushConfig } from "@/api";
import { useStatusStore } from "@/stores/status";
import { fmtDate, fmtInt } from "@/lib/format";

const status = useStatusStore();

const cert = ref<(CertStatus & { last_result?: string }) | null>(null);
const pm = ref<(PortmapStatus & { mappings?: { enabled?: boolean }[] }) | null>(null);
const push = ref<PushConfig | null>(null);
const loaded = ref(false);

onMounted(async () => {
  const [c, p, u] = await Promise.allSettled([
    services.certStatus(),
    services.portmapStatus(),
    services.pushConfig(),
  ]);
  if (c.status === "fulfilled") cert.value = c.value as typeof cert.value;
  if (p.status === "fulfilled") pm.value = p.value as typeof pm.value;
  if (u.status === "fulfilled") push.value = u.value;
  loaded.value = true;
});

/* Бэкенд отдаёт список проброса под двумя именами в разных версиях CGI —
   как и в разделе «Сервисы», принимаем оба. */
const rows = computed(() => pm.value?.mappings ?? pm.value?.entries ?? []);
const openRows = computed(() => rows.value.filter((r) => r.enabled).length);

const certText = computed(() => {
  const d = cert.value?.domain;
  if (!d) return "Сертификата нет — панель только по локальному адресу";
  if (cert.value?.last_result === "error") return `${d} — последняя попытка не удалась`;
  return cert.value?.expiry ? `${d} — до ${fmtDate(cert.value.expiry)}` : d;
});

const certOk = computed(
  () => !!cert.value?.domain && cert.value?.last_result !== "error",
);

const portmapText = computed(() => {
  if (!rows.value.length) return "Наружу ничего не опубликовано";
  return `Открыто наружу: ${openRows.value} из ${rows.value.length}`;
});

const pushText = computed(() => {
  if (push.value?.available === false) return "Уведомления недоступны на этом роутере";
  const n = push.value?.sub_count ?? 0;
  return n ? `Уведомления: подписок ${fmtInt(n)}` : "На уведомления никто не подписан";
});
</script>

<template>
  <TileCard title="Сервисы и доступ">
    <p class="meta col">
      <span :class="certOk ? 'ok' : 'faint'">{{ certText }}</span>
      <!-- Открытый наружу порт — это то, о чём стоит помнить, поэтому он
           подсвечен, даже когда всё в порядке. -->
      <span :class="openRows ? 'warn' : 'faint'">{{ portmapText }}</span>
      <span class="faint">{{ pushText }}</span>
      <span v-if="!loaded" class="faint">читаю состояние…</span>
    </p>
    <p v-if="status.isKeenetic" class="hint">
      На этом роутере защищённый доступ обычно даёт встроенный сервис имени.
    </p>
    <template #actions>
      <UiButton @click="$router.push({ path: '/services', query: { focus: 'cert' } })">
        Сертификат
      </UiButton>
      <UiButton @click="$router.push({ path: '/services', query: { focus: 'portmap' } })">
        Проброс наружу
      </UiButton>
      <UiButton @click="$router.push({ path: '/services', query: { focus: 'push' } })">
        Уведомления
      </UiButton>
    </template>
  </TileCard>
</template>

<style scoped>
.meta {
  font-size: 13px;
  color: var(--dim);
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.ok {
  color: var(--ok);
}
.warn {
  color: var(--warn);
}
.faint {
  color: var(--faint);
}
.hint {
  font-size: 12px;
  color: var(--faint);
}
</style>
