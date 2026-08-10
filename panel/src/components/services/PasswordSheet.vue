<script setup lang="ts">
/* Смена входа в панель. Отдельная шторка, потому что здесь есть последствие,
   о котором нужно предупредить заранее: смена логина завершает все открытые
   сессии, включая текущую. */
import { computed, ref, watch } from "vue";
import DrawerSheet from "@/components/DrawerSheet.vue";
import UiButton from "@/components/UiButton.vue";
import FormField from "@/components/services/FormField.vue";
import { auth } from "@/api";
import { useSessionStore } from "@/stores/session";
import { useToastStore } from "@/stores/toast";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ close: [] }>();

const session = useSessionStore();
const toast = useToastStore();

const oldPassword = ref("");
const newPassword = ref("");
const repeat = ref("");
const newUser = ref("");
const busy = ref(false);
const err = ref("");

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    oldPassword.value = "";
    newPassword.value = "";
    repeat.value = "";
    newUser.value = session.user;
    err.value = "";
  },
);

const userChanges = computed(
  () => newUser.value.trim() !== "" && newUser.value.trim() !== session.user,
);

async function submit() {
  err.value = "";
  if (!oldPassword.value) {
    err.value = "Введите текущий пароль";
    return;
  }
  if (newPassword.value.length < 8) {
    err.value = "Новый пароль должен быть не короче восьми знаков";
    return;
  }
  if (newPassword.value !== repeat.value) {
    err.value = "Пароли не совпадают";
    return;
  }
  busy.value = true;
  try {
    const r = await auth.changePassword({
      old_password: oldPassword.value,
      new_password: newPassword.value,
      ...(userChanges.value ? { new_user: newUser.value.trim() } : {}),
    });
    emit("close");
    if (r.user_changed) {
      toast.ok("Логин изменён — войдите заново");
      await session.logout();
    } else {
      toast.ok("Пароль изменён");
    }
  } catch (e) {
    err.value = e instanceof Error ? e.message : "Не удалось изменить пароль";
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <DrawerSheet :open="open" title="Вход в панель" @close="emit('close')">
    <div class="form">
      <FormField label="Текущий пароль">
        <input
          v-model="oldPassword"
          type="password"
          autocomplete="current-password"
          :disabled="busy"
        />
      </FormField>

      <FormField label="Новый пароль" hint="Не короче восьми знаков">
        <input
          v-model="newPassword"
          type="password"
          autocomplete="new-password"
          :disabled="busy"
        />
      </FormField>

      <FormField label="Новый пароль ещё раз">
        <input
          v-model="repeat"
          type="password"
          autocomplete="new-password"
          :disabled="busy"
        />
      </FormField>

      <FormField label="Логин" hint="Оставьте как есть, если менять не нужно">
        <input v-model="newUser" type="text" autocomplete="username" :disabled="busy" />
      </FormField>

      <p v-if="userChanges" class="note warn">
        Смена логина завершит все открытые сессии — на этом устройстве и на других.
        Сразу после сохранения панель попросит войти заново.
      </p>

      <p v-if="err" class="note bad">{{ err }}</p>
    </div>

    <template #footer>
      <UiButton variant="primary" :busy="busy" @click="submit">Сохранить</UiButton>
      <UiButton :disabled="busy" @click="emit('close')">Отмена</UiButton>
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
@media (max-width: 860px) {
  :deep(.btn) {
    min-height: 44px;
  }
}
</style>
