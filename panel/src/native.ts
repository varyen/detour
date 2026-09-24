/* Мост к Android-оболочке приложения (`DetourInsets` в MainActivity). В
   браузере и на десктопе его нет — всё здесь молча ничего не делает. */

interface Insets {
  top: number;
  bottom: number;
  left: number;
  right: number;
}

interface NativeBridge {
  insets(): string;
  theme(dark: boolean): void;
}

const bridge = (): NativeBridge | undefined =>
  (window as unknown as { DetourInsets?: NativeBridge }).DetourInsets;

function applyInsets(i: Insets) {
  const s = document.documentElement.style;
  for (const k of ["top", "bottom", "left", "right"] as const) {
    s.setProperty(`--sa-${k}`, `${Math.max(0, i[k] || 0)}px`);
  }
}

/* Отступы системных полос: оболочка отдаёт их при старте и присылает заново,
   когда они меняются (поворот, скрытый навбар). */
export function installNativeBridge() {
  const b = bridge();
  if (!b) return;
  (window as unknown as { __detourInsets?: (i: Insets) => void }).__detourInsets = applyInsets;
  try {
    applyInsets(JSON.parse(b.insets()) as Insets);
  } catch {
    /* старая оболочка — остаёмся на env() */
  }
}

/* Иконки статусбара — тёмные на светлой теме и светлые на тёмной. */
export function nativeTheme(dark: boolean) {
  try {
    bridge()?.theme(dark);
  } catch {
    /* нет моста */
  }
}
