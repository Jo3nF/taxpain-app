"""Tabla partida de cuentas anuales → casilla de las páginas de balance y PyG del modelo 200.

Uso: python -I tools/aeat/m200_cuentas.py fixtures/aeat/dr/200-cuentas.json

Empareja, sección a sección y en orden, el texto de cada partida de
`fixtures/accounting/cuentas-anuales-{pymes,abreviado,normal}.json` con la descripción de las
casillas de las páginas 03 a 08 de `fixtures/aeat/dr/200.json` (DR200e25). Las líneas de
resultado y las que no casan por texto van en MANUAL. Cada casilla se guarda como
[página, casilla], porque el 200 repite números de casilla en páginas distintas.
"""
import json, re, sys, unicodedata
from pathlib import Path

root = str(Path(__file__).resolve().parents[2]) + '/'

MANUAL = {
    'pymes': {'pyg:A': ['07', 296], 'pyg:B': ['08', 324], 'pyg:C': ['08', 325], 'pyg:D': ['08', 327]},
    'abreviado': {'pyg:A': ['07', 296], 'pyg:B': ['08', 324], 'pyg:C': ['08', 325], 'pyg:D': ['08', 327]},
    'normal': {
        'pyg:A.1': ['07', 296], 'pyg:A.2': ['08', 324], 'pyg:A.3': ['08', 325],
        'pyg:A.4': ['08', 327], 'pyg:18': ['08', 328], 'pyg:4.d': ['07', 264],
    },
}
dr=json.load(open(root+'fixtures/aeat/dr/200.json'))
def norm(s):
    s=unicodedata.normalize('NFD',s.lower())
    s=''.join(c for c in s if unicodedata.category(c)!='Mn')
    s=re.sub(r'\[.*?\]','',s)
    s=re.sub(r'^\s*([a-z]-?\d?\)|[ivx]+\.|\d+\.|[a-z]\d?\)|[a-z]\.\d\))\s*','',s)
    s=re.sub(r'\(([^)]*)\)',r'\1',s)
    s=re.sub(r'[^a-z0-9 ]',' ',s)
    return ' '.join(s.split())
fields=[]
for p in dr['pages']:
    if p['sheet'] in ('DP200003','DP200004','DP200005','DP200006','DP200007','DP200008'):
        for f in p['fields']:
            if f.get('box') and f['type'] in ('N','Num') and f['len']==17:
                last=f['desc'].split(' - ')[-1]
                sec='pyg' if p['sheet'] in ('DP200007','DP200008') else 'balance'
                fields.append((sec,p['sheet'],f['box'],norm(last),f['desc']))
out={}
for model in ['pymes','abreviado','normal']:
    d=json.load(open(root+f'fixtures/accounting/cuentas-anuales-{model}.json'))
    res={}; miss=[]
    for sec in ('balance','pyg'):
        cand=[f for f in fields if f[0]==sec]
        idx=0
        for line in d[sec]:
            if line['id'] in ('TA','TP') : 
                key='total activo' if line['id']=='TA' else 'total patrimonio neto y pasivo'
            else: key=norm(line['label'])
            key=key.rstrip('. ')
            hit=None
            for j in range(idx,len(cand)):
                if cand[j][3]==key or cand[j][3].startswith(key) and len(key)>12:
                    hit=j;break
            if hit is None:
                for j in range(0,len(cand)):
                    if cand[j][3]==key: hit=j;break
            if hit is None: miss.append((sec,line['id'],line['label'])); continue
            res[f"{sec}:{line['id']}"]=[cand[hit][1][-2:] if cand[hit][1][-1].isdigit() else cand[hit][1][-3:],cand[hit][2]]; idx=hit+1
    res.update(MANUAL[model])
    miss = [m for m in miss if f"{m[0]}:{m[1]}" not in res]
    out[model] = dict(sorted(res.items()))
    print('==', model, len(res), 'casillas; sin casilla:', [f"{m[0]}:{m[1]}" for m in miss], file=sys.stderr)
out['source'] = 'DR200e25.xls (v1.02) vía fixtures/aeat/dr/200.json; generado por tools/aeat/m200_cuentas.py'
out['notes'] = ('Sin casilla equivalente en el 200 (se avisa al generar el fichero): normal pyg:6.b '
                '(cargas sociales: el 200 las desglosa en 00274-00277) y balance:PC.I (pasivos '
                'vinculados con activos mantenidos para la venta). El resultado del ejercicio va '
                'además en [00500] de la página 08.')
json.dump(out, open(sys.argv[1], 'w'), indent=1, ensure_ascii=False)
open(sys.argv[1], 'a').write('\n')
