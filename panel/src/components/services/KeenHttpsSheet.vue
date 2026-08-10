<script setup lang="ts">
/* Инструкция «как открыть панель по защищённому адресу» для Keenetic.
   Отдельная шторка, а не строчка в области: шагов пять, они выполняются в
   чужом интерфейсе (веб-панель роутера), и человеку нужно держать их перед
   глазами, переключаясь между окнами.

   Сертификат Let's Encrypt здесь обычно не нужен: роутер уже умеет выдавать и
   продлевать свой через встроенный сервис имени, поэтому дешевле объяснить
   публикацию через него, чем гонять человека через выпуск. */
import DrawerSheet from "@/components/DrawerSheet.vue";
import UiButton from "@/components/UiButton.vue";

defineProps<{
  open: boolean;
  /** Порт панели с роутера. Не подставляем «обычный» — неверный порт в
      инструкции хуже прочерка: запись создастся и не будет работать. */
  panelPort?: number;
}>();

const emit = defineEmits<{ close: [] }>();
</script>

<template>
  <DrawerSheet :open="open" title="Панель по защищённому адресу" @close="emit('close')">
    <div class="form">
      <p class="lead">
        Уведомления в браузер работают только при защищённом соединении. На этом
        роутере отдельный сертификат для этого не нужен: у него есть встроенный
        сервис имени KeenDNS — роутер сам выдаёт и продлевает сертификат, а панель
        открывается по адресу вида
        <b>https://detour.ваш-хост.keenetic.pro/detour-next/</b>.
      </p>

      <ol class="steps">
        <li>
          В веб-интерфейсе роутера включите <b>KeenDNS</b> (Параметры → Доменное
          имя). Появится имя вида <b>ваш-хост.keenetic.pro</b>.
        </li>
        <li>
          Откройте <b>«Доступ к веб-приложениям домашней сети»</b> и нажмите
          <b>«Добавить»</b>.
        </li>
        <li>
          Заполните запись:
          <ul>
            <li>Доменное имя — поддомен, например <b>detour</b></li>
            <li>
              Адрес — адрес <b>этого роутера</b> в домашней сети
            </li>
            <li>Протокол — <b>HTTP</b></li>
            <li>
              Порт — <b>{{ panelPort ?? "—" }}</b>
              <template v-if="panelPort"> (порт панели)</template>
              <template v-else> — роутер пока не сообщил порт панели</template>
            </li>
          </ul>
        </li>
        <li>
          Сохраните. Примерно через минуту панель откроется по адресу
          <b>https://detour.ваш-хост.keenetic.pro/detour-next/</b>.
        </li>
        <li>Зайдите по этому адресу, вернитесь сюда и включите уведомления.</li>
      </ol>

      <p class="note faint">
        Такая запись делает панель доступной из интернета — задайте на ней
        надёжный пароль в разделе «Вход в панель».
      </p>
    </div>

    <template #footer>
      <UiButton @click="emit('close')">Закрыть</UiButton>
    </template>
  </DrawerSheet>
</template>

<style scoped>
.form {
  display: flex;
  flex-direction: column;
  gap: 14px;
  min-width: 0;
}
.lead {
  font-size: 13px;
  color: var(--dim);
  overflow-wrap: anywhere;
}
.steps {
  margin: 0;
  padding-left: 20px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  font-size: 13px;
  color: var(--dim);
  line-height: 1.55;
}
.steps b {
  color: var(--ink);
  overflow-wrap: anywhere;
}
.steps ul {
  margin: 5px 0 0;
  padding-left: 16px;
  display: flex;
  flex-direction: column;
  gap: 3px;
  list-style: disc;
}
.note {
  font-size: 12.5px;
  color: var(--dim);
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  padding: 9px 11px;
  overflow-wrap: anywhere;
}
.note.faint {
  color: var(--faint);
  border-color: transparent;
  padding: 0 2px;
}
@media (max-width: 860px) {
  :deep(.btn) {
    min-height: 44px;
  }
  /* Узкий экран: отступ под номера съедает и без того малую ширину. */
  .steps {
    padding-left: 18px;
  }
}
</style>
