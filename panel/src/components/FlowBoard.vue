<script setup lang="ts">
/* Схема потока — главный экран панели: видно, куда трафик уходит прямо сейчас.
   На широком экране это ленты с бегущими пакетами (SMIL: анимация живёт в
   разметке и не крутит JS-таймеры), на телефоне — те же три ленты столбиком с
   полосами долей: тянуть 900 px схемы на 375 px бессмысленно. */
import { computed, ref } from "vue";
import { useElementWidth } from "../composables/useElementWidth";

const props = defineProps<{
  /** Доли лент 0..100. */
  direct: number;
  vpn: number;
  bypass: number;
  /** true — точные счётчики файрвола, false — оценка по соединениям. */
  exact: boolean;
  /** false — счётчиков ещё нет; доли не выдумываем, показываем прочерк. */
  hasShares: boolean;
  clients: number;
  profileLabel: string;
  bypassLabel: string;
  /** Уже готовая подпись области: «214 доменов» или «всё, кроме белого списка». */
  vpnScope: string;
  bypassScope: string;
  connections?: number;
  speed?: string;
  externalIp?: string;
  vpnUp: boolean;
}>();

const reduced =
  typeof matchMedia === "function" &&
  matchMedia("(prefers-reduced-motion: reduce)").matches;

/* Пока счётчиков нет — прочерк. Выдуманная доля на схеме потока хуже, чем
   честное «неизвестно»: по ней принимают решения. */
const pct = (n: number) => (props.hasShares ? `${Math.round(n)}%` : "—");
const barWidth = (n: number) => (props.hasShares ? `${Math.round(n)}%` : "0%");

/* Схема РАСТЯГИВАЕТСЯ, но не МАСШТАБИРУЕТСЯ. Раньше viewBox был фиксированный
   (960×200) при width:100%, и на широком экране браузер увеличивал схему
   целиком: на 1728 px подписи узлов вырастали до 24 px против 13 px во всём
   остальном интерфейсе — та самая «непропорциональность». Теперь единица
   viewBox равна пикселю (ширину меряет ResizeObserver), узлы и подписи стоят в
   своём размере, а лишнюю ширину забирают ленты — им длина только на пользу. */
const flow = ref<SVGSVGElement | null>(null);
const W = useElementWidth(flow, 960);

/* Правая колонка подписей: не уже прежних 248 px (в них влезает штатное
   «100% · всё, кроме белого списка») и не шире 420 — дальше строка отрывается
   от своей ленты. Ленты упираются в неё, отступив 12 px. */
const labelW = computed(() => Math.min(420, Math.max(248, Math.round(W.value * 0.3))));
const labelX = computed(() => Math.max(560, W.value - labelW.value));
const laneEnd = computed(() => labelX.value - 12);

/* SVG-текст не переносится и не обрезается по многоточию — он просто уезжает за
   viewBox и молча срезается. Поэтому длинные имена (цепочка это «A → B → C»)
   режем сами по ширине колонки: бюджет считан по замеру в браузере, худший
   случай ≈8.6 px/символ у sans 13px (кириллица) и ≈6.8 у моно 9px. На широком
   экране колонка больше — и режется меньше. */
const clip = (s: string, max: number) =>
  s.length > max ? `${s.slice(0, max - 1).trimEnd()}…` : s;
const bigLine = (s: string) => clip(s, Math.floor(labelW.value / 8.6));
const monoLine = (s: string) => clip(s, Math.floor(labelW.value / 6.8));

/* Скорость точек привязана к доле ленты: пустая лента не должна выглядеть
   такой же оживлённой, как основная. */
const dur = (share: number) => `${(4.2 - Math.min(share, 100) / 100 * 2.4).toFixed(2)}s`;

const lanes = computed(() => [
  {
    key: "direct",
    label: "Напрямую",
    detail: "мимо туннеля",
    share: props.direct,
    color: "var(--lane-direct)",
  },
  {
    key: "vpn",
    label: props.vpnUp ? `Через VPN → ${props.profileLabel}` : "VPN не запущен",
    detail: props.vpnScope,
    share: props.vpn,
    color: "var(--lane-vpn)",
  },
  {
    key: "bypass",
    label: `Обход DPI → ${props.bypassLabel}`,
    detail: props.bypassScope,
    share: props.bypass,
    color: "var(--lane-bypass)",
  },
]);
</script>

<template>
  <section class="board">
    <div class="head">
      <span class="eyebrow">Поток трафика</span>
      <span class="live">
        <i class="dot" :class="{ pulse: vpnUp }"></i>
        <template v-if="speed">{{ speed }}</template>
        <template v-if="connections"> · {{ connections }} соединений</template>
      </span>
    </div>

    <!-- Широкий экран: ленты со схемой -->
    <svg
      ref="flow"
      class="flow"
      :viewBox="`0 0 ${W} 200`"
      role="img"
      :aria-label="`Трафик расходится на три ленты: напрямую ${pct(direct)}, через VPN ${pct(vpn)}, обход DPI ${pct(bypass)}`"
    >
      <rect class="node" x="6" y="76" width="118" height="48" rx="10" />
      <text x="65" y="95" text-anchor="middle" class="big">{{ clients }} устройств</text>
      <text x="65" y="111" text-anchor="middle">в локальной сети</text>

      <path class="lane" d="M126 100 L 146 100" />
      <!-- Ширина узла подобрана под самую длинную подпись («dnsmasq · ipset ·
           nat» — ~128 px моно 9px с letter-spacing): SVG-текст не переносится и
           не обрезается, он просто вылезал за рамку. -->
      <rect class="node hot" x="146" y="70" width="150" height="60" rx="12" />
      <text x="221" y="94" text-anchor="middle" class="big">Detour</text>
      <text x="221" y="112" text-anchor="middle">dnsmasq · ipset · nat</text>

      <!-- Изгиб ленты — фигура фиксированного размера (298→480), лишнюю ширину
           забирает прямой участок: растягивать сам изгиб значило бы менять
           рисунок вслед за окном. -->
      <path id="lane-d" class="lane" :d="`M298 100 C 372 100, 392 44, 480 44 L ${laneEnd} 44`" />
      <path
        id="lane-v"
        class="lane"
        :class="{ hot: vpnUp }"
        :d="`M298 100 L ${laneEnd} 100`"
      />
      <path id="lane-z" class="lane" :d="`M298 100 C 372 100, 392 156, 480 156 L ${laneEnd} 156`" />

      <!-- Ключ включает ширину: animateMotion берёт траекторию у mpath один
           раз, и после ресайза пакеты бежали бы по старой, укороченной ленте.
           Ресайз редок — перемонтировать три кружка дешевле, чем следить за
           этим руками. -->
      <g v-if="!reduced" :key="W">
        <circle v-for="i in 3" :key="`d${i}`" class="pkt d" r="2.6">
          <animateMotion :dur="dur(direct)" :begin="`${(i - 1) * 1.05}s`" repeatCount="indefinite">
            <mpath href="#lane-d" />
          </animateMotion>
        </circle>
        <template v-if="vpnUp">
          <circle v-for="i in 3" :key="`v${i}`" class="pkt v" r="3.1">
            <animateMotion :dur="dur(vpn)" :begin="`${(i - 1) * 0.73}s`" repeatCount="indefinite">
              <mpath href="#lane-v" />
            </animateMotion>
          </circle>
        </template>
        <circle v-if="bypass > 0" class="pkt z" r="2.4">
          <animateMotion :dur="dur(bypass)" repeatCount="indefinite">
            <mpath href="#lane-z" />
          </animateMotion>
        </circle>
      </g>

      <text :x="labelX" y="40" class="big">Напрямую</text>
      <text :x="labelX" y="56">{{ pct(direct) }} трафика</text>
      <text :x="labelX" y="96" class="big" :class="{ accent: vpnUp }">
        {{ bigLine(vpnUp ? `Через VPN → ${profileLabel}` : "VPN не запущен") }}
        <title>{{ vpnUp ? `Через VPN → ${profileLabel}` : "VPN не запущен" }}</title>
      </text>
      <text :x="labelX" y="112">
        {{ monoLine(`${pct(vpn)} · ${vpnScope}`) }}
        <title>{{ pct(vpn) }} · {{ vpnScope }}</title>
      </text>
      <text :x="labelX" y="152" class="big">
        {{ bigLine(`Обход DPI → ${bypassLabel}`) }}
        <title>Обход DPI → {{ bypassLabel }}</title>
      </text>
      <text :x="labelX" y="168">
        {{ monoLine(`${pct(bypass)} · ${bypassScope}`) }}
        <title>{{ pct(bypass) }} · {{ bypassScope }}</title>
      </text>
    </svg>

    <!-- Телефон: те же ленты столбиком -->
    <ul class="stack">
      <li v-for="l in lanes" :key="l.key">
        <div class="lrow">
          <span class="lname">{{ l.label }}</span>
          <b class="num">{{ pct(l.share) }}</b>
        </div>
        <div class="bar"><i :style="{ width: barWidth(l.share), background: l.color }"></i></div>
        <small>{{ l.detail }}</small>
      </li>
    </ul>

    <div class="legend">
      <div><i class="sw" style="background: var(--lane-direct)"></i> Напрямую <b class="num">{{ pct(direct) }}</b></div>
      <div><i class="sw" style="background: var(--lane-vpn)"></i> Через VPN <b class="num">{{ pct(vpn) }}</b></div>
      <div><i class="sw" style="background: var(--lane-bypass)"></i> Обход DPI <b class="num">{{ pct(bypass) }}</b></div>
      <div class="ip">
        <span class="eyebrow">{{
          !hasShares ? "счётчики не включены" : exact ? "по счётчикам" : "оценка"
        }}</span>
        <span v-if="externalIp" class="mono num">{{ externalIp }}</span>
      </div>
    </div>
  </section>
</template>

<style scoped>
.board {
  border: 1px solid var(--line);
  border-radius: var(--radius);
  background: var(--panel);
  backdrop-filter: blur(10px);
  padding: 16px 18px 8px;
  margin-bottom: 14px;
}
.head {
  display: flex;
  align-items: baseline;
  gap: 12px;
  flex-wrap: wrap;
}
.live {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  font-size: 12px;
  color: var(--dim);
  margin-left: auto;
}
.dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--faint);
}
.dot.pulse {
  background: var(--ok);
  animation: pulse 2.4s ease-out infinite;
}
@keyframes pulse {
  0% {
    box-shadow: 0 0 0 0 color-mix(in srgb, var(--ok) 55%, transparent);
  }
  70% {
    box-shadow: 0 0 0 7px transparent;
  }
}

.flow {
  width: 100%;
  /* Высота — в пикселях, ровно как высота viewBox: с `height:auto` браузер
     считал бы её из соотношения сторон, и на кадре сразу после ресайза (viewBox
     ещё старый) схема прыгала бы по вертикали. */
  height: 200px;
  display: block;
  margin-top: 4px;
}
.flow .lane {
  fill: none;
  stroke: var(--line-2);
  stroke-width: 1.4;
}
.flow .lane.hot {
  stroke: var(--accent);
  stroke-width: 2;
}
.flow text {
  font-family: var(--mono);
  font-size: 9px;
  letter-spacing: 0.08em;
  fill: var(--dim);
}
.flow text.big {
  font-family: var(--sans);
  font-size: 13px;
  letter-spacing: 0;
  fill: var(--ink);
  font-weight: 600;
}
.flow text.accent {
  fill: var(--accent);
}
.flow .node {
  fill: var(--panel-2);
  stroke: var(--line-2);
}
.flow .node.hot {
  stroke: var(--accent);
}
.flow .pkt.d {
  fill: var(--faint);
}
.flow .pkt.v {
  fill: var(--accent);
}
.flow .pkt.z {
  fill: var(--warn);
}

.stack {
  display: none;
  list-style: none;
  margin: 10px 0 4px;
  padding: 0;
  gap: 14px;
  flex-direction: column;
}
.lrow {
  display: flex;
  align-items: baseline;
  gap: 10px;
  font-size: 14px;
}
.lname {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.lrow b {
  margin-left: auto;
}
.stack small {
  font-size: 12px;
  color: var(--faint);
}
.bar {
  height: 6px;
  border-radius: 3px;
  background: var(--panel-2);
  border: 1px solid var(--line);
  overflow: hidden;
  margin: 5px 0 4px;
}
.bar i {
  display: block;
  height: 100%;
  transition: width 0.6s ease;
}

.legend {
  display: flex;
  gap: 18px;
  flex-wrap: wrap;
  border-top: 1px solid var(--line);
  margin-top: 6px;
  padding: 10px 2px 8px;
}
.legend div {
  display: flex;
  align-items: baseline;
  gap: 8px;
  font-size: 12.5px;
  color: var(--dim);
}
.legend b {
  font-size: 15px;
  color: var(--ink);
}
.sw {
  width: 8px;
  height: 8px;
  border-radius: 2px;
  align-self: center;
}
.ip {
  margin-left: auto;
}

@media (max-width: 860px) {
  .board {
    padding: 14px 14px 6px;
  }
  .flow {
    display: none;
  }
  .stack {
    display: flex;
  }
  .legend {
    display: none;
  }
}
</style>
