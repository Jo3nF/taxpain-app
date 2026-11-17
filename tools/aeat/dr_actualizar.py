"""Busca en la Sede de la AEAT diseños de registro (DR) nuevos o actualizados de los modelos que
usa TaxPain y, si se pide, los baja y genera su JSON para revisarlos.

La AEAT publica cada año (y a veces a mitad de año) nuevos diseños de registro. TaxPain lee los
ficheros de las declaraciones con el diseño de su ejercicio (`crates/models/src/dr_versions.rs`):
sin el diseño nuevo, las declaraciones de ese ejercicio se leen del PDF y la app avisa de que hay
que actualizar TaxPain. Esta herramienta lo detecta antes.

Uso:
    python3 -I tools/aeat/dr_actualizar.py --comprobar
        Sólo avisa. Código de salida: 0 al día, 1 hay novedades, 3 sin red (aviso; la CI no falla).
    python3 -I tools/aeat/dr_actualizar.py
        Baja las novedades a `fixtures/aeat/dr/nuevos/`, calcula su SHA-256, genera el JSON de las
        hojas de cálculo con `tools/aeat/dr2json.py` (necesita openpyxl y xlrd) y las anota en
        `fixtures/aeat/dr/sede.json` como «pendiente». No hace commit: hay que revisarlas, moverlas
        a `fixtures/aeat/dr/anteriores/` (o sustituir el vigente), añadirlas a `dr_versions.rs` y a
        `SOURCES.md` y marcarlas «incluido».
    python3 -I tools/aeat/dr_actualizar.py --registrar
        Anota en `sede.json` lo que hay ahora en la Sede: «incluido» lo que ya está en `SOURCES.md`
        e «ignorado» lo que TaxPain no usa (diseños anteriores a 2017). Para empezar o después de
        revisar las novedades.

Qué es una novedad: un enlace a un diseño de un modelo de TaxPain que no está en `sede.json`, o
uno que ya estaba pero cuya fecha «actualizado …» ha cambiado (la AEAT a veces sustituye el
fichero con el mismo nombre). Antes de cada versión se ejecuta (docs/RELEASE.md), y también en
`scripts/ci.sh` y en una tarea programada.

Sólo usa la biblioteca estándar salvo para generar el JSON.
"""

from __future__ import annotations

import argparse
import hashlib
import html
import json
import re
import sys
import urllib.error
import urllib.request
from dataclasses import asdict, dataclass
from html.parser import HTMLParser
from pathlib import Path

SEDE = "https://sede.agenciatributaria.gob.es"
PAGINAS = [
    f"{SEDE}/Sede/ayuda/disenos-registro/{p}.html"
    for p in (
        "modelos-100-199",
        "modelos-200-299",
        "modelos-300-399",
        "ejercicios-anteriores-modelos-100-199",
        "ejercicios-anteriores-modelos-200-299",
        "ejercicios-anteriores-modelos-300-399",
    )
]
# Modelos cuyos ficheros lee o genera TaxPain.
MODELOS = {
    "111", "115", "123", "130", "180", "184", "190", "193",
    "200", "202", "303", "347", "349", "390",
}
# Primer ejercicio que interesa: los diseños anteriores se ignoran.
PRIMER_EJERCICIO = 2017

RAIZ = Path(__file__).resolve().parents[2]
DR = RAIZ / "fixtures" / "aeat" / "dr"
REGISTRO = DR / "sede.json"
FUENTES = DR / "SOURCES.md"
NUEVOS = DR / "nuevos"

EXT = re.compile(r"\.(xlsx?|pdf)$", re.I)
ACTUALIZADO = re.compile(r"actualizad[oa]\s+(?:el\s+)?([0-9]{1,2}[-/][0-9]{1,2}[-/][0-9]{2,4}|[A-Za-zñÑáéíóú]+\s+[0-9]{4})", re.I)
EJERCICIO = re.compile(r"[Ee]jercicios?\s+((?:19|20)[0-9]{2})")
# Un año suelto; no el número de una orden («HAP/2055/2012»: el 2055 va seguido de «/»).
AÑO = re.compile(r"(?<![0-9])((?:19|20)[0-9]{2})(?![0-9/])")


def ejercicio_de(texto: str) -> int | None:
    """El ejercicio del diseño: el de «Ejercicio 2026» o, si no lo dice, el año más reciente que
    aparezca en el texto (el de la orden que lo aprobó, «1P 2016»…). `None` si no hay ninguno."""
    e = EJERCICIO.search(texto)
    if e:
        return int(e.group(1))
    años = [int(a) for a in AÑO.findall(texto)]
    return max(años) if años else None


@dataclass
class Enlace:
    modelo: str
    nombre: str
    url: str
    texto: str
    actualizado: str
    ejercicio: int | None


class _Anclas(HTMLParser):
    """Recoge (href, texto) de cada enlace de la página."""

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.anclas: list[tuple[str, str]] = []
        self._href: str | None = None
        self._texto: list[str] = []

    def handle_starttag(self, tag, attrs):
        if tag == "a":
            self._href = dict(attrs).get("href")
            self._texto = []

    def handle_data(self, data):
        if self._href is not None:
            self._texto.append(data)

    def handle_endtag(self, tag):
        if tag == "a" and self._href is not None:
            texto = re.sub(r"\s+", " ", "".join(self._texto)).strip()
            self.anclas.append((self._href, texto))
            self._href = None


def enlaces(pagina: str, base: str = SEDE) -> list[Enlace]:
    """Diseños de los modelos de TaxPain enlazados en una página de la Sede."""
    p = _Anclas()
    p.feed(pagina)
    out = []
    for href, texto in p.anclas:
        if not EXT.search(href):
            continue
        texto = html.unescape(texto)
        m = re.match(r"\s*(?:Anexo\s+)?([0-9]{3})\s*-", texto)
        if not m or m.group(1) not in MODELOS or texto.lower().startswith("anexo"):
            continue
        url = href if href.startswith("http") else base + href
        a = ACTUALIZADO.search(texto)
        out.append(
            Enlace(
                modelo=m.group(1),
                nombre=url.rsplit("/", 1)[-1],
                url=url,
                texto=texto,
                actualizado=a.group(1) if a else "",
                ejercicio=ejercicio_de(texto),
            )
        )
    return out


def en_fuentes(texto_fuentes: str) -> set[str]:
    """Ficheros de diseño que ya están en SOURCES.md (los nombres entre comillas invertidas)."""
    return set(re.findall(r"`([^`/\s]+\.(?:xlsx?|pdf))`", texto_fuentes, re.I))


def novedades(actuales: list[Enlace], registro: dict) -> list[tuple[str, Enlace]]:
    """(«nuevo» o «actualizado», enlace) frente al registro."""
    vistos = registro.get("enlaces", {})
    out = []
    for e in actuales:
        r = vistos.get(e.nombre)
        if r is None:
            out.append(("nuevo", e))
        elif e.actualizado and r.get("actualizado") != e.actualizado:
            out.append(("actualizado", e))
    return out


def registrar(actuales: list[Enlace], registro: dict, fuentes: set[str]) -> dict:
    """Anota lo que hay en la Sede: incluido (en SOURCES.md) o ignorado (anterior a 2017)."""
    vistos = registro.setdefault("enlaces", {})
    for e in actuales:
        previo = vistos.get(e.nombre, {})
        if e.nombre in fuentes:
            estado, motivo = "incluido", ""
        elif previo.get("estado") in ("incluido", "pendiente"):
            estado, motivo = previo["estado"], previo.get("motivo", "")
        elif e.ejercicio is None or e.ejercicio < PRIMER_EJERCICIO:
            estado, motivo = "ignorado", f"anterior a {PRIMER_EJERCICIO}"
        else:
            # Lo demás se queda fuera del registro: sale como novedad hasta revisarlo.
            continue
        vistos[e.nombre] = {**asdict(e), "estado": estado, "motivo": motivo} | (
            {"sha256": previo["sha256"]} if "sha256" in previo else {}
        )
    return registro


def bajar(url: str, timeout: float = 30) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": "TaxPain dr_actualizar"})
    with urllib.request.urlopen(req, timeout=timeout) as r:  # noqa: S310 (sólo https de la Sede)
        return r.read()


def leer_sede(get=bajar) -> list[Enlace]:
    out: list[Enlace] = []
    vistos = set()
    for url in PAGINAS:
        pagina = get(url).decode("utf-8", errors="replace")
        for e in enlaces(pagina):
            if e.nombre not in vistos:
                vistos.add(e.nombre)
                out.append(e)
    return out


def cargar_registro() -> dict:
    if REGISTRO.exists():
        return json.loads(REGISTRO.read_text(encoding="utf-8"))
    return {"enlaces": {}}


def guardar_registro(r: dict) -> None:
    r["enlaces"] = dict(sorted(r.get("enlaces", {}).items()))
    REGISTRO.write_text(json.dumps(r, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")


def actualizar(nuevas: list[tuple[str, Enlace]], registro: dict, get=bajar) -> list[str]:
    """Baja las novedades a `nuevos/`, con su SHA-256 y, en las hojas de cálculo, su JSON."""
    NUEVOS.mkdir(parents=True, exist_ok=True)
    hechos = []
    for tipo, e in nuevas:
        if not e.url.startswith(SEDE + "/"):
            continue
        datos = get(e.url)
        destino = NUEVOS / e.nombre
        destino.write_bytes(datos)
        sha = hashlib.sha256(datos).hexdigest()
        if e.nombre.lower().endswith((".xls", ".xlsx")):
            sys.path.insert(0, str(Path(__file__).resolve().parent))
            import subprocess

            json_ = subprocess.run(
                [sys.executable, "-I", str(Path(__file__).with_name("dr2json.py")), str(destino), e.modelo]
                # El 200 pesa unos 10 MB por año: se guarda compacto.
                + (["--compacto"] if e.modelo == "200" else []),
                capture_output=True,
                text=True,
                check=False,
            )
            if json_.returncode == 0:
                destino.with_suffix(".json").write_text(json_.stdout, encoding="utf-8")
            else:
                print(f"  {e.nombre}: no se pudo generar el JSON: {json_.stderr.strip()}")
        registro.setdefault("enlaces", {})[e.nombre] = {
            **asdict(e),
            "estado": "pendiente",
            "motivo": tipo,
            "sha256": sha,
        }
        hechos.append(f"{tipo}: {e.texto} → {destino.relative_to(RAIZ)} ({sha[:12]}…)")
    return hechos


def main(argv: list[str] | None = None, get=bajar) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--comprobar", action="store_true", help="sólo avisa (código 1 si hay novedades)")
    ap.add_argument("--registrar", action="store_true", help="anota lo que hay ahora en la Sede")
    a = ap.parse_args(argv)
    try:
        actuales = leer_sede(get)
    except (urllib.error.URLError, TimeoutError, OSError) as err:
        print(f"AVISO: no se pudo leer la Sede ({err}); no se comprueban los diseños de registro.")
        return 3
    registro = cargar_registro()
    if a.registrar:
        fuentes = en_fuentes(FUENTES.read_text(encoding="utf-8"))
        guardar_registro(registrar(actuales, registro, fuentes))
        print(f"Registrados {len(registro['enlaces'])} diseños en {REGISTRO.relative_to(RAIZ)}.")
    nuevas = novedades(actuales, registro)
    if not nuevas:
        print("Diseños de registro al día.")
        return 0
    print("Diseños de registro nuevos o actualizados en la Sede:")
    for tipo, e in nuevas:
        print(f"  [{tipo}] {e.texto}\n      {e.url}")
    if a.comprobar or a.registrar:
        print(
            "Ejecuta `python3 -I tools/aeat/dr_actualizar.py` para bajarlos y revísalos antes de "
            "la próxima versión (docs/RELEASE.md)."
        )
        return 1
    hechos = actualizar(nuevas, registro, get)
    guardar_registro(registro)
    print("Bajados (sin commit; revísalos, añádelos a dr_versions.rs y SOURCES.md y márcalos «incluido» en sede.json):")
    for h in hechos:
        print(f"  {h}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
