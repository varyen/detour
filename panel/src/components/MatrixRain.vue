<script setup lang="ts">
/* Анимированный фон панели. Единственное, что переехало из старого дизайна
   без изменений по духу: скорости заданы в СТРОКАХ В СЕКУНДУ, поэтому на
   60/90/120 Гц дождь выглядит одинаково. Палитра читается из токенов темы,
   так что переключение светлая/тёмная подхватывается на лету. */
import { onBeforeUnmount, onMounted, ref } from "vue";

const canvas = ref<HTMLCanvasElement | null>(null);

const GLYPHS =
  "アイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホマミムメモヤユヨラリルレロワヲabcdefghijklmnopqrstuvwxyz0123456789<>[]{}/*+-=$#@%&";
const SIZE = 16;
const SPEED_MIN = 4.6;
const SPEED_MAX = 16.8;
const FADE_PER_SEC = 1.8;
/* Заливка с альфой ~0.02 на 8-битном канвасе округляется в ноль — хвосты
   тогда не гаснут никогда. Поэтому гасим порциями не меньше этой. */
const FADE_MIN_STEP = 0.12;
const RESPAWN_PER_SEC = 1.44;

interface Palette {
  rgb: [number, number, number];
  base: string;
  fadeScale: number;
  lead: string;
  trail: string;
}

let ctx: CanvasRenderingContext2D | null = null;
let cols = 0;
let drops: number[] = [];
let speeds: number[] = [];
let headRow: number[] = [];
let headChar: string[] = [];
let fadeAcc = 0;
let lastTs = 0;
let raf = 0;
let pal: Palette | null = null;
let observer: MutationObserver | null = null;
let reduced = false;

function parseRgb(s: string): [number, number, number] {
  const hex = s.trim().match(/^#([0-9a-f]{3}|[0-9a-f]{6})$/i);
  if (hex) {
    let h = hex[1];
    if (h.length === 3) h = h[0] + h[0] + h[1] + h[1] + h[2] + h[2];
    return [
      parseInt(h.slice(0, 2), 16),
      parseInt(h.slice(2, 4), 16),
      parseInt(h.slice(4, 6), 16),
    ];
  }
  const m = s.match(/(\d+)[,\s]+(\d+)[,\s]+(\d+)/);
  return m ? [+m[1], +m[2], +m[3]] : [8, 12, 18];
}

function readPalette(): Palette {
  const cs = getComputedStyle(document.documentElement);
  /* Именно токен --ground, а не backgroundColor: на переключении темы у фона
     идёт CSS-переход, и computed-цвет ещё старый — дождь тогда замывал бы
     себя предыдущим цветом до следующего ресайза. */
  const rgb = parseRgb(cs.getPropertyValue("--ground"));
  const lum = (rgb[0] * 0.299 + rgb[1] * 0.587 + rgb[2] * 0.114) / 255;
  return {
    rgb,
    base: `rgb(${rgb.join(",")})`,
    /* Тёмные глифы на светлом фоне держатся дольше при той же альфе. */
    fadeScale: lum > 0.5 ? 1.35 : 1,
    lead: cs.getPropertyValue("--rain-lead").trim(),
    trail: cs.getPropertyValue("--rain-trail").trim(),
  };
}

const glyph = () => GLYPHS[(Math.random() * GLYPHS.length) | 0];
const randSpeed = () =>
  SPEED_MIN * Math.pow(SPEED_MAX / SPEED_MIN, Math.random());

function resize() {
  const el = canvas.value;
  if (!el || !ctx) return;
  pal = readPalette();
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  el.width = Math.floor(innerWidth * dpr);
  el.height = Math.floor(innerHeight * dpr);
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.fillStyle = pal.base;
  ctx.fillRect(0, 0, innerWidth, innerHeight);

  cols = Math.ceil(innerWidth / SIZE);
  const rows = innerHeight / SIZE;
  drops = new Array(cols);
  speeds = new Array(cols);
  headRow = new Array(cols);
  headChar = new Array(cols);
  fadeAcc = 0;
  lastTs = 0;
  for (let i = 0; i < cols; i++) {
    /* Раскидываем и выше экрана: колонка, стартующая за экраном, иначе
       появилась бы только секунд через пятнадцать. */
    drops[i] = (Math.random() * 1.4 - 0.7) * rows;
    speeds[i] = randSpeed();
    headRow[i] = -1;
    headChar[i] = glyph();
  }
  if (reduced) staticFrame(rows);
}

/** При prefers-reduced-motion рисуем один разреженный кадр и больше не трогаем. */
function staticFrame(rows: number) {
  if (!ctx || !pal) return;
  ctx.font = `${SIZE}px ${getComputedStyle(document.body).fontFamily}`;
  for (let i = 0; i < cols; i++) {
    for (let k = 0; k < 3; k++) {
      ctx.fillStyle = k === 0 ? pal.lead : pal.trail;
      ctx.fillText(glyph(), i * SIZE, Math.random() * rows * SIZE);
    }
  }
}

function frame(ts: number) {
  raf = requestAnimationFrame(frame);
  if (!ctx || !pal) return;
  if (!lastTs) {
    lastTs = ts;
    return;
  }
  const dt = Math.min((ts - lastTs) / 1000, 0.1);
  lastTs = ts;

  fadeAcc += FADE_PER_SEC * pal.fadeScale * dt;
  if (fadeAcc >= FADE_MIN_STEP) {
    ctx.fillStyle = `rgba(${pal.rgb.join(",")},${Math.min(fadeAcc, 0.5)})`;
    ctx.fillRect(0, 0, innerWidth, innerHeight);
    fadeAcc = 0;
  }

  ctx.font = `${SIZE}px ${getComputedStyle(document.body).fontFamily}`;
  const rows = innerHeight / SIZE;
  for (let i = 0; i < cols; i++) {
    drops[i] += speeds[i] * dt;
    const r = Math.floor(drops[i]);
    if (r !== headRow[i]) {
      headRow[i] = r;
      headChar[i] = glyph();
    }
    if (r >= 0 && r <= rows) {
      ctx.fillStyle = pal.trail;
      ctx.fillText(headChar[i], i * SIZE, (r - 1) * SIZE);
      ctx.fillStyle = pal.lead;
      ctx.fillText(headChar[i], i * SIZE, r * SIZE);
    }
    if (drops[i] > rows && Math.random() < RESPAWN_PER_SEC * dt) {
      drops[i] = -Math.random() * rows * 0.6;
      speeds[i] = randSpeed();
    }
  }
}

onMounted(() => {
  const el = canvas.value;
  if (!el) return;
  ctx = el.getContext("2d", { alpha: false });
  if (!ctx) return;
  reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
  resize();
  addEventListener("resize", resize);
  observer = new MutationObserver(() => {
    pal = readPalette();
  });
  observer.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["data-theme"],
  });
  if (!reduced) raf = requestAnimationFrame(frame);
});

onBeforeUnmount(() => {
  cancelAnimationFrame(raf);
  removeEventListener("resize", resize);
  observer?.disconnect();
});
</script>

<template>
  <canvas ref="canvas" class="rain" aria-hidden="true"></canvas>
</template>

<style scoped>
.rain {
  position: fixed;
  inset: 0;
  width: 100%;
  height: 100%;
  display: block;
  z-index: -2;
  pointer-events: none;
}
</style>
