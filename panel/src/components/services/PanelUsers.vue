<script setup lang="ts">
/* Другие логины панели. Роль «только VPN-сервер» видит в панели одну область —
   своих клиентов сервера; роль «полный доступ» — всё, как главный админ.
   Ограничение держит CGI, интерфейс только не показывает лишнего. */
import { onMounted, ref } from "vue";
import FormField from "@/components/services/FormField.vue";
import UiButton from "@/components/UiButton.vue";
import SegmentedControl from "@/components/SegmentedControl.vue";
import { auth } from "@/api";
import type { PanelRole, PanelUser } from "@/api";
import { useSessionStore } from "@/stores/session";
import { useToastStore } from "@/stores/toast";

const session = useSessionStore();
const toast = useToastStore();

const users = ref<PanelUser[]>([]);
const loadError = ref("");
const busy = ref("");
const confirmDel = ref("");
const pwFor = ref("");
const pwValue = ref("");

const newUser = ref("");
const newPass = ref("");
const newRole = ref<PanelRole>("vpn");

const ROLE_OPTIONS: { value: PanelRole; label: string; hint: string }[] = [
  { value: "vpn", label: "Только VPN-сервер", hint: "Свои клиенты сервера, больше ничего" },
  { value: "admin", label: "Полный доступ", hint: "Вся панель, как у вас" },
];

async function load() {
  try {
    users.value = (await auth.users()).users;
    loadError.value = "";
  } catch (e) {
    loadError.value = e instanceof Error ? e.message : "Не удалось получить список";
  }
}
onMounted(load);

function roleLabel(u: PanelUser): string {
  if (u.role === "admin") return "полный доступ";
  const n = u.clients;
  return `только VPN-сервер · клиентов: ${n}`;
}

async function add() {
  const u = newUser.value.trim();
  if (!u || newPass.value.length < 8) {
    toast.error("Нужен логин и пароль не короче 8 символов");
    return;
  }
  busy.value = "add";
  try {
    await auth.userAdd(u, newPass.value, newRole.value);
    toast.ok(`Логин ${u} создан`);
    newUser.value = "";
    newPass.value = "";
  } catch (e) {
    toast.fromError(e, "Не удалось создать логин");
  } finally {
    busy.value = "";
    await load();
  }
}

async function setPassword(u: PanelUser) {
  if (pwValue.value.length < 8) {
    toast.error("Пароль не короче 8 символов");
    return;
  }
  busy.value = `pw:${u.user}`;
  try {
    await auth.userSet(u.user, { password: pwValue.value });
    toast.ok(`Пароль ${u.user} сменён — его открытые сессии закрыты`);
    pwFor.value = "";
    pwValue.value = "";
  } catch (e) {
    toast.fromError(e, "Не удалось сменить пароль");
  } finally {
    busy.value = "";
  }
}

async function setRole(u: PanelUser, role: PanelRole) {
  if (role === u.role) return;
  busy.value = `role:${u.user}`;
  try {
    await auth.userSet(u.user, { role });
    toast.ok(role === "admin" ? `${u.user}: полный доступ` : `${u.user}: только VPN-сервер`);
  } catch (e) {
    toast.fromError(e, "Не удалось сменить доступ");
  } finally {
    busy.value = "";
    await load();
  }
}

async function remove(u: PanelUser) {
  busy.value = `del:${u.user}`;
  try {
    await auth.userDel(u.user);
    toast.ok(u.clients ? `Логин ${u.user} удалён, его клиенты теперь ваши` : `Логин ${u.user} удалён`);
  } catch (e) {
    toast.fromError(e, "Не удалось удалить логин");
  } finally {
    confirmDel.value = "";
    busy.value = "";
    await load();
  }
}
</script>

<template>
  <div class="users">
    <h3 class="sub">Админы</h3>
    <p class="note faint">
      У админа «только VPN-сервер» в панели одна область — его клиенты сервера:
      заводит, отключает, отдаёт ссылки. Чужих клиентов, профилей и настроек он не
      видит. Вы видите всех клиентов и можете передать любого другому админу.
    </p>
    <p v-if="loadError" class="note bad">{{ loadError }}</p>

    <div v-for="u in users" :key="u.user" class="user">
      <div class="u-head">
        <b>{{ u.user }}</b>
        <span class="dim">{{ u.user === session.user ? "это вы" : roleLabel(u) }}</span>
      </div>
      <template v-if="u.user !== session.user">
        <SegmentedControl
          :model-value="u.role"
          label="Доступ"
          :options="ROLE_OPTIONS"
          :busy="busy === `role:${u.user}`"
          @update:model-value="setRole(u, $event as PanelRole)"
        />
        <div v-if="pwFor === u.user" class="pw">
          <input v-model="pwValue" type="password" autocomplete="new-password" placeholder="Новый пароль" />
          <UiButton variant="primary" :busy="busy === `pw:${u.user}`" @click="setPassword(u)">Сохранить</UiButton>
          <UiButton @click="pwFor = ''">Отмена</UiButton>
        </div>
        <div class="actions">
          <UiButton v-if="pwFor !== u.user" @click="pwFor = u.user; pwValue = ''">Сменить пароль</UiButton>
          <template v-if="confirmDel === u.user">
            <UiButton variant="danger" :busy="busy === `del:${u.user}`" @click="remove(u)">Точно удалить</UiButton>
            <UiButton @click="confirmDel = ''">Отмена</UiButton>
          </template>
          <UiButton v-else variant="danger" @click="confirmDel = u.user">Удалить</UiButton>
        </div>
        <p v-if="confirmDel === u.user && u.clients" class="note">
          Клиенты {{ u.user }} ({{ u.clients }}) продолжат работать и перейдут к вам.
        </p>
      </template>
    </div>

    <div class="add">
      <FormField label="Новый логин" hint="2–32 символа: латиница, цифры, точка, дефис, подчёркивание">
        <input v-model="newUser" type="text" maxlength="32" autocomplete="off" placeholder="manager" />
      </FormField>
      <FormField label="Пароль" hint="Не короче 8 символов">
        <input v-model="newPass" type="password" autocomplete="new-password" />
      </FormField>
      <SegmentedControl v-model="newRole" label="Доступ" :options="ROLE_OPTIONS" />
      <div class="actions">
        <UiButton variant="primary" :busy="busy === 'add'" @click="add">Добавить админа</UiButton>
      </div>
    </div>
  </div>
</template>

<style scoped>
.users {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin-top: 14px;
}
.sub {
  font-size: 13px;
  font-weight: 600;
}
.note {
  font-size: 12.5px;
  color: var(--dim);
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  padding: 9px 11px;
}
.note.faint {
  color: var(--faint);
  border-color: transparent;
  padding: 0 2px;
}
.note.bad {
  color: var(--bad);
  border-color: color-mix(in srgb, var(--bad) 45%, transparent);
}
.user,
.add {
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  padding: 10px 12px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.u-head {
  display: flex;
  justify-content: space-between;
  gap: 10px;
  flex-wrap: wrap;
  font-size: 13px;
}
.dim {
  color: var(--faint);
}
.actions,
.pw {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}
.pw input {
  flex: 1 1 180px;
  min-width: 0;
}
</style>
