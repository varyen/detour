<script setup lang="ts">
/* График трафика по трём направлениям: напрямую, через VPN, обход DPI.
 *
 * СЛОВО. В коде это `lane` (и токены --lane-*), по-русски — НАПРАВЛЕНИЕ, а не
 * «лента»: полоса движения так и называется, а «лента» читается как лента
 * новостей. Идентификаторы английские, подписи русские — так и держим.
 *
 * ФОРМА. Данные отвечают на вопрос «как менялся объём и чья это была доля» —
 * это изменение во времени плюс состав целого, то есть стековые площади. Не
 * три отдельные линии: они отвечают на другой вопрос (сравнить направления между
 * собой), а нам важна и сумма тоже.
 *
 * ЦВЕТ. Направления красятся токенами --lane-*, теми же, что и схема потока:
 * одна сущность — один цвет на всём экране. Шаги валидированы скриптом из
 * гайда по визуализации в обеих схемах; в светлой теме два оттенка не
 * добирают контраста 3:1 к подложке, поэтому обязательны видимые подписи и
 * табличный вид — они здесь есть, это не украшение.
 *
 * ЧЕГО ЗДЕСЬ НЕТ НАМЕРЕННО. Второй оси (две шкалы на одном поле врут о
 * соотношении) и числа над каждой точкой — подписываются только концы.
 */
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { overview } from "../../api";
import { useElementWidth } from "../../composables/useElementWidth";
import { fmtBytes, fmtBitrate } from "../../lib/format";

/* Живая скорость прямо сейчас — из traffic_counters (дельта rx/tx за span),
   которую «Обзор» и так опрашивает раз в 10 с. Второго читателя этого действия
   заводить нельзя: `read` разрушающий, два опроса делят интервал пополам.
   Поэтому число приходит пропсами, а не своим запросом. null — «прогрев» либо
   счётчик поехал назад: показываем «—», а не ноль. */
const props = defineProps<{
  rxSpeed?: number | null;
  txSpeed?: number | null;
}>();

/* Что показывает поле: доли по направлениям (напрямую/VPN/DPI) или скорость
   приёма и передачи во времени. Живой показ ↓/↑ виден в обоих режимах. */
type Mode = "lanes" | "speed";
const mode = ref<Mode>("lanes");

type Point = {
  ts: number;
  direct: number;
  vpn: number;
  bypass: number;
  rx: number;
  tx: number;
};
/* Источник — то, что реально копит роутер: `minute` (сутки поминутно, tmpfs) и
   `hour` (30 дней по часам, флеш). Все диапазоны панели — это ОКНА поверх этих
   двух рядов, а не новые запросы к бэкенду: роутер уже отдаёт оба целиком. */
type Source = "minute" | "hour";
type PresetKey = "1h" | "6h" | "24h" | "7d" | "30d" | "custom";

const LANES = [
  { key: "direct", label: "Напрямую", color: "var(--lane-direct)" },
  { key: "vpn", label: "Через VPN", color: "var(--lane-vpn)" },
  { key: "bypass", label: "Обход DPI", color: "var(--lane-bypass)" },
] as const;

/* windowSec — сколько секунд от последней точки показать; null — весь ряд
   (столько, сколько источник вообще хранит). */
const PRESETS: {
  key: Exclude<PresetKey, "custom">;
  label: string;
  source: Source;
  windowSec: number | null;
}[] = [
  { key: "1h", label: "Час", source: "minute", windowSec: 3600 },
  { key: "6h", label: "6 ч", source: "minute", windowSec: 6 * 3600 },
  { key: "24h", label: "Сутки", source: "minute", windowSec: null },
  { key: "7d", label: "Неделя", source: "hour", windowSec: 7 * 86400 },
  { key: "30d", label: "Месяц", source: "hour", windowSec: null },
];

/* Пределы истории: минутный ряд держит сутки, часовой — 30 дней. Вне их данных
   нет, поэтому кастомный интервал по ним же и ограничивается. */
const MINUTE_WINDOW = 24 * 3600;
const HOUR_WINDOW = 30 * 86400;

const preset = ref<PresetKey>("24h");
/* Кэш обоих рядов: переключение пресетов внутри одного источника не гоняет сеть
   лишний раз, а кастомный интервал может выбирать источник на лету. */
const minutePoints = ref<Point[]>([]);
const hourPoints = ref<Point[]>([]);
const supported = ref(true);
const loading = ref(false);
const showTable = ref(false);
const hover = ref<number | null>(null);

/* Применённый кастомный интервал (границы в unix-секундах + выбранный источник).
   Отдельно от полей ввода: пока не нажали «Показать», график не дёргается. */
const customOpen = ref(false);
const customFrom = ref("");
const customTo = ref("");
const customErr = ref("");
const applied = ref<{ from: number; to: number; source: Source } | null>(null);

const currentSource = computed<Source>(() =>
  preset.value === "custom"
    ? applied.value?.source ?? "hour"
    : PRESETS.find((p) => p.key === preset.value)!.source,
);

const points = computed<Point[]>(() => {
  const src = currentSource.value === "minute" ? minutePoints.value : hourPoints.value;
  if (!src.length) return src;
  if (preset.value === "custom") {
    const c = applied.value;
    if (!c) return [];
    return src.filter((p) => p.ts >= c.from && p.ts <= c.to);
  }
  const cfg = PRESETS.find((p) => p.key === preset.value)!;
  if (cfg.windowSec == null) return src;
  /* Окно считаем от ПОСЛЕДНЕЙ точки данных, а не от текущего времени: если сбор
     на минуту отставал, показываем последний час собранного, а не пустой хвост
     из-за расхождения часов браузера и роутера. */
  const cutoff = src[src.length - 1].ts - cfg.windowSec;
  return src.filter((p) => p.ts >= cutoff);
});

/* Геометрия — В ПИКСЕЛЯХ: viewBox равен реальному размеру поля, замеренному
   ResizeObserver. Раньше здесь стояла фиксированная ширина 720 и
   `preserveAspectRatio="none"`, и на широком экране браузер растягивал
   картинку по X в два с лишним раза — вместе с ней плющились подписи осей и
   обводки направлений (вертикаль при этом не менялась: высота задана в пикселях).
   Единица viewBox = пиксель, поэтому 10px-подпись остаётся 10px при любой
   ширине карточки. */
const plot = ref<HTMLElement | null>(null);
const W = useElementWidth(plot);
const H = 180;
const PAD_L = 8;
const PAD_R = 8;
const PAD_T = 10;
const PAD_B = 18;

let timer: number | undefined;

async function load(src: Source = currentSource.value) {
  loading.value = true;
  try {
    const r = await overview.trafficSeries(src);
    if (r?.supported === false || r?.ok === false) {
      supported.value = false;
      if (src === "minute") minutePoints.value = [];
      else hourPoints.value = [];
    } else {
      supported.value = true;
      /* Роутер со старым сборщиком отдаёт точки без rx/tx — приводим к нулю,
         чтобы график скорости не падал на undefined, а честно показал разрыв. */
      const pts: Point[] = (Array.isArray(r?.points) ? r.points : []).map((p) => ({
        ts: p.ts,
        direct: p.direct,
        vpn: p.vpn,
        bypass: p.bypass,
        rx: p.rx ?? 0,
        tx: p.tx ?? 0,
      }));
      if (src === "minute") minutePoints.value = pts;
      else hourPoints.value = pts;
    }
  } finally {
    loading.value = false;
  }
}

onMounted(() => {
  void load();
  /* Оба ряда пополняются не чаще раза в минуту (минутный — по cron, часовой —
     и того реже), поэтому опрашиваем текущий источник раз в минуту. */
  timer = window.setInterval(() => void load(), 60_000);
});
onUnmounted(() => timer && clearInterval(timer));
/* Смена источника — единственный повод сходить в сеть заранее; окно того же
   источника перестраивается из кэша без запроса. */
watch(currentSource, (s) => void load(s));

const totals = computed(() => {
  const t = { direct: 0, vpn: 0, bypass: 0 };
  for (const p of points.value) {
    t.direct += p.direct;
    t.vpn += p.vpn;
    t.bypass += p.bypass;
  }
  return t;
});
const grandTotal = computed(() => totals.value.direct + totals.value.vpn + totals.value.bypass);

/* Суммы приёма и передачи за окно — байты, а не скорость. Средняя скорость в
   легенде = сумма / длину окна: одно понятное число вместо мельтешащего пика. */
const dirTotals = computed(() => {
  const t = { rx: 0, tx: 0 };
  for (const p of points.value) {
    t.rx += p.rx;
    t.tx += p.tx;
  }
  return t;
});

/* Пик суммы задаёт шкалу. Ноль в знаменателе означает «трафика не было» —
   рисуем пустое поле, а не делим на ноль. */
const peak = computed(() =>
  points.value.reduce((m, p) => Math.max(m, p.direct + p.vpn + p.bypass), 0),
);

const geom = computed(() => {
  const pts = points.value;
  const n = pts.length;
  /* Меньше двух точек — рисовать площадь не из чего (одна точка дала бы
     вырожденный отрезок и пустую сетку). Отдаём null → показываем заглушку. */
  if (n < 2 || peak.value <= 0) return null;
  const x = (i: number) => PAD_L + (i * (W.value - PAD_L - PAD_R)) / Math.max(1, n - 1);
  const y = (v: number) => PAD_T + (H - PAD_T - PAD_B) * (1 - v / peak.value);

  /* Стек снизу вверх: напрямую → VPN → обход. Каждое направление — замкнутая
     область между своей верхней границей и границей предыдущей.
     Направление, у которого за всё окно ноль байт, НЕ рисуется: площадь нулевой
     высоты, но обводка верхней границы легла бы поверх соседнего направления и
     читалась бы как настоящий трафик (жёлтая линия «Обход DPI» через весь
     график при нуле — именно это и было видно на первом же скриншоте).
     В легенде направление остаётся — с честным нулём. */
  const areas = LANES.filter((lane) => pts.some((p) => p[lane.key] > 0)).map((lane) => {
    const li = LANES.findIndex((l) => l.key === lane.key);
    const upper: string[] = [];
    const lower: string[] = [];
    pts.forEach((p, i) => {
      let below = 0;
      for (let k = 0; k < li; k++) below += p[LANES[k].key];
      const top = below + p[lane.key];
      upper.push(`${x(i).toFixed(1)},${y(top).toFixed(1)}`);
      lower.push(`${x(i).toFixed(1)},${y(below).toFixed(1)}`);
    });
    return {
      key: lane.key,
      label: lane.label,
      color: lane.color,
      fill: `M${upper.join("L")}L${lower.reverse().join("L")}Z`,
      line: `M${upper.join("L")}`,
    };
  });
  return { x, y, areas, n };
});

/* --- скорость приёма/передачи во времени --- */

const DIRS = [
  { key: "rx", label: "Приём", color: "var(--dir-rx)" },
  { key: "tx", label: "Передача", color: "var(--dir-tx)" },
] as const;

/* Точки ряда — это байты ЗА интервал, а не скорость. Переводим в байты/с по
   РАЗНОСТИ соседних ts, а не по step: на Keenetic нет crond, шаг ряда там 300 с
   против 60 на OpenWrt, и деление на step завысило бы скорость в пять раз.
   Индекс совпадает с points, поэтому наведение и подписи оси общие для обоих
   режимов. Для первой точки берём интервал второй (её собственный неизвестен). */
const rates = computed(() => {
  const pts = points.value;
  if (pts.length < 2) return [] as { ts: number; rx: number; tx: number }[];
  const dt0 = Math.max(1, pts[1].ts - pts[0].ts);
  return pts.map((p, i) => {
    const dt = i > 0 ? Math.max(1, p.ts - pts[i - 1].ts) : dt0;
    return { ts: p.ts, rx: p.rx / dt, tx: p.tx / dt };
  });
});

/* Пик — по большей из двух скоростей: шкала симметрична относительно оси, но
   общая, чтобы приём и передача сравнивались честно (передача обычно много
   меньше — эту асимметрию график и показывает, а не прячет двумя шкалами). */
const speedPeak = computed(() =>
  rates.value.reduce((m, r) => Math.max(m, r.rx, r.tx), 0),
);

const speedGeom = computed(() => {
  const rs = rates.value;
  const n = rs.length;
  if (n < 2 || speedPeak.value <= 0) return null;
  const x = (i: number) => PAD_L + (i * (W.value - PAD_L - PAD_R)) / Math.max(1, n - 1);
  const half = (H - PAD_T - PAD_B) / 2;
  const axisY = PAD_T + half;
  const yUp = (v: number) => axisY - half * (v / speedPeak.value); // приём — вверх
  const yDn = (v: number) => axisY + half * (v / speedPeak.value); // передача — вниз

  const build = (pick: (r: { rx: number; tx: number }) => number, yv: (v: number) => number) => {
    const line: string[] = [];
    rs.forEach((r, i) => line.push(`${x(i).toFixed(1)},${yv(pick(r)).toFixed(1)}`));
    const x0 = x(0).toFixed(1);
    const xn = x(n - 1).toFixed(1);
    const a = axisY.toFixed(1);
    return {
      fill: `M${x0},${a}L${line.join("L")}L${xn},${a}Z`,
      line: `M${line.join("L")}`,
    };
  };

  const rx = build((r) => r.rx, yUp);
  const tx = build((r) => r.tx, yDn);
  return { x, n, axisY, rx, tx };
});

/* Общее число точек активного режима — по нему считаем наведение и подписи. */
const activeN = computed(() =>
  mode.value === "speed" ? speedGeom.value?.n ?? 0 : geom.value?.n ?? 0,
);

const hoverPoint = computed(() => {
  const i = hover.value;
  if (i === null || !points.value[i]) return null;
  const p = points.value[i];
  const r = rates.value[i];
  return { i, p, r, total: p.direct + p.vpn + p.bypass };
});

function onMove(e: MouseEvent) {
  const n = activeN.value;
  if (n < 2) return;
  const box = (e.currentTarget as SVGElement).getBoundingClientRect();
  const rel = ((e.clientX - box.left) / box.width) * W.value;
  const step = (W.value - PAD_L - PAD_R) / Math.max(1, n - 1);
  const i = Math.round((rel - PAD_L) / step);
  hover.value = Math.min(n - 1, Math.max(0, i));
}

/* Живая скорость сейчас — строкой; «—», когда прогрев или откат счётчика. */
const rxNow = computed(() =>
  typeof props.rxSpeed === "number" ? fmtBitrate(props.rxSpeed) : "—",
);
const txNow = computed(() =>
  typeof props.txSpeed === "number" ? fmtBitrate(props.txSpeed) : "—",
);

const pad = (n: number) => String(n).padStart(2, "0");

/* Размах показанного окна в секундах — по нему выбираем формат подписи оси. */
const spanSec = computed(() => {
  const p = points.value;
  return p.length >= 2 ? p[p.length - 1].ts - p[0].ts : 0;
});

/* Подпись оси адаптивна к масштабу: внутри суток — время, на неделе/месяце —
   дата (с часом, пока окно не переросло несколько дней). */
function fmtTime(ts: number) {
  const d = new Date(ts * 1000);
  const day = `${pad(d.getDate())}.${pad(d.getMonth() + 1)}`;
  const time = `${pad(d.getHours())}:${pad(d.getMinutes())}`;
  if (currentSource.value === "hour") {
    return spanSec.value > 3 * 86400 ? day : `${day} ${pad(d.getHours())}:00`;
  }
  /* Минутное окно максимум сутки, но может пересечь полночь — тогда полезнее
     показать и день. */
  return spanSec.value > 12 * 3600 ? `${day} ${time}` : time;
}

/* В подсказке всегда полная дата-время: точка может быть где угодно в месяце. */
function fmtTip(ts: number) {
  const d = new Date(ts * 1000);
  return `${pad(d.getDate())}.${pad(d.getMonth() + 1)} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

const axisLabels = computed(() => {
  const pts = points.value;
  if (pts.length < 2) return [];
  /* x берём у активной геометрии: в режиме скорости ленточная geom может быть
     null (доли нулевые, а приём/передача есть) — тогда подписи схлопнулись бы в
     ноль. Обе геометрии считают x по одной формуле, поэтому позиции совпадают. */
  const xf = mode.value === "speed" ? speedGeom.value?.x : geom.value?.x;
  /* Больше подписей на широком поле — на узком они бы наехали друг на друга. */
  const count = W.value > 560 ? 5 : 3;
  const seen = new Set<number>();
  const out: { i: number; text: string; x: number; anchor: string }[] = [];
  for (let k = 0; k < count; k++) {
    const i = Math.round((k * (pts.length - 1)) / (count - 1));
    if (seen.has(i)) continue;
    seen.add(i);
    out.push({
      i,
      text: fmtTime(pts[i].ts),
      x: xf?.(i) ?? 0,
      anchor: i === 0 ? "start" : i === pts.length - 1 ? "end" : "middle",
    });
  }
  return out;
});

/* --- кастомный интервал --- */

/* Date → строка для <input type="datetime-local"> в ЛОКАЛЬНОМ времени (значение
   без таймзоны, поэтому toISOString с его UTC-сдвигом тут не годится). */
function toLocalInput(d: Date) {
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/* Границы полей: раньше 30 дней истории всё равно нет. */
const customMin = computed(() => toLocalInput(new Date(Date.now() - HOUR_WINDOW * 1000)));
const customMax = computed(() => toLocalInput(new Date()));

function openCustom() {
  customErr.value = "";
  if (!customFrom.value || !customTo.value) {
    const now = new Date();
    customTo.value = toLocalInput(now);
    customFrom.value = toLocalInput(new Date(now.getTime() - 3 * 3600 * 1000));
  }
  customOpen.value = true;
}

function pickPreset(k: PresetKey) {
  preset.value = k;
  customOpen.value = false;
}

function applyCustom() {
  const from = Math.floor(new Date(customFrom.value).getTime() / 1000);
  const to = Math.floor(new Date(customTo.value).getTime() / 1000);
  if (!Number.isFinite(from) || !Number.isFinite(to)) {
    customErr.value = "Укажи обе границы";
    return;
  }
  if (to <= from) {
    customErr.value = "Конец раньше начала";
    return;
  }
  /* Минутное разрешение доступно, только если весь интервал попадает в сутки,
     которые хранит tmpfs-ряд; иначе берём часовой (30 дней). */
  const nowSec = Date.now() / 1000;
  const source: Source =
    to - from <= MINUTE_WINDOW && from >= nowSec - MINUTE_WINDOW - 120 ? "minute" : "hour";
  applied.value = { from, to, source };
  customErr.value = "";
  preset.value = "custom";
  void load(source);
}
</script>

<template>
  <section class="chart">
    <!-- Скорость прямо сейчас — крупно и в обоих режимах: это первое, за чем
         открывают карточку. Приём и передача теми же цветами, что и график
         скорости ниже. -->
    <div class="now" aria-label="Скорость сейчас">
      <div class="now-cell">
        <span class="arrow" :style="{ color: 'var(--dir-rx)' }">↓</span>
        <span class="val">{{ rxNow }}</span>
        <span class="cap">приём</span>
      </div>
      <div class="now-cell">
        <span class="arrow" :style="{ color: 'var(--dir-tx)' }">↑</span>
        <span class="val">{{ txNow }}</span>
        <span class="cap">передача</span>
      </div>
      <!-- Что рисует поле: доли по направлениям или скорость во времени. -->
      <div class="mode" role="group" aria-label="Что показывать">
        <button type="button" :class="{ on: mode === 'lanes' }"
          :aria-pressed="mode === 'lanes'" @click="mode = 'lanes'">Направления</button>
        <button type="button" :class="{ on: mode === 'speed' }"
          :aria-pressed="mode === 'speed'" @click="mode = 'speed'">Скорость</button>
      </div>
    </div>

    <header>
      <div>
        <p class="sub">
          <template v-if="mode === 'speed'">
            <template v-if="speedGeom">Скорость приёма и передачи за период</template>
            <template v-else-if="!supported">Сборщик не установлен</template>
            <template v-else-if="loading">Загружаю…</template>
            <template v-else-if="points.length < 2">
              Данные копятся — график появится через пару минут
            </template>
            <template v-else>За период трафика не было</template>
          </template>
          <template v-else>
            <template v-if="geom && grandTotal > 0">
              Всего за период {{ fmtBytes(grandTotal) }}
            </template>
            <template v-else-if="!supported">Сборщик не установлен</template>
            <template v-else-if="loading">Загружаю…</template>
            <template v-else-if="points.length < 2">
              Данные копятся — график появится через пару минут
            </template>
            <template v-else>За период трафика не было</template>
          </template>
        </p>
      </div>
      <!-- Пресеты периода одной строкой над графиком; последний чип —
           произвольный интервал. -->
      <div class="ranges" role="group" aria-label="Период">
        <button
          v-for="r in PRESETS"
          :key="r.key"
          type="button"
          :class="{ on: preset === r.key }"
          :aria-pressed="preset === r.key"
          @click="pickPreset(r.key)"
        >
          {{ r.label }}
        </button>
        <button
          type="button"
          class="custom-chip"
          :class="{ on: preset === 'custom' || customOpen }"
          :aria-pressed="preset === 'custom'"
          @click="customOpen ? (customOpen = false) : openCustom()"
        >
          Свой…
        </button>
      </div>
    </header>

    <!-- Редактор произвольного интервала: раскрывается по чипу «Свой…». -->
    <div v-if="customOpen" class="custom" role="group" aria-label="Свой интервал">
      <label>
        <span>С</span>
        <input v-model="customFrom" type="datetime-local" :min="customMin" :max="customMax" />
      </label>
      <label>
        <span>По</span>
        <input v-model="customTo" type="datetime-local" :min="customMin" :max="customMax" />
      </label>
      <button type="button" class="apply" @click="applyCustom">Показать</button>
      <span v-if="customErr" class="cerr">{{ customErr }}</span>
      <span v-else class="chint">История: сутки поминутно, 30 дней по часам</span>
    </div>

    <div ref="plot" class="plot">
      <!-- Режим «Направления»: стековые площади долей. -->
      <svg
        v-if="mode === 'lanes' && geom"
        :viewBox="`0 0 ${W} ${H}`"
        role="img"
        :aria-label="`График трафика: всего ${fmtBytes(grandTotal)}`"
        @mousemove="onMove"
        @mouseleave="hover = null"
      >
        <!-- Сетка намеренно бледная: она справочная, а не содержательная. -->
        <line
          v-for="f in [0.25, 0.5, 0.75]"
          :key="f"
          class="grid"
          :x1="PAD_L"
          :x2="W - PAD_R"
          :y1="PAD_T + (H - PAD_T - PAD_B) * f"
          :y2="PAD_T + (H - PAD_T - PAD_B) * f"
        />
        <g v-for="a in geom.areas" :key="a.key">
          <path :d="a.fill" :fill="a.color" class="area" />
          <!-- Зазор в 2px между заливками: границы направлений не слипаются. -->
          <path :d="a.line" :stroke="a.color" class="edge" fill="none" />
        </g>
        <g v-if="hoverPoint">
          <line
            class="cross"
            :x1="geom.x(hoverPoint.i)"
            :x2="geom.x(hoverPoint.i)"
            :y1="PAD_T"
            :y2="H - PAD_B"
          />
        </g>
        <text
          v-for="l in axisLabels"
          :key="l.i"
          class="axis"
          :x="l.x"
          :y="H - 4"
          :text-anchor="l.anchor"
        >
          {{ l.text }}
        </text>
      </svg>

      <!-- Режим «Скорость»: зеркальные площади — приём вверх от оси, передача
           вниз. Ось посередине, шкала общая, поэтому асимметрия видна как есть. -->
      <svg
        v-else-if="mode === 'speed' && speedGeom"
        :viewBox="`0 0 ${W} ${H}`"
        role="img"
        :aria-label="`График скорости: приём ${rxNow}, передача ${txNow}`"
        @mousemove="onMove"
        @mouseleave="hover = null"
      >
        <path :d="speedGeom.rx.fill" fill="var(--dir-rx)" class="area" />
        <path :d="speedGeom.rx.line" stroke="var(--dir-rx)" class="edge" fill="none" />
        <path :d="speedGeom.tx.fill" fill="var(--dir-tx)" class="area" />
        <path :d="speedGeom.tx.line" stroke="var(--dir-tx)" class="edge" fill="none" />
        <!-- Нулевая ось: приём над ней, передача под ней. -->
        <line class="axis-mid" :x1="PAD_L" :x2="W - PAD_R" :y1="speedGeom.axisY" :y2="speedGeom.axisY" />
        <g v-if="hoverPoint">
          <line
            class="cross"
            :x1="speedGeom.x(hoverPoint.i)"
            :x2="speedGeom.x(hoverPoint.i)"
            :y1="PAD_T"
            :y2="H - PAD_B"
          />
        </g>
        <text
          v-for="l in axisLabels"
          :key="l.i"
          class="axis"
          :x="l.x"
          :y="H - 4"
          :text-anchor="l.anchor"
        >
          {{ l.text }}
        </text>
      </svg>

      <!-- Заглушка живёт ВНУТРИ поля: контейнер должен существовать всегда,
           иначе мерить нечего и первый кадр с данными нарисуется по
           запасной ширине. -->
      <p v-else class="empty">
        {{
          !supported
            ? "Нет detour-trafficlog"
            : points.length < 2
              ? "Мало данных за этот период — точки ещё копятся"
              : "За этот период трафика не было"
        }}
      </p>

      <div v-if="hoverPoint && mode === 'lanes'" class="tip">
        <b>{{ fmtTip(hoverPoint.p.ts) }}</b>
        <span v-for="lane in LANES" :key="lane.key">
          <i class="sw" :style="{ background: lane.color }"></i>
          {{ lane.label }} <b>{{ fmtBytes(hoverPoint.p[lane.key]) }}</b>
        </span>
      </div>
      <div v-else-if="hoverPoint && hoverPoint.r && mode === 'speed'" class="tip">
        <b>{{ fmtTip(hoverPoint.p.ts) }}</b>
        <span v-for="dir in DIRS" :key="dir.key">
          <i class="sw" :style="{ background: dir.color }"></i>
          {{ dir.label }} <b>{{ fmtBitrate(hoverPoint.r[dir.key]) }}</b>
        </span>
      </div>
    </div>

    <!-- Легенда обязательна: идентичность не должна держаться на одном цвете.
         Числа рядом — те самые видимые подписи. В режиме направлений — суммы за
         период; в режиме скорости — средняя скорость за период (общий объём,
         делённый на длину окна). -->
    <div class="legend">
      <template v-if="mode === 'speed'">
        <div v-for="dir in DIRS" :key="dir.key">
          <i class="sw" :style="{ background: dir.color }"></i>
          {{ dir.label }}
          <b class="num">{{ spanSec ? fmtBitrate(dirTotals[dir.key] / spanSec) : "—" }}</b>
          <span class="sub-num">· {{ fmtBytes(dirTotals[dir.key]) }}</span>
        </div>
      </template>
      <template v-else>
        <div v-for="lane in LANES" :key="lane.key">
          <i class="sw" :style="{ background: lane.color }"></i>
          {{ lane.label }}
          <b class="num">{{ fmtBytes(totals[lane.key]) }}</b>
        </div>
      </template>
      <button type="button" class="tbl" @click="showTable = !showTable">
        {{ showTable ? "Скрыть таблицу" : "Таблицей" }}
      </button>
    </div>

    <!-- Табличный вид: и как доступная альтернатива цвету, и просто чтобы
         посмотреть числа. -->
    <table v-if="showTable && mode === 'speed' && rates.length" class="data">
      <thead>
        <tr>
          <th>Время</th>
          <th v-for="dir in DIRS" :key="dir.key">{{ dir.label }}</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="r in rates.slice(-24).reverse()" :key="r.ts">
          <td>{{ fmtTip(r.ts) }}</td>
          <td>{{ fmtBitrate(r.rx) }}</td>
          <td>{{ fmtBitrate(r.tx) }}</td>
        </tr>
      </tbody>
    </table>
    <table v-else-if="showTable && points.length" class="data">
      <thead>
        <tr>
          <th>Время</th>
          <th v-for="lane in LANES" :key="lane.key">{{ lane.label }}</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="p in points.slice(-24).reverse()" :key="p.ts">
          <td>{{ fmtTip(p.ts) }}</td>
          <td>{{ fmtBytes(p.direct) }}</td>
          <td>{{ fmtBytes(p.vpn) }}</td>
          <td>{{ fmtBytes(p.bypass) }}</td>
        </tr>
      </tbody>
    </table>
  </section>
</template>

<style scoped>
.chart {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

/* Живая скорость сейчас — крупная строка над графиком. */
.now {
  display: flex;
  align-items: center;
  gap: 18px 24px;
  flex-wrap: wrap;
}
.now-cell {
  display: flex;
  align-items: baseline;
  gap: 6px;
  min-width: 0;
}
.now-cell .arrow {
  font-size: 18px;
  font-weight: 700;
  line-height: 1;
}
.now-cell .val {
  font-size: 20px;
  font-weight: 700;
  color: var(--ink);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}
.now-cell .cap {
  font-size: 11.5px;
  color: var(--faint);
}
/* Переключатель режима прижат вправо. */
.mode {
  margin-left: auto;
  display: flex;
  gap: 4px;
}
.mode button {
  border: 1px solid var(--line-2);
  background: var(--panel-2);
  color: var(--dim);
  border-radius: 999px;
  padding: 5px 12px;
  font: inherit;
  font-size: 12px;
  cursor: pointer;
}
.mode button.on {
  background: var(--accent);
  border-color: var(--accent);
  color: var(--accent-on);
  font-weight: 600;
}

header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
}
header > div:first-child {
  flex: 1 1 auto;
  min-width: 0;
}
.sub {
  margin: 0;
  font-size: 12px;
  color: var(--faint);
}
.ranges {
  display: flex;
  gap: 4px;
  flex-wrap: wrap;
  justify-content: flex-end;
}
.ranges button {
  border: 1px solid var(--line-2);
  background: var(--panel-2);
  color: var(--dim);
  border-radius: 999px;
  padding: 5px 11px;
  font: inherit;
  font-size: 12px;
  cursor: pointer;
}
.ranges button.on {
  background: var(--accent);
  border-color: var(--accent);
  color: var(--accent-on);
  font-weight: 600;
}
.custom-chip {
  border-style: dashed !important;
}

/* Редактор произвольного интервала. */
.custom {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px 12px;
  padding: 10px 12px;
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  background: var(--panel-2);
  font-size: 12px;
}
.custom label {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: var(--dim);
}
.custom input {
  border: 1px solid var(--line-2);
  background: var(--panel);
  color: var(--ink);
  border-radius: 8px;
  padding: 4px 8px;
  font: inherit;
  font-size: 12px;
  color-scheme: dark light;
}
.custom .apply {
  border: 1px solid var(--accent);
  background: var(--accent);
  color: var(--accent-on);
  border-radius: 999px;
  padding: 5px 13px;
  font: inherit;
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
}
.custom .chint {
  color: var(--faint);
}
.custom .cerr {
  color: var(--bad);
}
.plot {
  position: relative;
}
svg {
  width: 100%;
  height: 180px;
  display: block;
  overflow: visible;
}
.grid {
  stroke: var(--line);
  stroke-width: 1;
}
.area {
  opacity: 0.55;
}
.edge {
  stroke-width: 2;
  stroke-linejoin: round;
  /* Зазор к соседней заливке — не слипшийся стек. */
  paint-order: stroke;
}
.cross {
  stroke: var(--line-2);
  stroke-width: 1;
  stroke-dasharray: 3 3;
}
.axis {
  fill: var(--faint);
  font-size: 10px;
}
/* Нулевая ось зеркального графика — заметнее сетки, но не кричит. */
.axis-mid {
  stroke: var(--line-2);
  stroke-width: 1;
}
.tip {
  position: absolute;
  top: 0;
  right: 0;
  background: var(--panel);
  border: 1px solid var(--line-2);
  border-radius: 10px;
  padding: 7px 10px;
  font-size: 12px;
  display: grid;
  gap: 3px;
  pointer-events: none;
  backdrop-filter: blur(8px);
}
.tip b {
  color: var(--ink);
}
.legend {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  align-items: center;
  font-size: 12px;
  color: var(--dim);
}
.sw {
  width: 9px;
  height: 9px;
  border-radius: 3px;
  display: inline-block;
  margin-right: 5px;
  vertical-align: -1px;
}
.num {
  color: var(--ink);
  font-variant-numeric: tabular-nums;
}
/* Второе число в легенде скорости — объём за период, приглушённо. */
.sub-num {
  color: var(--faint);
  font-variant-numeric: tabular-nums;
}
.tbl {
  margin-left: auto;
  border: 1px solid var(--line-2);
  background: transparent;
  color: var(--dim);
  border-radius: 999px;
  padding: 4px 10px;
  font: inherit;
  font-size: 11.5px;
  cursor: pointer;
}
.empty {
  margin: 0;
  padding: 24px 0;
  text-align: center;
  color: var(--faint);
  font-size: 12.5px;
}
.data {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
  font-variant-numeric: tabular-nums;
}
.data th,
.data td {
  text-align: right;
  padding: 4px 6px;
  border-bottom: 1px solid var(--line);
}
.data th:first-child,
.data td:first-child {
  text-align: left;
}
.data th {
  color: var(--faint);
  font-weight: 600;
}
</style>
