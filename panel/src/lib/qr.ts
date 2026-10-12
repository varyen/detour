/* Чтение QR-кода профиля: с камеры и с картинки.

   Декодер — встроенный BarcodeDetector, где он есть (Chrome и WebView на
   Android, macOS), иначе jsQR. jsQR грузится отдельным чанком только при
   первом сканировании: панель роутера живёт на флеше, а большинство людей
   сканер не откроет ни разу.

   Камера (getUserMedia) работает только в защищённом контексте: HTTPS,
   localhost и окна приложения. Панель роутера по http://192.168.x.x её не
   получит — тогда остаётся картинка: на телефоне выбор файла сам предлагает
   снять фото камерой. */

type Detector = { detect(src: CanvasImageSource): Promise<{ rawValue: string }[]> };
type DetectorCtor = new (opts: { formats: string[] }) => Detector;

let nativeDetector: Detector | null | undefined;
type JsQr = (d: Uint8ClampedArray, w: number, h: number, o?: object) => { data: string } | null;
let jsqr: JsQr | null = null;

async function native(): Promise<Detector | null> {
  if (nativeDetector !== undefined) return nativeDetector;
  nativeDetector = null;
  const Ctor = (globalThis as { BarcodeDetector?: DetectorCtor & { getSupportedFormats?: () => Promise<string[]> } })
    .BarcodeDetector;
  if (!Ctor) return null;
  try {
    const formats = (await Ctor.getSupportedFormats?.()) ?? ["qr_code"];
    if (formats.includes("qr_code")) nativeDetector = new Ctor({ formats: ["qr_code"] });
  } catch {
    nativeDetector = null;
  }
  return nativeDetector;
}

async function loadJsqr() {
  if (!jsqr) jsqr = (await import("jsqr")).default as unknown as JsQr;
  return jsqr;
}

export function cameraSupported(): boolean {
  return !!(globalThis.isSecureContext && navigator.mediaDevices?.getUserMedia);
}

/** Текст QR-кода в кадре или null. Источник — видео, картинка или холст. */
export async function decodeFrame(
  src: HTMLVideoElement | HTMLImageElement | HTMLCanvasElement | ImageBitmap,
  canvas?: HTMLCanvasElement,
): Promise<string | null> {
  const det = await native();
  if (det) {
    try {
      const r = await det.detect(src);
      if (r[0]?.rawValue) return r[0].rawValue;
    } catch {
      /* BarcodeDetector бывает, а движка под ним нет — дальше jsQR */
    }
  }
  const w = "videoWidth" in src ? src.videoWidth : src.width;
  const h = "videoHeight" in src ? src.videoHeight : src.height;
  if (!w || !h) return null;
  /* Большие фото jsQR разбирает долго и хуже — 1280 по длинной стороне хватает. */
  const k = Math.min(1, 1280 / Math.max(w, h));
  const cw = Math.round(w * k);
  const ch = Math.round(h * k);
  const c = canvas ?? document.createElement("canvas");
  c.width = cw;
  c.height = ch;
  const ctx = c.getContext("2d", { willReadFrequently: true });
  if (!ctx) return null;
  ctx.drawImage(src, 0, 0, cw, ch);
  const img = ctx.getImageData(0, 0, cw, ch);
  const dec = await loadJsqr();
  const r = dec(img.data, cw, ch, { inversionAttempts: "attemptBoth" });
  return r?.data || null;
}

/** QR с картинки (файл, фото с камеры, вставка из буфера). */
export async function decodeFile(file: Blob): Promise<string | null> {
  let bmp: ImageBitmap | HTMLImageElement;
  try {
    bmp = await createImageBitmap(file);
  } catch {
    const url = URL.createObjectURL(file);
    try {
      bmp = await new Promise<HTMLImageElement>((ok, bad) => {
        const i = new Image();
        i.onload = () => ok(i);
        i.onerror = () => bad(new Error("не картинка"));
        i.src = url;
      });
    } catch {
      return null;
    } finally {
      URL.revokeObjectURL(url);
    }
  }
  return decodeFrame(bmp);
}
