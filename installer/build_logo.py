# Uso: python installer/build_logo.py <carpeta>  (después copiar icon.ico a installer/ e icon.png y logo.png a crates/app/assets/images/)
# Reconstruye el logo "SV" (dorado + cian) como polígonos limpios y genera:
#   logo.svg            vector (para guardar y reusar)
#   icon.ico            ícono de Windows con cada tamaño dibujado por separado
#   icon.png            256 px, fondo redondeado (bandeja / avisos)
#   logo.png            256 px, fondo cuadrado lleno (la app redondea las esquinas)
#   preview.png         hoja de prueba con todos los tamaños
import sys
from PIL import Image, ImageDraw

OUT = sys.argv[1]
GOLD = (0xD6, 0xB3, 0x6A)
CYAN = (0x1F, 0xD6, 0xD0)
BG = (0x0F, 0x14, 0x1B)

# Coordenadas tomadas de la imagen generada (lienzo de 1024), con los bordes horizontales igualados
# y sin los biseles de ruido del trazado.
S_SHAPE = [
    (249, 287), (478, 287), (532, 388), (501, 451), (438, 347), (334, 347),
    (600, 601), (520, 753), (270, 520), (352, 520), (502, 659), (527, 616), (249, 351),
]
RIGHT_BAR = [(710, 287), (772, 287), (620, 567), (578, 526)]
CYAN_BAR = [(594, 287), (662, 287), (547, 497), (503, 456)]
SHAPES = [(S_SHAPE, GOLD), (RIGHT_BAR, GOLD), (CYAN_BAR, CYAN)]

xs = [x for s, _ in SHAPES for x, _ in s]
ys = [y for s, _ in SHAPES for _, y in s]
BX0, BX1, BY0, BY1 = min(xs), max(xs), min(ys), max(ys)
BW, BH = BX1 - BX0, BY1 - BY0


def render(size, emblem_frac, rounded, radius_frac=0.22, ss=8):
    """Dibuja el ícono a `size` px. emblem_frac: ancho del emblema respecto al lienzo."""
    big = size * ss
    im = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    if rounded:
        d.rounded_rectangle([0, 0, big - 1, big - 1], radius=int(big * radius_frac), fill=BG + (255,))
    else:
        d.rectangle([0, 0, big, big], fill=BG + (255,))
    scale = big * emblem_frac / BW
    ox = (big - BW * scale) / 2 - BX0 * scale
    oy = (big - BH * scale) / 2 - BY0 * scale
    for pts, color in SHAPES:
        d.polygon([(ox + x * scale, oy + y * scale) for x, y in pts], fill=color + (255,))
    return im.resize((size, size), Image.LANCZOS)


def emblem_frac(size):
    # En tamaños chicos el emblema ocupa más, para que los trazos no queden de menos de 1 px.
    if size <= 20:
        return 0.80
    if size <= 32:
        return 0.74
    if size <= 48:
        return 0.68
    return 0.62


ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256]
frames = [render(s, emblem_frac(s), rounded=True) for s in ICO_SIZES]
frames[-1].save(f"{OUT}/icon.ico", format="ICO", sizes=[(s, s) for s in ICO_SIZES], append_images=frames[:-1])
render(256, 0.62, rounded=True).save(f"{OUT}/icon.png")
render(256, 0.62, rounded=False).save(f"{OUT}/logo.png")
render(1024, 0.62, rounded=True).save(f"{OUT}/logo-1024.png")

# Vector.
def svg_poly(pts, color):
    p = " ".join(f"{x},{y}" for x, y in pts)
    return f'<polygon points="{p}" fill="#{color[0]:02X}{color[1]:02X}{color[2]:02X}"/>'
cx, cy = BX0 + BW / 2, BY0 + BH / 2
s = 1024 * 0.62 / BW
body = "".join(svg_poly(p, c) for p, c in SHAPES)
svg = (
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">'
    f'<rect width="1024" height="1024" rx="225" fill="#{BG[0]:02X}{BG[1]:02X}{BG[2]:02X}"/>'
    f'<g transform="translate(512 512) scale({s:.5f}) translate({-cx} {-cy})">{body}</g></svg>'
)
open(f"{OUT}/logo.svg", "w", encoding="utf-8").write(svg)

# Hoja de prueba: cada tamaño real, sobre fondo claro y oscuro, más una ampliación ×4 de los chicos.
sheet = Image.new("RGBA", (1100, 420), (40, 44, 52, 255))
x = 10
for f in frames:
    sheet.paste(f, (x, 10), f)
    x += f.width + 12
light = Image.new("RGBA", (1100, 140), (235, 235, 235, 255))
x = 10
for f in frames[:6]:
    light.paste(f, (x, 10), f)
    x += f.width + 12
sheet.paste(light, (0, 280))
x = 10
for f in frames[:4]:
    z = f.resize((f.width * 4, f.height * 4), Image.NEAREST)
    sheet.paste(z, (x, 280 + 60), z) if 280 + 60 + z.height <= 420 else None
    x += z.width + 12
sheet.save(f"{OUT}/preview.png")
print("ok", [f.size for f in frames])
