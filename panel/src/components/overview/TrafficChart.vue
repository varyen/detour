<script setup lang="ts">
/* График трафика по трём лентам.
 *
 * ФОРМА. Данные отвечают на вопрос «как менялся объём и чья это была доля» —
 * это изменение во времени плюс состав целого, то есть стековые площади. Не
 * три отдельные линии: они отвечают на другой вопрос (сравнить ленты между
 * собой), а нам важна и сумма тоже.
 *
 * ЦВЕТ. Ленты красятся токенами --lane-*, теми же, что и схема потока выше:
 * одна сущность — один цвет на всём экране. Шаги валидированы скриптом из
 * гайда по визуализации в обеих схемах; в светлой теме два оттенка не
 * добирают контраста 3:1 к подложке, поэтому обязательны видимые подписи и
 * табличный вид — они здесь есть, это не украшение.
 *
 * ЧЕГО ЗДЕСЬ НЕТ НАМЕРЕННО. Второй оси (две шкалы на одном поле врут о
 * соотношении) и числа над каждой точкой — подписываются только концы лент.
 */
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { overview } from "../../api";
import { fmtBytes } from "../../lib/format";

type Point = { ts: number; direct: number; vpn: number; bypass: number };
type Range = "minute" | "hour";

const LANES = [
  { key: "direct", label: "Напрямую", color: "var(--lane-direct)" },
  { key: "vpn", label: "Через VPN", color: "var(--lane-vpn)" },
  { key: "bypass", label: "Обход DPI", color: "var(--lane-bypass)" },
] as const;

const range = ref<Range>("minute");
const points = ref<Point[]>([]);
const supported = ref(true);
const loading = ref(false);
const showTable = ref(false);
const hover = ref<number | null>(null);

/* Геометрия в единицах viewBox: SVG тянется по ширине, поэтому пиксельных
   размеров здесь нет вовсе. */
const W = 720;
const H = 180;
const PAD_L = 8;
const PAD_R = 8;
const PAD_T = 10;
const PAD_B = 18;

let timer: number | undefined;

async function load() {
  loading.value = true;
  try {
    const r = await overview.trafficSeries(range.value);
    if (r?.supported === false || r?.ok === false) {
      supported.value = false;
      points.value = [];
    } else {
      supported.value = true;
      points.value = Array.isArray(r?.points) ? r.points : [];
    }
  } finally {
    loading.value = false;
  }
}

onMounted(() => {
  void load();
  /* Минутный ряд пополняется раз в минуту — чаще спрашивать нечего. */
  timer = window.setInterval(() => void load(), 60_000);
});
onUnmounted(() => timer && clearInterval(timer));
watch(range, () => void load());

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

/* Пик суммы задаёт шкалу. Ноль в знаменателе означает «трафика не было» —
   рисуем пустое поле, а не делим на ноль. */
const peak = computed(() =>
  points.value.reduce((m, p) => Math.max(m, p.direct + p.vpn + p.bypass), 0),
);

const geom = computed(() => {
  const pts = points.value;
  const n = pts.length;
  if (n === 0 || peak.value <= 0) return null;
  const x = (i: number) => PAD_L + (i * (W - PAD_L - PAD_R)) / Math.max(1, n - 1);
  const y = (v: number) => PAD_T + (H - PAD_T - PAD_B) * (1 - v / peak.value);

  /* Стек снизу вверх: напрямую → VPN → обход. Каждая лента — замкнутая
     область между своей верхней границей и границей предыдущей.
     Лента, у которой за всё окно ноль байт, НЕ рисуется: её площадь нулевой
     высоты, но обводка верхней границы легла бы поверх соседней ленты и
     читалась бы как настоящий трафик (жёлтая линия «Обход DPI» через весь
     график при нуле — именно это и было видно на первом же скриншоте).
     В легенде лента остаётся — с честным нулём. */
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

const hoverPoint = computed(() => {
  const i = hover.value;
  if (i === null || !points.value[i]) return null;
  const p = points.value[i];
  return { i, p, total: p.direct + p.vpn + p.bypass };
});

function onMove(e: MouseEvent) {
  const g = geom.value;
  if (!g) return;
  const box = (e.currentTarget as SVGElement).getBoundingClientRect();
  const rel = ((e.clientX - box.left) / box.width) * W;
  const step = (W - PAD_L - PAD_R) / Math.max(1, g.n - 1);
  const i = Math.round((rel - PAD_L) / step);
  hover.value = Math.min(g.n - 1, Math.max(0, i));
}

function fmtTime(ts: number) {
  const d = new Date(ts * 1000);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  if (range.value === "hour") {
    return `${String(d.getDate()).padStart(2, "0")}.${String(d.getMonth() + 1).padStart(2, "0")} ${hh}:00`;
  }
  return `${hh}:${mm}`;
}

const axisLabels = computed(() => {
  const pts = points.value;
  if (pts.length < 2) return [];
  const idx = [0, Math.floor(pts.length / 2), pts.length - 1];
  return idx.map((i) => ({ i, text: fmtTime(pts[i].ts), x: geom.value?.x(i) ?? 0 }));
});
</script>

<template>
  <section class="chart">
    <header>
      <div>
        <h3>Трафик по лентам</h3>
        <p class="sub">
          <template v-if="grandTotal > 0">
            Всего за период {{ fmtBytes(grandTotal) }}
          </template>
          <template v-else-if="!supported">Сборщик не установлен</template>
          <template v-else-if="loading">Загружаю…</template>
          <template v-else>Данные копятся — точки появляются раз в минуту</template>
        </p>
      </div>
      <!-- Фильтры одной строкой над графиком. -->
      <div class="ranges" role="group" aria-label="Период">
        <button
          type="button"
          :class="{ on: range === 'minute' }"
          :aria-pressed="range === 'minute'"
          @click="range = 'minute'"
        >
          Сутки
        </button>
        <button
          type="button"
          :class="{ on: range === 'hour' }"
          :aria-pressed="range === 'hour'"
          @click="range = 'hour'"
        >
          Месяц
        </button>
      </div>
    </header>

    <div v-if="geom" class="plot">
      <svg
        :viewBox="`0 0 ${W} ${H}`"
        preserveAspectRatio="none"
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
          <!-- Зазор в 2px между заливками: границы лент не слипаются. -->
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
          :text-anchor="l.i === 0 ? 'start' : l.i === points.length - 1 ? 'end' : 'middle'"
        >
          {{ l.text }}
        </text>
      </svg>

      <div v-if="hoverPoint" class="tip">
        <b>{{ fmtTime(hoverPoint.p.ts) }}</b>
        <span v-for="lane in LANES" :key="lane.key">
          <i class="sw" :style="{ background: lane.color }"></i>
          {{ lane.label }} <b>{{ fmtBytes(hoverPoint.p[lane.key]) }}</b>
        </span>
      </div>
    </div>

    <p v-else class="empty">
      {{ supported ? "Пока нечего показать" : "Нет detour-trafficlog" }}
    </p>

    <!-- Легенда обязательна: три ленты, идентичность не должна держаться на
         одном цвете. Числа рядом — те самые видимые подписи. -->
    <div class="legend">
      <div v-for="lane in LANES" :key="lane.key">
        <i class="sw" :style="{ background: lane.color }"></i>
        {{ lane.label }}
        <b class="num">{{ fmtBytes(totals[lane.key]) }}</b>
      </div>
      <button type="button" class="tbl" @click="showTable = !showTable">
        {{ showTable ? "Скрыть таблицу" : "Таблицей" }}
      </button>
    </div>

    <!-- Табличный вид: и как доступная альтернатива цвету, и просто чтобы
         посмотреть числа. -->
    <table v-if="showTable && points.length" class="data">
      <thead>
        <tr>
          <th>Время</th>
          <th v-for="lane in LANES" :key="lane.key">{{ lane.label }}</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="p in points.slice(-24).reverse()" :key="p.ts">
          <td>{{ fmtTime(p.ts) }}</td>
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
header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
}
h3 {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
}
.sub {
  margin: 2px 0 0;
  font-size: 12px;
  color: var(--faint);
}
.ranges {
  display: flex;
  gap: 4px;
  flex: none;
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
