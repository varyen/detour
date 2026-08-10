<script lang="ts">
/* Строчная разметка описания изменений: `код`, **жирный**, *курсив*, ссылка.
   Текст приходит с GitHub, то есть он внешний и недоверенный — поэтому здесь
   нет ни v-html, ни сборки HTML строками: разбираем в структуру, а рисует её
   Vue обычной интерполяцией, куда чужие теги подставить нельзя. */

export type MdSpan =
  | { k: "text"; v: string }
  /* Жирное и курсив держат разобранное содержимое, а не строку: в release notes
     этого проекта сплошь `**жирный заголовок с `кодом` внутри**`, и при плоской
     модели такой код выводился с бэктиками прямо в первой строке описания. */
  | { k: "b"; kids: MdSpan[] }
  | { k: "i"; kids: MdSpan[] }
  | { k: "code"; v: string }
  | { k: "link"; v: string; href: string };

/* Порядок ветвей важен: `**` обязан проверяться раньше `*`, иначе жирное
   станет курсивом со звёздочками по краям.

   У одиночных `*` и `_` есть границы слева и справа (группы 6 и 8 — то, что
   стоит перед открывающим знаком): без них `path_to_file` превращается в
   «path<em>to</em>file», а `2*3*4` — в «2<em>3</em>4». Граница слева
   захватывается в группу, а не проверяется lookbehind'ом: lookbehind не понимают
   Safari до 16.4, а падение разбора регулярки уронило бы всю сборку панели. */
const INLINE_SRC =
  "`([^`\\n]+)`|\\[([^\\]\\n]+)\\]\\(([^)\\s]+)\\)|\\*\\*([^*\\n]+)\\*\\*|__([^_\\n]+)__|(^|[\\s(])\\*([^*\\s][^*\\n]*)\\*(?=$|[\\s.,;:!?)])|(^|[\\s(])_([^_\\s][^_\\n]*)_(?=$|[\\s.,;:!?)])";

/* Глубина ограничена: текст внешний, и на разбор специально вложенной строки не
   стоит тратить больше нескольких проходов. На пределе содержимое остаётся
   простым текстом — хуже вида, но не бесконечный разбор. */
const MAX_DEPTH = 4;

/** Ссылку пускаем только на http(s): javascript:/data: — не ссылка, а атака. */
function safeHref(url: string): string {
  return /^https?:\/\/[^\s]+$/i.test(url) ? url : "";
}

export function parseInline(line: string, depth = 0): MdSpan[] {
  const out: MdSpan[] = [];
  let last = 0;
  /* Регулярка своя на каждый вызов: она с флагом /g, а рекурсия по содержимому
     жирного сбросила бы lastIndex общего экземпляра — внешний разбор продолжился
     бы с чужой позиции. */
  const re = new RegExp(INLINE_SRC, "g");
  /** Содержимое жирного/курсива разбираем тем же разбором — на один уровень глубже. */
  const kids = (s: string): MdSpan[] =>
    depth >= MAX_DEPTH ? [{ k: "text", v: s }] : parseInline(s, depth + 1);
  let m: RegExpExecArray | null;
  while ((m = re.exec(line)) !== null) {
    if (m.index > last) out.push({ k: "text", v: line.slice(last, m.index) });
    last = m.index + m[0].length;
    if (m[1] !== undefined) {
      out.push({ k: "code", v: m[1] });
    } else if (m[2] !== undefined && m[3] !== undefined) {
      const href = safeHref(m[3]);
      /* Схема не разрешена — оставляем исходный markdown текстом: показать
         сырое `[текст](…)` честнее, чем ссылку в никуда. */
      out.push(href ? { k: "link", v: m[2], href } : { k: "text", v: m[0] });
    } else if (m[4] !== undefined) {
      out.push({ k: "b", kids: kids(m[4]) });
    } else if (m[5] !== undefined) {
      out.push({ k: "b", kids: kids(m[5]) });
    } else if (m[7] !== undefined) {
      /* Границу слева забрал сам курсив — возвращаем её текстом. */
      if (m[6]) out.push({ k: "text", v: m[6] });
      out.push({ k: "i", kids: kids(m[7]) });
    } else if (m[9] !== undefined) {
      if (m[8]) out.push({ k: "text", v: m[8] });
      out.push({ k: "i", kids: kids(m[9]) });
    }
    /* Пустое совпадение невозможно (каждая ветвь требует хотя бы один символ),
       так что бесконечного цикла по lastIndex тут быть не может. */
  }
  if (last < line.length) out.push({ k: "text", v: line.slice(last) });
  return out;
}
</script>

<script setup lang="ts">
defineProps<{ spans: MdSpan[] }>();
</script>

<template>
  <!-- Переносы между тегами склеены намеренно: в строчной разметке лишний
       перевод строки превращается в лишний пробел перед знаком препинания. -->
  <template v-for="(s, i) in spans" :key="i"
    ><strong v-if="s.k === 'b'"><MarkdownInline :spans="s.kids" /></strong
    ><em v-else-if="s.k === 'i'"><MarkdownInline :spans="s.kids" /></em
    ><code v-else-if="s.k === 'code'">{{ s.v }}</code
    ><a
      v-else-if="s.k === 'link'"
      :href="s.href"
      target="_blank"
      rel="noopener noreferrer nofollow"
      >{{ s.v }}</a
    ><template v-else>{{ s.v }}</template></template
  >
</template>
