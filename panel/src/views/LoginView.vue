<script setup lang="ts">
import { computed, ref } from "vue";
import { useSessionStore } from "@/stores/session";

const session = useSessionStore();

const user = ref("");
const pass = ref("");
const pass2 = ref("");

const isSetup = computed(() => session.screen === "setup");
const locked = computed(() => session.lockedUntil > Date.now());

const canSubmit = computed(() => {
  if (session.busy || locked.value) return false;
  if (!user.value.trim() || !pass.value) return false;
  if (isSetup.value) return pass.value.length >= 8 && pass.value === pass2.value;
  return true;
});

async function submit() {
  if (!canSubmit.value) return;
  if (isSetup.value) await session.firstSetup(user.value.trim(), pass.value);
  else await session.login(user.value.trim(), pass.value);
  pass.value = "";
  pass2.value = "";
}
</script>

<template>
  <div class="wrap">
    <form class="card" @submit.prevent="submit">
      <div class="brand">
        <svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true">
          <rect x="3" y="11" width="18" height="11" rx="2" />
          <path d="M7 11V7a5 5 0 0 1 10 0v4" />
        </svg>
        <span>Detour</span>
      </div>

      <p v-if="isSetup" class="lede">
        Первый запуск. Придумайте логин и пароль администратора — других учётных
        записей у панели нет.
      </p>

      <label class="field">
        <span>Логин</span>
        <input
          v-model="user"
          type="text"
          autocomplete="username"
          :placeholder="isSetup ? 'латиница, цифры, ._- (2–32)' : ''"
          required
        />
      </label>

      <label class="field">
        <span>Пароль</span>
        <input
          v-model="pass"
          type="password"
          :autocomplete="isSetup ? 'new-password' : 'current-password'"
          :placeholder="isSetup ? 'не короче 8 символов' : ''"
          required
        />
      </label>

      <label v-if="isSetup" class="field">
        <span>Повторите пароль</span>
        <input v-model="pass2" type="password" autocomplete="new-password" required />
      </label>

      <p v-if="isSetup && pass2 && pass !== pass2" class="hint bad">
        Пароли не совпадают
      </p>

      <button class="submit" type="submit" :disabled="!canSubmit">
        {{
          session.busy
            ? "Проверяю…"
            : isSetup
              ? "Создать учётную запись"
              : "Войти"
        }}
      </button>

      <p v-if="session.error" class="hint bad">{{ session.error }}</p>
    </form>
  </div>
</template>

<style scoped>
.wrap {
  min-height: 100dvh;
  display: grid;
  place-items: center;
  padding: 24px 16px calc(24px + env(safe-area-inset-bottom));
}
.card {
  width: min(400px, 100%);
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding: 26px 24px 24px;
  border: 1px solid var(--line);
  border-radius: 18px;
  background: color-mix(in srgb, var(--ground) 88%, transparent);
  backdrop-filter: blur(14px);
  box-shadow: var(--shadow);
}
.brand {
  display: flex;
  align-items: center;
  gap: 11px;
  font-size: 20px;
  font-weight: 620;
  letter-spacing: -0.02em;
  color: var(--accent);
  margin-bottom: 2px;
}
.brand span {
  color: var(--ink);
}
.lede {
  font-size: 13.5px;
  color: var(--dim);
  line-height: 1.5;
}
.field {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.field span {
  font-size: 12.5px;
  color: var(--dim);
}
.field input {
  border: 1px solid var(--line-2);
  border-radius: var(--radius-sm);
  background: var(--panel-2);
  padding: 11px 13px;
  /* 16px, иначе iOS зумит страницу при фокусе в поле. */
  font-size: 16px;
  outline: none;
}
.field input:focus {
  border-color: var(--accent);
}
.submit {
  margin-top: 4px;
  border: 0;
  border-radius: var(--radius-sm);
  background: var(--accent);
  color: var(--accent-on);
  font-weight: 600;
  padding: 12px;
  font-size: 15px;
}
.submit:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.hint {
  font-size: 13px;
  color: var(--dim);
}
.hint.bad {
  color: var(--bad);
}
</style>
