<script setup lang="ts">
/* Плитка «Маршрутизация»: что именно решает, куда пойдёт соединение. Всё
   берётся из уже загруженного `status` — лишних запросов карточка не делает,
   поэтому её можно держать на главной постоянно. Правится это в «Правилах»,
   отсюда — ссылки в нужный блок. */
import { computed } from "vue";
import TileCard from "@/components/TileCard.vue";
import UiButton from "@/components/UiButton.vue";
import { useStatusStore } from "@/stores/status";

const status = useStatusStore();

const sb = computed(() => status.data?.singbox);

/* В режиме all-except список сайтов не участвует вовсе, и «сайтов в списке: 0»
   там читалось бы как «ничего не проксируется» — ровно наоборот. Та же
   осторожность, что и в плитке «Область действия». */
const allExcept = computed(() => sb.value?.routing_mode === "all-except");

const modeText = computed(() =>
  allExcept.value ? "в туннеле всё, кроме белого списка" : "в туннеле только список сайтов",
);

/* Несколько одновременных выходов (`multi`) — это не косметика: маршруты по
   сайтам работают только в нём, и если режим одиночный, «цели маршрутов»
   объясняют, почему правило не сработало. */
const multi = computed(() => sb.value?.singbox_mode === "multi");
const targets = computed(() => sb.value?.route_targets ?? 0);
</script>

<template>
  <TileCard title="Маршрутизация">
    <p class="meta col">
      <span>{{ modeText }}</span>
      <span v-if="allExcept" class="faint">
        Список сайтов сейчас не используется — что идёт мимо туннеля, решает
        белый список
      </span>
      <span v-else>
        Сайтов в списке VPN: {{ sb?.domains ?? 0 }}<template v-if="sb?.ips">
          · адресов {{ sb.ips }}</template>
      </span>
      <span v-if="multi">
        Маршруты по сайтам: {{ targets }}
        {{ targets === 1 ? "цель" : "целей" }}
      </span>
      <span v-else class="faint">
        Маршруты по сайтам выключены — один выход на все соединения
      </span>
      <span v-if="sb?.ipset_count">Адресов в ipset сейчас: {{ sb.ipset_count }}</span>
    </p>
    <template #actions>
      <UiButton @click="$router.push({ path: '/rules', query: { focus: 'routes' } })">
        Маршруты по сайтам
      </UiButton>
      <UiButton
        v-if="allExcept"
        @click="$router.push({ path: '/rules', query: { focus: 'whitelist' } })"
      >
        Белый список
      </UiButton>
      <UiButton v-else @click="$router.push({ path: '/rules', query: { focus: 'domains' } })">
        Сайты через VPN
      </UiButton>
      <UiButton @click="$router.push({ path: '/rules', query: { focus: 'mode' } })">
        Режим туннеля
      </UiButton>
    </template>
  </TileCard>
</template>

<style scoped>
.meta {
  font-size: 13px;
  color: var(--dim);
  display: flex;
  gap: 4px 14px;
  flex-wrap: wrap;
}
.meta.col {
  flex-direction: column;
  gap: 4px;
}
.faint {
  color: var(--faint);
}
</style>
