/* UI-модуль плагина «Привет». Панель грузит его через import() и зовёт default
   с объектом host — в нём тот же экземпляр Vue, что у панели, вызов действий
   плагина, тосты и базовые компоненты. Шаблонов тут нет (компилятора шаблонов
   в сборке панели нет) — только render-функции через host.vue.h. */

export default function setup(host) {
  const { h, ref, defineComponent } = host.vue;
  const { UiButton } = host.components;

  const About = defineComponent({
    name: "HelloAbout",
    setup() {
      const reply = ref("");
      const busy = ref(false);

      async function echo() {
        busy.value = true;
        try {
          const r = await host.call("echo", "привет из ESM-модуля");
          reply.value = JSON.stringify(r, null, 2);
        } catch (e) {
          host.toast.error(e instanceof Error ? e.message : String(e));
        } finally {
          busy.value = false;
        }
      }

      return () =>
        h("div", { style: "display:flex;flex-direction:column;gap:10px" }, [
          h(
            "p",
            { style: "margin:0;color:var(--dim);font-size:13.5px" },
            `Эту секцию рисует модуль плагина ${host.plugin.name} ${host.plugin.version}. ` +
              `Приветствие из настроек: «${host.settings.greeting ?? ""}».`,
          ),
          h("div", null, [
            h(UiButton, { busy: busy.value, onClick: echo }, () => "Отправить echo"),
          ]),
          reply.value
            ? h(
                "pre",
                {
                  style:
                    "margin:0;padding:10px;border:1px solid var(--line);border-radius:var(--radius-sm);" +
                    "font-family:var(--mono);font-size:12px;white-space:pre-wrap",
                },
                reply.value,
              )
            : null,
        ]);
    },
  });

  return { components: { About } };
}
