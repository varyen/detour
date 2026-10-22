<script setup lang="ts">
/* Управление плагинами: что установлено, что есть в каталоге, установка из
   файла. Плагин — пакет роутера, ставит и удаляет его detour-update в фоне,
   а ход виден здесь же (apply_log). Сторонний пакет без подписи Detour ставится
   только после явного согласия: его код выполняется на роутере от root. */
import { computed, onMounted, ref } from "vue";
import AreaIcon from "@/components/AreaIcon.vue";
import SwitchToggle from "@/components/SwitchToggle.vue";
import UiButton from "@/components/UiButton.vue";
import { diag, plugins, poll } from "@/api";
import type { ApplyLogResponse, CatalogPlugin, InstalledPlugin, UploadedPlugin } from "@/api";
import { usePluginsStore } from "@/stores/plugins";
import { useToastStore } from "@/stores/toast";

const store = usePluginsStore();
const toast = useToastStore();

const catalog = ref<CatalogPlugin[]>([]);
const catalogError = ref("");
const busy = ref("");
const log = ref("");
const uploaded = ref<UploadedPlugin | null>(null);
const pkgFile = ref<File | null>(null);
const sigFile = ref<File | null>(null);
const allowUnsigned = ref(false);

function vcmp(a: string, b: string) {
  const x = a.split(".").map(Number);
  const y = b.split(".").map(Number);
  for (let i = 0; i < 3; i++) {
    const d = (x[i] || 0) - (y[i] || 0);
    if (d) return d;
  }
  return 0;
}

const isKeenetic = computed(() => store.platform === "keenetic");

function forThisPlatform(c: CatalogPlugin) {
  return isKeenetic.value ? !!c.files.keenetic : !!(c.files.ipk || c.files.apk);
}

function catalogOf(id: string) {
  return catalog.value.find((c) => c.id === id);
}

function updateOf(p: InstalledPlugin) {
  const c = catalogOf(p.id);
  return c && forThisPlatform(c) && vcmp(c.version, p.manifest.version) > 0 ? c.version : "";
}

const available = computed(() =>
  catalog.value.filter((c) => forThisPlatform(c) && !store.byId(c.id)),
);

async function loadCatalog(refresh: boolean) {
  if (refresh) busy.value = "catalog";
  try {
    catalog.value = (await plugins.catalog(refresh)).plugins;
    catalogError.value = "";
  } catch (e) {
    catalogError.value = e instanceof Error ? e.message : String(e);
  } finally {
    if (refresh) busy.value = "";
  }
}

async function follow(what: string): Promise<boolean> {
  const r = await poll<ApplyLogResponse | null>(() => diag.applyLog(), {
    done: (v) => v?.done === true,
    intervalMs: 1500,
    timeoutMs: 300_000,
    onTick: (v) => {
      if (v && typeof v.log === "string") log.value = v.log;
    },
  });
  const ok = !!r?.done && Number(r.rc) === 0;
  if (ok) toast.ok(what);
  else toast.error("Не получилось — подробности в журнале ниже");
  await store.load(true);
  return ok;
}

async function install(id: string, name: string) {
  busy.value = `install:${id}`;
  log.value = "";
  try {
    await plugins.install(id);
    await follow(`Плагин «${name}» установлен`);
  } catch (e) {
    toast.fromError(e, "Не удалось запустить установку");
  } finally {
    busy.value = "";
  }
}

async function remove(p: InstalledPlugin) {
  if (!window.confirm(`Удалить плагин «${p.manifest.name}»?`)) return;
  const purge = window.confirm(
    "Стереть и его настройки? «ОК» — стереть, «Отмена» — оставить на случай повторной установки.",
  );
  busy.value = `remove:${p.id}`;
  log.value = "";
  try {
    await plugins.remove(p.id, purge);
    await follow(`Плагин «${p.manifest.name}» удалён`);
  } catch (e) {
    toast.fromError(e, "Не удалось запустить удаление");
  } finally {
    busy.value = "";
  }
}

async function toggle(p: InstalledPlugin, on: boolean) {
  busy.value = `toggle:${p.id}`;
  try {
    await plugins.toggle(p.id, on);
    await store.load(true);
  } catch (e) {
    toast.fromError(e, "Не удалось переключить плагин");
  } finally {
    busy.value = "";
  }
}

function pick(e: Event, kind: "pkg" | "sig") {
  const f = (e.target as HTMLInputElement).files?.[0] ?? null;
  if (kind === "pkg") pkgFile.value = f;
  else sigFile.value = f;
  uploaded.value = null;
  allowUnsigned.value = false;
}

async function upload() {
  if (!pkgFile.value) return;
  busy.value = "upload";
  uploaded.value = null;
  try {
    await plugins.uploadSig(sigFile.value ? await sigFile.value.text() : "");
    uploaded.value = await plugins.upload(pkgFile.value);
  } catch (e) {
    toast.fromError(e, "Пакет не принят");
  } finally {
    busy.value = "";
  }
}

async function installUploaded() {
  const u = uploaded.value;
  if (!u) return;
  busy.value = "upload-install";
  log.value = "";
  try {
    await plugins.installUploaded(u.signed === false && allowUnsigned.value);
    if (await follow(`Плагин «${u.name}» установлен`)) {
      uploaded.value = null;
      pkgFile.value = null;
      sigFile.value = null;
    }
  } catch (e) {
    toast.fromError(e, "Не удалось запустить установку");
  } finally {
    busy.value = "";
  }
}

onMounted(() => {
  void store.load();
  void loadCatalog(false);
});
</script>

<template>
  <div class="pm">
    <p v-if="store.error" class="hint">{{ store.error }}</p>

    <section>
      <h3>Установлены</h3>
      <p v-if="!store.list.length" class="hint">Плагинов пока нет.</p>
      <div v-for="p in store.list" :key="p.id" class="item">
        <AreaIcon :name="p.manifest.icon || 'plug'" />
        <div class="body">
          <div class="ttl">
            {{ p.manifest.name }} <span class="ver">{{ p.manifest.version }}</span>
            <span v-if="!p.compatible" class="chip bad">{{ p.reason }}</span>
            <span v-else-if="!p.signed" class="chip warn" title="Пакет без подписи Detour">сторонний</span>
            <span v-if="updateOf(p)" class="chip ok">есть {{ updateOf(p) }}</span>
          </div>
          <small v-if="p.manifest.description">{{ p.manifest.description }}</small>
          <div class="row">
            <SwitchToggle
              :model-value="p.enabled"
              label="Включён"
              :busy="busy === `toggle:${p.id}`"
              :disabled="!!busy && busy !== `toggle:${p.id}`"
              @update:model-value="toggle(p, $event)"
            />
            <UiButton
              v-if="updateOf(p)"
              variant="primary"
              :busy="busy === `install:${p.id}`"
              :disabled="!!busy"
              @click="install(p.id, p.manifest.name)"
            >
              Обновить
            </UiButton>
            <UiButton variant="danger" :busy="busy === `remove:${p.id}`" :disabled="!!busy" @click="remove(p)">
              Удалить
            </UiButton>
          </div>
        </div>
      </div>
    </section>

    <section>
      <h3>
        Каталог
        <UiButton :busy="busy === 'catalog'" :disabled="!!busy" @click="loadCatalog(true)">Проверить</UiButton>
      </h3>
      <p v-if="catalogError" class="hint">{{ catalogError }}</p>
      <p v-else-if="!catalog.length" class="hint">Каталог ещё не загружался — нажмите «Проверить».</p>
      <p v-else-if="!available.length" class="hint">Всё из каталога для этого роутера уже установлено.</p>
      <div v-for="c in available" :key="c.id" class="item">
        <AreaIcon :name="c.icon || 'plug'" />
        <div class="body">
          <div class="ttl">{{ c.name }} <span class="ver">{{ c.version }}</span></div>
          <small v-if="c.description">{{ c.description }}</small>
          <div class="row">
            <UiButton
              variant="primary"
              :busy="busy === `install:${c.id}`"
              :disabled="!!busy"
              @click="install(c.id, c.name)"
            >
              Установить
            </UiButton>
          </div>
        </div>
      </div>
    </section>

    <section>
      <h3>Из файла</h3>
      <p class="hint">
        Пакет плагина под этот роутер: {{ isKeenetic ? "…_keenetic.ipk" : "…_all.ipk или …_noarch.apk" }}.
        Подпись .sig — если она есть: без неё пакет считается сторонним.
      </p>
      <label class="file">
        <span>Пакет</span>
        <input type="file" accept=".ipk,.apk" @change="pick($event, 'pkg')" />
      </label>
      <label class="file">
        <span>Подпись (необязательно)</span>
        <input type="file" accept=".sig" @change="pick($event, 'sig')" />
      </label>
      <div class="row">
        <UiButton :busy="busy === 'upload'" :disabled="!pkgFile || !!busy" @click="upload">Проверить пакет</UiButton>
      </div>

      <div v-if="uploaded" class="up">
        <div class="ttl">
          {{ uploaded.name }} <span class="ver">{{ uploaded.version }}</span>
          <span v-if="uploaded.signed === true" class="chip ok">подпись Detour</span>
          <span v-else-if="uploaded.signed === null" class="chip">подпись не проверить</span>
          <span v-else class="chip warn">без подписи</span>
        </div>
        <small v-if="uploaded.description">{{ uploaded.description }}</small>
        <p v-if="!uploaded.compatible" class="hint bad">{{ uploaded.reason }}</p>
        <template v-else>
          <div v-if="uploaded.signed === false" class="warn-box">
            <p>
              Этот пакет не подписан ключом Detour. Плагин получит полный доступ к роутеру: его
              скрипты выполняются от root, а интерфейс — внутри панели. Ставьте только то, чему
              доверяете.
            </p>
            <SwitchToggle v-model="allowUnsigned" label="Понимаю и доверяю автору" />
          </div>
          <div class="row">
            <UiButton
              variant="primary"
              :busy="busy === 'upload-install'"
              :disabled="!!busy || (uploaded.signed === false && !allowUnsigned)"
              @click="installUploaded"
            >
              Установить
            </UiButton>
          </div>
        </template>
      </div>
    </section>

    <pre v-if="log" class="log">{{ log }}</pre>
  </div>
</template>

<style scoped>
.pm {
  display: flex;
  flex-direction: column;
  gap: 18px;
  min-width: 0;
}
section {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-width: 0;
}
h3 {
  margin: 0;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  font-size: 13px;
  font-weight: 600;
  color: var(--dim);
}
.item {
  display: flex;
  gap: 12px;
  align-items: flex-start;
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  padding: 12px;
  min-width: 0;
}
.body {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
  flex: 1;
}
.ttl {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px 8px;
  font-weight: 600;
  overflow-wrap: anywhere;
}
.ver {
  font-family: var(--mono);
  font-weight: 400;
  font-size: 12.5px;
  color: var(--faint);
}
small {
  font-size: 12.5px;
  color: var(--dim);
}
.row {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 10px;
}
.chip {
  font-size: 11.5px;
  font-weight: 500;
  border: 1px solid var(--line-2);
  border-radius: 999px;
  padding: 2px 9px;
  color: var(--dim);
  white-space: nowrap;
}
.chip.ok {
  color: var(--ok);
  border-color: color-mix(in srgb, var(--ok) 45%, transparent);
}
.chip.warn {
  color: var(--warn);
  border-color: color-mix(in srgb, var(--warn) 45%, transparent);
}
.chip.bad {
  color: var(--bad);
  border-color: color-mix(in srgb, var(--bad) 45%, transparent);
}
.file {
  display: flex;
  flex-direction: column;
  gap: 5px;
  font-size: 12.5px;
  color: var(--dim);
}
.file input {
  font-size: 16px;
  min-height: 44px;
  border: 1px dashed var(--line-2);
  border-radius: var(--radius-sm);
  padding: 9px 10px;
  background: var(--panel-2);
  max-width: 100%;
  color: var(--ink);
}
.up {
  display: flex;
  flex-direction: column;
  gap: 10px;
  border: 1px solid var(--line-2);
  border-radius: var(--radius-sm);
  padding: 12px;
}
.warn-box {
  display: flex;
  flex-direction: column;
  gap: 10px;
  border: 1px solid color-mix(in srgb, var(--warn) 45%, transparent);
  background: color-mix(in srgb, var(--warn) 10%, transparent);
  border-radius: var(--radius-sm);
  padding: 10px 12px;
}
.warn-box p {
  margin: 0;
  font-size: 13px;
}
.hint.bad {
  color: var(--bad);
}
.log {
  margin: 0;
  font-family: var(--mono);
  font-size: 11.5px;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  max-height: 220px;
  overflow: auto;
  background: var(--panel-2);
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  padding: 10px;
}
</style>
