<script setup lang="ts">
/* Бесследное удаление панели. Два подтверждения: сначала список того, что
   исчезнет, потом слово, набранное руками. Сервер без {"confirm":"uninstall"}
   тоже ничего не делает — кнопку не обойти запросом из консоли по ошибке. */
import { computed, ref, watch } from "vue";
import DrawerSheet from "@/components/DrawerSheet.vue";
import UiButton from "@/components/UiButton.vue";
import FormField from "@/components/services/FormField.vue";
import LogPane from "@/components/journal/LogPane.vue";
import { usePowerStore } from "@/stores/power";
import { useStatusStore } from "@/stores/status";

const WORD = "удалить";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ close: [] }>();

const power = usePowerStore();
const status = useStatusStore();

const step = ref<1 | 2>(1);
const typed = ref("");
const keepEngines = ref(false);

const running = computed(() => power.busy === "uninstall");
const matches = computed(() => typed.value.trim().toLowerCase() === WORD);

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    step.value = 1;
    typed.value = "";
    keepEngines.value = false;
  },
);

function close() {
  if (running.value) return;
  emit("close");
}

function submit() {
  if (!matches.value || running.value) return;
  void power.uninstall(keepEngines.value);
}
</script>

<template>
  <DrawerSheet :open="open" title="Удаление панели" @close="close">
    <div class="form">
      <template v-if="!running && !power.log">
        <template v-if="step === 1">
          <p class="note bad">
            Detour будет удалён с {{ status.isKeenetic ? "Keenetic" : "роутера" }} полностью.
            Вернуть его можно только новой установкой — с нуля, без прежних настроек.
          </p>
          <p class="lead">Исчезнет:</p>
          <ul class="list">
            <li>все профили VPN, подписки, цепочки и списки доменов;</li>
            <li>правила маршрутизации, устройств, обхода DPI, проброс сервисов;</li>
            <li>сертификат панели, ключи уведомлений, логин и пароль;</li>
            <li>расписание, журналы и строка нашего фида в менеджере пакетов.</li>
          </ul>
          <p class="lead">
            Трафик пойдёт напрямую, как до установки. Резервную копию настроек
            имеет смысл скачать заранее — выше, в «Резервной копии».
          </p>
          <label class="check">
            <input v-model="keepEngines" type="checkbox" />
            <span>Оставить движки (sing-box, tpws, nfqws2, mihomo)</span>
          </label>
          <p v-if="!status.isKeenetic" class="hint">
            На GL.iNet может остаться разметка LAN-трафика (<code>lan_mark_fallback</code>):
            без неё прошивка режет весь интернет в локальной сети. Скрипт снимет её
            сам, если такого правила в прошивке нет.
          </p>
        </template>

        <template v-else>
          <p class="note bad">Это последнее подтверждение. Отменить удаление будет нельзя.</p>
          <FormField :label="`Наберите «${WORD}»`">
            <input
              v-model="typed"
              type="text"
              autocomplete="off"
              autocapitalize="off"
              spellcheck="false"
              @keydown.enter.prevent="submit"
            />
          </FormField>
        </template>
      </template>

      <template v-else>
        <p v-if="running" class="note warn">
          Удаляю… Панель пропадёт по ходу — это ожидаемо.
        </p>
        <LogPane :text="power.log" follow height="320px" empty-text="Жду вывод удаления…" />
      </template>
    </div>

    <template #footer>
      <template v-if="!running && !power.log">
        <template v-if="step === 1">
          <UiButton variant="danger" @click="step = 2">Продолжить</UiButton>
          <UiButton @click="close">Отмена</UiButton>
        </template>
        <template v-else>
          <UiButton variant="danger" :disabled="!matches" @click="submit">
            Удалить навсегда
          </UiButton>
          <UiButton @click="step = 1">Назад</UiButton>
        </template>
      </template>
    </template>
  </DrawerSheet>
</template>

<style scoped>
.form {
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-width: 0;
}
.lead {
  font-size: 13px;
  color: var(--dim);
}
.list {
  margin: 0;
  padding-left: 18px;
  display: grid;
  gap: 4px;
  font-size: 12.5px;
  color: var(--dim);
}
.hint {
  font-size: 12px;
  color: var(--faint);
  overflow-wrap: anywhere;
}
.note {
  font-size: 12.5px;
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  padding: 9px 11px;
  color: var(--dim);
  overflow-wrap: anywhere;
}
.note.warn {
  color: var(--warn);
  border-color: color-mix(in srgb, var(--warn) 45%, transparent);
}
.note.bad {
  color: var(--bad);
  border-color: color-mix(in srgb, var(--bad) 45%, transparent);
}
.check {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  cursor: pointer;
  min-height: 32px;
}
.check input {
  width: 17px;
  height: 17px;
  flex: none;
  accent-color: var(--accent);
}
@media (max-width: 860px) {
  :deep(.btn) {
    min-height: 44px;
  }
}
</style>
