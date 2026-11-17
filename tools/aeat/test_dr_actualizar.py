"""Pruebas de dr_actualizar.py con HTML de ejemplo sintético (sin red).

    python3 -I -m unittest discover -s tools/aeat -p 'test_*.py'
"""

import sys
import unittest
import urllib.error
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import dr_actualizar as dra  # noqa: E402

PAGINA = """
<html><body><main>
<ul>
<li><a href="/static_files/Sede/Disenyo_registro/DR_300_399/archivos_26/DR303e26v101.xlsx">
303 - Ejercicio 2026 y siguientes (actualizado 28/01/26) <span>(378 KB - xlsx )</span></a></li>
<li><a href="/static_files/Sede/Disenyo_registro/DR_300_399/archivos_27/dr390e2027.xlsx">
390 - Ejercicio 2027 y siguientes <span>(544 KB - xlsx )</span></a></li>
<li><a href="/static_files/Sede/Disenyo_registro/ant_300_399/archivos/DR303e14v22.xlsx">
303 - Orden HAP/2373/2014, de 9 de diciembre (ejercicio 2014) (246 KB - xlsx )</a></li>
<li><a href="/static_files/Sede/Disenyo_registro/DR_300_399/archivos/347.pdf">
347 - Ejercicio 2025 y siguientes. Modificados por Orden HAC/1431/2025 (332 KB - pdf )</a></li>
<li><a href="/static_files/Sede/Disenyo_registro/DR_100_199/archivos_26/DR131_2026.xlsx">
131 - Ejercicios 2026 (actualizado 28/09/26) (180 KB - xlsx )</a></li>
<li><a href="/static_files/Sede/Disenyo_registro/DR_200_299/A202e25.xlsx">
Anexo modelo 202: Comunicación de datos adicionales (actualizado 14-07-26)</a></li>
<li><a href="/Sede/ayuda.html">303 - Ayuda</a></li>
</ul></main></body></html>
"""

FUENTES = """
| 303 | `DR303e26v101.xlsx` | `DR_300_399/archivos_26/` | `0be8` | Ejercicio 2026 |
| 347 | `347.pdf` | `DR_300_399/archivos/` | `1458` | Ejercicio 2025 |
"""


class Pruebas(unittest.TestCase):
    def test_enlaces_de_los_modelos_de_taxpain(self):
        e = dra.enlaces(PAGINA)
        nombres = [x.nombre for x in e]
        self.assertEqual(nombres, ["DR303e26v101.xlsx", "dr390e2027.xlsx", "DR303e14v22.xlsx", "347.pdf"])
        p = e[0]
        self.assertEqual(p.modelo, "303")
        self.assertEqual(p.actualizado, "28/01/26")
        self.assertEqual(p.ejercicio, 2026)
        self.assertTrue(p.url.startswith("https://sede.agenciatributaria.gob.es/static_files/"))
        self.assertEqual(e[2].ejercicio, 2014)
        # Sin «Ejercicio»: el año más reciente del texto.
        self.assertEqual(dra.ejercicio_de("202 - Orden HAP/523/2015 (1P 2016)"), 2016)
        self.assertEqual(dra.ejercicio_de("202 - Orden HAP/2055/2012 (v3.2)"), 2012)
        self.assertIsNone(dra.ejercicio_de("303 - Orden EHA/3786/2008 (v1.1)".replace("2008", "x")))

    def test_fuentes(self):
        self.assertEqual(dra.en_fuentes(FUENTES), {"DR303e26v101.xlsx", "347.pdf"})

    def test_registrar_y_novedades(self):
        e = dra.enlaces(PAGINA)
        r = dra.registrar(e, {"enlaces": {}}, dra.en_fuentes(FUENTES))
        estados = {k: v["estado"] for k, v in r["enlaces"].items()}
        self.assertEqual(
            estados,
            {"DR303e26v101.xlsx": "incluido", "347.pdf": "incluido", "DR303e14v22.xlsx": "ignorado"},
        )
        # El 390 de 2027 no está: es una novedad.
        self.assertEqual([(t, x.nombre) for t, x in dra.novedades(e, r)], [("nuevo", "dr390e2027.xlsx")])
        # La AEAT cambia la fecha de «actualizado» de un diseño ya incluido.
        otra = PAGINA.replace("actualizado 28/01/26", "actualizado 15/03/26")
        n = dra.novedades(dra.enlaces(otra), r)
        self.assertIn(("actualizado", "DR303e26v101.xlsx"), [(t, x.nombre) for t, x in n])

    def test_comprobar_sin_red_avisa_sin_fallar(self):
        def sin_red(url):
            raise urllib.error.URLError("sin conexión")

        with mock.patch("builtins.print"):
            self.assertEqual(dra.main(["--comprobar"], get=sin_red), 3)

    def test_comprobar_con_novedades_y_al_dia(self):
        e = dra.enlaces(PAGINA)
        registro = dra.registrar(e, {"enlaces": {}}, dra.en_fuentes(FUENTES))
        get = lambda url: PAGINA.encode("utf-8")  # noqa: E731
        with mock.patch.object(dra, "cargar_registro", return_value=registro), mock.patch(
            "builtins.print"
        ):
            self.assertEqual(dra.main(["--comprobar"], get=get), 1)
        registro["enlaces"]["dr390e2027.xlsx"] = {"estado": "incluido", "actualizado": ""}
        with mock.patch.object(dra, "cargar_registro", return_value=registro), mock.patch(
            "builtins.print"
        ):
            self.assertEqual(dra.main(["--comprobar"], get=get), 0)


if __name__ == "__main__":
    unittest.main()
