"""Convierte un diseño de registro (DR) oficial de la AEAT (.xls/.xlsx) en JSON.

Uso (en un entorno con openpyxl y xlrd):
    python -I tools/aeat/dr2json.py fixtures/aeat/dr/DR303e26v101.xlsx 303 > fixtures/aeat/dr/303.json

Cada hoja del DR es una página del fichero. Por campo se guarda: posición (1-based), longitud,
tipo (An, A, Num, N), descripción, la casilla [NN] si la hay y la constante si el DR la fija.
La primera hoja (cabecera <T…><AUX>…</AUX>) se guarda igual, aunque su contenido es común.
"""

import hashlib
import json
import re
import sys
from pathlib import Path


def rows(path: Path):
    if path.suffix == ".xlsx":
        import openpyxl

        wb = openpyxl.load_workbook(path, read_only=True, data_only=True)
        for ws in wb.worksheets:
            yield ws.title, [list(r) for r in ws.iter_rows(values_only=True)]
    else:
        import xlrd

        wb = xlrd.open_workbook(str(path))
        for ws in wb.sheets():
            yield ws.name, [[c.value for c in ws.row(i)] for i in range(ws.nrows)]


def num(v):
    if isinstance(v, (int, float)):
        return int(v)
    if isinstance(v, str) and v.strip().isdigit():
        return int(v.strip())
    return None


def text(v):
    return re.sub(r"\s+", " ", str(v)).strip() if v not in (None, "") else ""


CONST = re.compile(r'Constante\s*"([^"]*)"', re.I)
QUOTED = re.compile(r'^"([^"]*)"$')
BOX = re.compile(r"\[\s*(\d+)\s*\]")
TYPES = {"AN": "An", "A": "A", "NUM": "Num", "N": "N"}
# Casillas que el DR no escribe junto a su campo: el del 115 describe el campo 16 como
# «Resultado a ingresar [03] - [04]» (la fórmula, no su número), y en el modelo es la [05].
# Sin esto el campo se quedaría con la [03] del primer corchete, repetida con el campo 14.
BOX_OVERRIDES = {("115", 16, "Resultado a ingresar"): 5}


def field(cells, model):
    n, pos, length = num(cells[0]), num(cells[1]), cells[2]
    kind = TYPES.get(text(cells[3]).upper())
    # Campos sin posición o longitud fijas («***», «Variable»): sólo en la hoja de cabecera, que
    # se genera aparte (envelope en crates/models/src/dr.rs).
    if n is None or kind is None or pos is None or num(length) is None:
        return None
    rest = [text(c) for c in cells[4:] if text(c)]
    # Algunas hojas tienen una columna extra con «C» (complementaria) antes de la descripción.
    if rest and rest[0] == "C":
        rest = rest[1:]
    desc = rest[0] if rest else ""
    tail = " | ".join(rest[1:])
    f = {"n": n, "pos": pos, "len": num(length), "type": kind, "desc": desc}
    if tail:
        f["note"] = tail
    # La casilla es el primer [NN] fuera de paréntesis: «[07] … ([04]-[05]-[06])»,
    # «Total … ( [152] + … ) [27]», «[04] 20 por 100 del importe de la casilla [03]».
    outside = desc
    while True:  # paréntesis anidados: «(clave ([16] x [17]) + [47]) [18]»
        stripped = re.sub(r"\([^()]*\)", " ", outside)
        if stripped == outside:
            break
        outside = stripped
    boxes = BOX.findall(outside)
    if boxes:
        f["box"] = int(boxes[0])
    for (m, k, label), box in BOX_OVERRIDES.items():
        if m == model and k == n and label in desc:
            f["box"] = box
    m = CONST.search(" | ".join(rest))
    if m:
        f["const"] = m.group(1)
    elif len(rest) > 1 and QUOTED.match(rest[-1]) and "Constante" in desc:
        f["const"] = QUOTED.match(rest[-1]).group(1)
    return f


def compactar(out: dict) -> dict:
    """Diseño compacto (el del 200, cuyos originales pesan unos 10 MB): sólo las constantes del
    principio de cada página (su marca), los campos de importe con casilla y el último campo de
    cada página (para su longitud), sin descripciones. Es lo que necesita TaxPain para leer las
    casillas de un fichero ya presentado (`taxpain_models::dr::read_boxes` y `pages_fit`)."""
    pages = []
    for p in out["pages"]:
        fs = p["fields"]
        keep = []
        cabecera = True
        for i, f in enumerate(fs):
            if cabecera and f.get("const") is not None:
                keep.append(f)
                continue
            cabecera = False
            importe = (
                "box" in f
                and f.get("const") is None
                and f["type"] in ("N", "Num")
                and f["len"] >= 13
                and "número" not in f["desc"].lower()
            )
            if importe or i == len(fs) - 1:
                g = {k: f[k] for k in ("n", "pos", "len", "type", "box", "const") if k in f}
                g["desc"] = ""
                keep.append(g)
        pages.append({"sheet": p["sheet"], "fields": keep})
    return {**out, "compacto": "Marcas de página, campos de importe con casilla y último campo de cada página; sin descripciones.", "pages": pages}


def main():
    path, model = Path(sys.argv[1]), sys.argv[2]
    compacto = "--compacto" in sys.argv[3:]
    pages = []
    for sheet, rs in rows(path):
        fields = []
        for r in rs:
            cells = [c for c in r] + [None] * 8
            f = field(cells, model)
            if f:
                fields.append(f)
        if fields:
            pages.append({"sheet": sheet, "fields": fields})
    out = {
        "model": model,
        "source": path.name,
        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        "pages": pages,
    }
    if compacto:
        json.dump(compactar(out), sys.stdout, ensure_ascii=False, separators=(",", ":"))
    else:
        json.dump(out, sys.stdout, ensure_ascii=False, indent=1)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
