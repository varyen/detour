<script setup lang="ts">
/* Сканер QR поверх всего: видео с задней камеры, разбор кадра несколько раз
   в секунду, первый же прочитанный код уходит наружу. Камера гасится при
   закрытии — иначе на телефоне горит индикатор и садится батарея. */
import { nextTick, onBeforeUnmount, ref, watch } from "vue";
import UiButton from "@/components/UiButton.vue";
import { decodeFrame } from "@/lib/qr";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ result: [text: string]; close: []; image: [] }>();

const video = ref<HTMLVideoElement | null>(null);
const error = ref("");
const starting = ref(false);
let stream: MediaStream | null = null;
let timer: ReturnType<typeof setTimeout> | undefined;
let stopped = true;
const canvas = document.createElement("canvas");

function stop() {
  stopped = true;
  if (timer) clearTimeout(timer);
  timer = undefined;
  stream?.getTracks().forEach((t) => t.stop());
  stream = null;
}

async function start() {
  error.value = "";
  starting.value = true;
  stopped = false;
  try {
    stream = await navigator.mediaDevices.getUserMedia({
      video: { facingMode: { ideal: "environment" }, width: { ideal: 1280 }, height: { ideal: 720 } },
      audio: false,
    });
    if (stopped) return stop();
    await nextTick();
    if (!video.value) return stop();
    video.value.srcObject = stream;
    await video.value.play();
    tick();
  } catch (e) {
    const name = (e as { name?: string }).name;
    error.value =
      name === "NotAllowedError" || name === "SecurityError"
        ? "Нет доступа к камере. Разрешите его в настройках приложения или браузера — или выберите картинку с QR-кодом."
        : name === "NotFoundError" || name === "OverconstrainedError"
          ? "Камера не найдена. Выберите картинку с QR-кодом."
          : `Камера не запустилась: ${(e as Error).message || name || "неизвестная ошибка"}`;
    stop();
  } finally {
    starting.value = false;
  }
}

async function tick() {
  if (stopped || !video.value) return;
  let text: string | null = null;
  if (video.value.readyState >= 2) {
    try {
      text = await decodeFrame(video.value, canvas);
    } catch {
      text = null;
    }
  }
  if (stopped) return;
  if (text) {
    stop();
    emit("result", text);
    return;
  }
  timer = setTimeout(tick, 180);
}

function close() {
  stop();
  emit("close");
}

/* Escape закрывает сканер, а не шторку под ним. */
function onKey(e: KeyboardEvent) {
  if (e.key !== "Escape") return;
  e.stopImmediatePropagation();
  close();
}

watch(
  () => props.open,
  (o) => {
    if (o) {
      addEventListener("keydown", onKey, true);
      void start();
    } else {
      removeEventListener("keydown", onKey, true);
      stop();
    }
  },
  { immediate: true },
);
onBeforeUnmount(() => {
  removeEventListener("keydown", onKey, true);
  stop();
});
</script>

<template>
  <Teleport to="body">
    <div v-if="open" class="qr" role="dialog" aria-modal="true" aria-label="Сканирование QR-кода">
      <video ref="video" class="cam" playsinline muted></video>
      <div class="frame" aria-hidden="true"></div>
      <div class="bar">
        <p v-if="error" class="err">{{ error }}</p>
        <p v-else class="hint">{{ starting ? "Включаю камеру…" : "Наведите камеру на QR-код профиля" }}</p>
        <div class="btns">
          <UiButton @click="emit('image')">Картинка с QR…</UiButton>
          <UiButton variant="primary" @click="close">Закрыть</UiButton>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.qr {
  position: fixed;
  inset: 0;
  z-index: 1000;
  background: #000;
  display: flex;
  flex-direction: column;
}
.cam {
  flex: 1;
  min-height: 0;
  width: 100%;
  object-fit: cover;
}
.frame {
  position: absolute;
  left: 50%;
  top: 42%;
  width: min(68vw, 300px);
  aspect-ratio: 1;
  transform: translate(-50%, -50%);
  border: 3px solid rgb(255 255 255 / 85%);
  border-radius: 18px;
  box-shadow: 0 0 0 100vmax rgb(0 0 0 / 35%);
  pointer-events: none;
}
.bar {
  position: relative;
  /* --sa-bottom — навбар Android от оболочки приложения (env() там нулевой). */
  padding: 14px 16px calc(14px + var(--sa-bottom));
  background: rgb(0 0 0 / 80%);
  color: #fff;
  display: grid;
  gap: 10px;
}
.hint,
.err {
  margin: 0;
  font-size: 14px;
  text-align: center;
}
.err {
  color: #ffb4a8;
}
.btns {
  display: flex;
  gap: 8px;
  justify-content: center;
  flex-wrap: wrap;
}
</style>
