<script setup lang="ts">
/* Описание изменений (release notes) в человеческом виде: заголовки, списки,
   цитаты, блоки кода и строчная разметка. Раньше этот текст показывался в
   LogPane моноширинным дампом вместе с решётками и звёздочками — читать список
   правок так почти невозможно.

   Разметка собирается НЕ строками HTML: текст приходит с GitHub, он внешний и
   недоверенный, поэтому v-html здесь нет вообще. Разбор даёт структуру блоков,
   а печатает её Vue интерполяцией — любые теги во входе останутся текстом.

   Поддержан намеренно небольшой поднабор markdown — ровно то, что бывает в
   RELEASE_NOTES: заголовки, `-`/`*`/`1.` списки, `>` цитата, ``` блок кода,
   `---` разделитель, а в строке — код, жирный, курсив и http(s)-ссылка. */
import { computed } from "vue";
import MarkdownInline, { parseInline, type MdSpan } from "./MarkdownInline.vue";

type Block =
  | { k: "head"; level: number; spans: MdSpan[] }
  | { k: "para"; lines: MdSpan[][] }
  | { k: "list"; ordered: boolean; items: MdSpan[][] }
  | { k: "quote"; spans: MdSpan[] }
  | { k: "code"; text: string }
  | { k: "rule" };

const props = defineProps<{ text: string; emptyText?: string }>();

function parse(md: string): Block[] {
  const lines = String(md ?? "")
    .replace(/\r\n?/g, "\n")
    .split("\n");
  const out: Block[] = [];
  let para: MdSpan[][] = [];
  let list: { ordered: boolean; items: MdSpan[][] } | null = null;
  let code: string[] | null = null;

  function flushPara() {
    if (!para.length) return;
    out.push({ k: "para", lines: para });
    para = [];
  }
  function flushList() {
    if (!list) return;
    out.push({ k: "list", ordered: list.ordered, items: list.items });
    list = null;
  }
  function flushCode() {
    if (!code) return;
    out.push({ k: "code", text: code.join("\n") });
    code = null;
  }
  /* Через функцию, а не `list.items[...]` в цикле: список наполняют вложенные
     функции, поэтому в теле цикла TypeScript считает переменную всё ещё null. */
  function lastItem(): MdSpan[] | null {
    const l = list;
    return l && l.items.length ? l.items[l.items.length - 1] : null;
  }
  function pushItem(ordered: boolean, text: string) {
    flushPara();
    if (!list || list.ordered !== ordered) {
      flushList();
      list = { ordered, items: [] };
    }
    list.items.push(parseInline(text));
  }

  for (const raw of lines) {
    if (/^\s*```/.test(raw)) {
      /* Незакрытый блок кода — обычное дело в обрезанном changelog: то, что
         успело прийти, всё равно показываем (закрываем его после цикла). */
      if (code) flushCode();
      else {
        flushPara();
        flushList();
        code = [];
      }
      continue;
    }
    if (code) {
      code.push(raw);
      continue;
    }
    if (!raw.trim()) {
      flushPara();
      flushList();
      continue;
    }

    let m = raw.match(/^\s{0,3}(#{1,6})\s+(.*)$/);
    if (m) {
      flushPara();
      flushList();
      /* h1 из markdown отдаём как h3: у шторки уже есть собственный заголовок,
         и второй h1 на странице сломал бы порядок для скринридера. */
      out.push({
        k: "head",
        level: Math.min(m[1].length + 2, 6),
        spans: parseInline(m[2].replace(/\s+#+\s*$/, "")),
      });
      continue;
    }
    if (/^\s{0,3}(-{3,}|\*{3,}|_{3,})\s*$/.test(raw)) {
      flushPara();
      flushList();
      out.push({ k: "rule" });
      continue;
    }
    m = raw.match(/^\s*[-*+]\s+(.*)$/);
    if (m) {
      pushItem(false, m[1]);
      continue;
    }
    m = raw.match(/^\s*\d+[.)]\s+(.*)$/);
    if (m) {
      pushItem(true, m[1]);
      continue;
    }
    m = raw.match(/^\s*>\s?(.*)$/);
    if (m) {
      flushPara();
      flushList();
      out.push({ k: "quote", spans: parseInline(m[1]) });
      continue;
    }
    /* Продолжение пункта с висячим отступом — дописываем в тот же пункт, а не
       начинаем абзац: иначе длинный пункт разваливается на куски. */
    const li = lastItem();
    if (li) {
      li.push({ k: "text", v: " " });
      li.push(...parseInline(raw.trim()));
      continue;
    }
    para.push(parseInline(raw.trim()));
  }
  flushCode();
  flushPara();
  flushList();
  return out;
}

const blocks = computed(() => parse(props.text));
</script>

<template>
  <p v-if="!blocks.length" class="note">
    {{ emptyText ?? "Описание изменений не пришло." }}
  </p>
  <div v-else class="md">
    <template v-for="(b, i) in blocks" :key="i">
      <component :is="`h${b.level}`" v-if="b.k === 'head'" class="h">
        <MarkdownInline :spans="b.spans" />
      </component>
      <ul v-else-if="b.k === 'list' && !b.ordered">
        <li v-for="(it, j) in b.items" :key="j"><MarkdownInline :spans="it" /></li>
      </ul>
      <ol v-else-if="b.k === 'list'">
        <li v-for="(it, j) in b.items" :key="j"><MarkdownInline :spans="it" /></li>
      </ol>
      <blockquote v-else-if="b.k === 'quote'">
        <MarkdownInline :spans="b.spans" />
      </blockquote>
      <pre v-else-if="b.k === 'code'" class="code" tabindex="0">{{ b.text }}</pre>
      <hr v-else-if="b.k === 'rule'" />
      <p v-else-if="b.k === 'para'">
        <template v-for="(ln, j) in b.lines" :key="j">
          <br v-if="j" /><MarkdownInline :spans="ln" />
        </template>
      </p>
    </template>
  </div>
</template>

<style scoped>
.note {
  font-size: 13.5px;
  color: var(--dim);
  padding: 6px 2px;
}
.md {
  display: flex;
  flex-direction: column;
  gap: 9px;
  min-width: 0;
  font-size: 14px;
  line-height: 1.55;
  color: var(--ink);
  overflow-wrap: anywhere;
}
.h {
  font-weight: 600;
  line-height: 1.3;
  /* Заголовок отбивается от предыдущего блока, но не от начала списка. */
  margin-top: 6px;
}
.md > .h:first-child {
  margin-top: 0;
}
/* Уровни идут от h3: `#` в changelog — это h3 (см. разбор), поэтому именно он
   должен читаться как заголовок раздела, а не как подпись. */
h3.h {
  font-size: 16px;
}
h4.h {
  font-size: 14.5px;
}
h5.h,
h6.h {
  font-size: 13.5px;
}
.md ul,
.md ol {
  display: flex;
  flex-direction: column;
  gap: 5px;
  /* Маркеры внутри — иначе на телефоне они уезжают за левый край шторки. */
  padding-left: 20px;
  min-width: 0;
}
.md ul {
  list-style: disc;
}
.md ol {
  list-style: decimal;
}
.md li {
  min-width: 0;
}
blockquote {
  border-left: 2px solid var(--line-2);
  padding-left: 10px;
  color: var(--dim);
}
hr {
  border: 0;
  border-top: 1px solid var(--line);
  width: 100%;
}
.code {
  margin: 0;
  max-width: 100%;
  overflow: auto;
  -webkit-overflow-scrolling: touch;
  overscroll-behavior: contain;
  white-space: pre;
  font-family: var(--mono);
  font-size: 12.5px;
  line-height: 1.45;
  tab-size: 4;
  background: var(--panel-2);
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  padding: 9px 11px;
}
/* Строчные теги рисует MarkdownInline — из scoped-стилей до них только :deep. */
.md :deep(code) {
  font-family: var(--mono);
  font-size: 12.5px;
  background: var(--panel-2);
  border: 1px solid var(--line);
  border-radius: 5px;
  padding: 1px 4px;
}
.md :deep(strong) {
  font-weight: 600;
}
.md :deep(em) {
  font-style: italic;
}
.md :deep(a) {
  color: var(--accent);
  text-decoration: underline;
}
</style>
