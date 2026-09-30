"""Иконки Android и iOS из знака Detour (тот же, что в landing/favicon.svg и шапке панели).

Запуск: PYTHONUTF8=1 python client/scripts/gen-icons.py
Пишет client/app/icons/{android,ios} и синхронизирует res/ в gen/android.
Десктопные icon.png/.ico/.icns остаются как есть — в них уже тот же знак.
"""
import os
import re
import shutil
from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
ICONS = os.path.normpath(os.path.join(HERE, "..", "app", "icons"))
GEN_RES = os.path.normpath(os.path.join(HERE, "..", "app", "gen", "android", "app", "src", "main", "res"))

SS = 4
C0, C1 = (0x1f, 0x6f, 0xeb), (0x2e, 0xa0, 0x43)


def bez(p0, p1, p2, p3, n=60):
    out = []
    for i in range(n + 1):
        t = i / n
        u = 1 - t
        out.append((
            u**3 * p0[0] + 3 * u * u * t * p1[0] + 3 * u * t * t * p2[0] + t**3 * p3[0],
            u**3 * p0[1] + 3 * u * u * t * p1[1] + 3 * u * t * t * p2[1] + t**3 * p3[1],
        ))
    return out


ROUTE = (
    [(4.5, 21), (10, 21)]
    + bez((10, 21), (14, 21), (14, 9), (16.5, 9))
    + bez((16.5, 9), (19, 9), (19, 21), (23, 21))
    + [(25, 21)]
)
ARROW = [(22, 17.8), (26.2, 21), (22, 24.2)]


def gradient(size):
    g = Image.new("RGB", (size, size))
    px = g.load()
    for y in range(size):
        for x in range(size):
            t = (x + y) / (2 * (size - 1))
            px[x, y] = tuple(round(C0[i] + (C1[i] - C0[i]) * t) for i in range(3))
    return g


def mark_layer(size, scale, cx=16.0, cy=16.0):
    """Белый маршрут и красная точка на прозрачном фоне. scale — px на единицу 32-сетки."""
    big = size * SS
    k = scale * SS
    im = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    ox, oy = big / 2 - cx * k, big / 2 - cy * k

    def pt(p):
        return (ox + p[0] * k, oy + p[1] * k)

    def stroke(points, w, fill):
        r = w * k / 2
        pts = [pt(p) for p in points]
        d.line(pts, fill=fill, width=round(w * k), joint="curve")
        for x, y in pts:
            d.ellipse((x - r, y - r, x + r, y + r), fill=fill)

    white = (255, 255, 255, 255)
    stroke(ROUTE, 3, white)
    stroke(ARROW, 3, white)
    x, y = pt((16.5, 21))
    for r, col in ((3.2, (0x0f, 0x14, 0x19, 255)), (2.2, (0xf8, 0x51, 0x49, 255))):
        d.ellipse((x - r * k, y - r * k, x + r * k, y + r * k), fill=col)
    return im.resize((size, size), Image.LANCZOS)


def badge(size, scale_units=32, rounded=False, circle=False):
    bg = gradient(size).convert("RGBA")
    bg.alpha_composite(mark_layer(size, size / scale_units))
    if rounded or circle:
        big = size * SS
        m = Image.new("L", (big, big), 0)
        d = ImageDraw.Draw(m)
        if circle:
            d.ellipse((0, 0, big - 1, big - 1), fill=255)
        else:
            d.rounded_rectangle((0, 0, big - 1, big - 1), radius=big * 7 / 32, fill=255)
        bg.putalpha(m.resize((size, size), Image.LANCZOS))
    return bg


def ios():
    master = badge(1024).convert("RGB")
    d = os.path.join(ICONS, "ios")
    for name in os.listdir(d):
        m = re.match(r"AppIcon-(\d+(?:\.\d+)?)(?:x\1)?@(\d)x", name)
        if not m:
            continue
        px = round(float(m.group(1)) * int(m.group(2)))
        master.resize((px, px), Image.LANCZOS).save(os.path.join(d, name))


def android():
    d = os.path.join(ICONS, "android")
    legacy = {"mdpi": 48, "hdpi": 72, "xhdpi": 96, "xxhdpi": 144, "xxxhdpi": 192}
    for dens, px in legacy.items():
        out = os.path.join(d, f"mipmap-{dens}")
        badge(px * 2, rounded=True).resize((px, px), Image.LANCZOS).save(os.path.join(out, "ic_launcher.png"))
        badge(px * 2, circle=True).resize((px, px), Image.LANCZOS).save(os.path.join(out, "ic_launcher_round.png"))
        fg = round(px * 108 / 48)
        # 108dp-холст, безопасная зона 66dp: ширина знака (~24 ед.) — около половины холста
        mark_layer(fg, fg * 0.62 / 32, cx=15.35, cy=16.0).save(os.path.join(out, "ic_launcher_foreground.png"))
    os.makedirs(os.path.join(d, "drawable"), exist_ok=True)
    with open(os.path.join(d, "drawable", "ic_launcher_background.xml"), "w", encoding="utf-8", newline="\n") as f:
        f.write(
            '<?xml version="1.0" encoding="utf-8"?>\n'
            '<vector xmlns:android="http://schemas.android.com/apk/res/android"\n'
            '    xmlns:aapt="http://schemas.android.com/aapt"\n'
            '    android:width="108dp" android:height="108dp"\n'
            '    android:viewportWidth="108" android:viewportHeight="108">\n'
            '    <path android:pathData="M0,0h108v108h-108z">\n'
            '        <aapt:attr name="android:fillColor">\n'
            '            <gradient android:startX="0" android:startY="0" android:endX="108" android:endY="108"\n'
            '                android:startColor="#1f6feb" android:endColor="#2ea043"/>\n'
            '        </aapt:attr>\n'
            '    </path>\n'
            '</vector>\n'
        )
    with open(os.path.join(d, "mipmap-anydpi-v26", "ic_launcher.xml"), "w", encoding="utf-8", newline="\n") as f:
        f.write(
            '<?xml version="1.0" encoding="utf-8"?>\n'
            '<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">\n'
            '  <foreground android:drawable="@mipmap/ic_launcher_foreground"/>\n'
            '  <background android:drawable="@drawable/ic_launcher_background"/>\n'
            '</adaptive-icon>\n'
        )
    bgxml = os.path.join(d, "values", "ic_launcher_background.xml")
    if os.path.exists(bgxml):
        os.remove(bgxml)


def sync_gen():
    if not os.path.isdir(GEN_RES):
        return
    src = os.path.join(ICONS, "android")
    for dens in ("mdpi", "hdpi", "xhdpi", "xxhdpi", "xxxhdpi"):
        shutil.copytree(os.path.join(src, f"mipmap-{dens}"), os.path.join(GEN_RES, f"mipmap-{dens}"), dirs_exist_ok=True)
    for sub in ("mipmap-anydpi-v26", "drawable"):
        shutil.copytree(os.path.join(src, sub), os.path.join(GEN_RES, sub), dirs_exist_ok=True)
    for stale in (os.path.join(GEN_RES, "drawable-v24", "ic_launcher_foreground.xml"),
                  os.path.join(GEN_RES, "values", "ic_launcher_background.xml")):
        if os.path.exists(stale):
            os.remove(stale)


if __name__ == "__main__":
    ios()
    android()
    sync_gen()
    print("ok")
