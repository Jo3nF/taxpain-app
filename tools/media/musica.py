# Música de fondo de los vídeos de lanzamiento, sintetizada con numpy: no usa muestras ni
# ficheros de terceros, así que no hay licencias que respetar.
#
# Uso: python3 -I tools/media/musica.py <tranquila|producto> <segundos> <salida.wav>
#   tranquila: piano eléctrico a golpes, base suave y una melodía que cambia cada compás (84 ppm).
#   producto:  arpegio de cuerda pulsada con bombo, bajo y campana (104 ppm).
# La llama tools/media/lanzamiento.mjs; necesita numpy.
import sys, wave
import numpy as np

SR = 44100
estilo, dur, salida = sys.argv[1], float(sys.argv[2]), sys.argv[3]
N = int(SR * (dur + 0.5))
L = np.zeros(N); R = np.zeros(N)
rng = np.random.default_rng(7)
hz = lambda m: 440.0 * 2 ** ((m - 69) / 12)

def pon(sig, t0, g=1.0, pan=0.0):
    i = int(t0 * SR)
    if i >= N: return
    s = sig[: N - i]
    L[i:i + len(s)] += s * g * (1 - max(0, pan))
    R[i:i + len(s)] += s * g * (1 + min(0, pan))

def env(n, a=0.004):
    e = np.ones(n); k = max(1, int(a * SR)); e[:k] = np.linspace(0, 1, k); return e

def pulsada(m, d=0.9, brillo=6):
    n = int(d * SR); t = np.arange(n) / SR; f = hz(m); y = np.zeros(n)
    for k in range(1, brillo + 1):
        y += np.sin(2 * np.pi * k * f * t) / k * np.exp(-t * (2.5 + 1.6 * k))
    return y * env(n)

def campana(m, d=1.4):
    n = int(d * SR); t = np.arange(n) / SR; f = hz(m)
    y = np.sin(2 * np.pi * f * t) * np.exp(-t * 3.2) + 0.35 * np.sin(2 * np.pi * 2 * f * t) * np.exp(-t * 5) \
        + 0.12 * np.sin(2 * np.pi * 3.01 * f * t) * np.exp(-t * 8)
    return y * env(n, 0.006)

def piano(m, d=1.6):
    n = int(d * SR); t = np.arange(n) / SR; f = hz(m)
    mod = 1.4 * np.exp(-t * 6) * np.sin(2 * np.pi * 2 * f * t)          # FM suave: piano eléctrico
    y = np.sin(2 * np.pi * f * t + mod) * np.exp(-t * 2.2) * (1 + 0.08 * np.sin(2 * np.pi * 4.5 * t))
    return y * env(n, 0.005)

def bajo(m, d=0.45):
    n = int(d * SR); t = np.arange(n) / SR; f = hz(m)
    y = (np.sin(2 * np.pi * f * t) + 0.3 * np.sin(2 * np.pi * 2 * f * t)) * np.exp(-t * 4.5)
    k = int(0.02 * SR); y[-k:] *= np.linspace(1, 0, k)
    return y * env(n, 0.006)

def colchon(notas, d):
    n = int(d * SR); t = np.arange(n) / SR; y = np.zeros(n)
    for m in notas:
        f = hz(m); y += np.sin(2 * np.pi * f * t) + 0.6 * np.sin(2 * np.pi * f * 1.005 * t)
    a = int(min(0.8, d / 3) * SR); e = np.ones(n); e[:a] = np.linspace(0, 1, a); e[-a:] = np.linspace(1, 0, a)
    return y * e / len(notas)

def bombo(d=0.28):
    n = int(d * SR); t = np.arange(n) / SR
    fase = 2 * np.pi * (45 * t + (120 - 45) / 28 * (1 - np.exp(-28 * t)))
    return np.sin(fase) * np.exp(-t * 11) * env(n, 0.002)

def ruido(d, caida, agudo=True):
    n = int(d * SR); x = rng.standard_normal(n)
    if agudo: x = np.diff(x, prepend=0); x = np.diff(x, prepend=0)
    else: x = np.convolve(x, np.ones(6) / 6, 'same') - np.convolve(x, np.ones(60) / 60, 'same')
    return x * np.exp(-np.arange(n) / SR * caida) * env(n, 0.001)

def producto():
    bpm = 104; negra = 60 / bpm; compas = 4 * negra
    acordes = [(47, [59, 62, 66, 69]), (43, [55, 59, 62, 66]), (38, [62, 64, 66, 69]), (45, [57, 61, 64, 69])]
    total = int(np.ceil(dur / compas)); fin = total - 2
    arps = [[0, 1, 2, 3, 2, 1, 2, 3], [0, 2, 1, 3, 2, 3, 1, 2], [3, 2, 1, 0, 1, 2, 3, 2], [0, 1, 3, 2, 0, 2, 3, 1]]
    motivo = [[(78, 0, 1.5), (76, 1.5, .5), (74, 2, 2)], [(71, 0, 1.5), (74, 1.5, .5), (76, 2, 2)],
              [(78, 0, 1), (81, 1, 1), (78, 2, 2)], [(76, 0, 3)]]
    motivo2 = [[(74, 0, 1), (76, 1, 1), (78, 2, 2)], [(81, 0, 1.5), (78, 1.5, .5), (76, 2, 2)],
               [(74, 0, 1.5), (69, 1.5, .5), (74, 2, 2)], [(73, 0, 2), (76, 2, 2)]]
    for c in range(total):
        t0 = c * compas; raiz, notas = acordes[c % 4]; bloque = c // 4
        lleno = 4 <= c < fin; ritmo = 2 <= c < fin
        pon(colchon(notas, compas + 0.4), t0, 0.05)
        patron = arps[(bloque + (c % 2)) % 4]
        for i, p in enumerate(patron):
            oct = 12 if (lleno and i in (3, 7)) or (bloque % 2 == 1 and i == 5) else 0
            g = 0.16 if i % 2 == 0 else 0.11
            pon(pulsada(notas[p] + oct), t0 + i * negra / 2, g, pan=0.35 if i % 2 else -0.35)
        if ritmo:
            for b in (0, 1.5, 2, 3.5) if c % 2 == 0 else (0, 1.5, 2.5, 3):
                pon(bajo(raiz), t0 + b * negra, 0.34)
            for b in range(4):
                pon(bombo(), t0 + b * negra, 0.5)
        if lleno:
            for b in range(4):
                pon(ruido(0.06, 70), t0 + (b + 0.5) * negra, 0.035, pan=0.2)
            for b in (1, 3):
                pon(ruido(0.16, 28, agudo=False), t0 + b * negra, 0.10)
            frase = (motivo if (bloque % 2 == 1) else motivo2)[c % 4]
            for m, b, d in frase:
                pon(campana(m, 0.5 + d * negra), t0 + b * negra, 0.13, pan=-0.1)
        if c == total - 1:
            for m in [50, 57, 62, 66, 69]:
                pon(pulsada(m, 3.0, 4), t0, 0.14)

def tranquila():
    bpm = 84; negra = 60 / bpm; compas = 4 * negra; sw = negra * 0.16
    acordes = [(41, [57, 60, 64, 67]), (40, [55, 59, 62, 66]), (38, [53, 57, 60, 64]), (36, [52, 55, 59, 62])]
    total = int(np.ceil(dur / compas)); fin = total - 1; pausa = int(total * 0.6)
    golpes = [(0, 1.0), (1.5, 0.7), (3, 0.8)], [(0, 1.0), (2.5, 0.75), (3.5, 0.6)]
    penta = [72, 74, 76, 79, 81, 84]
    frases = [[(2, 0, 1), (3, 1, .5), (4, 1.5, 1.5)], [(3, 0.5, 1), (2, 1.5, .5), (1, 2, 2)],
              [(0, 0, .5), (1, .5, .5), (2, 1, 1), (4, 2.5, 1.5)], [(3, 0, 1.5), (1, 2, 2)],
              [(4, 0, 1), (5, 1, 1), (4, 2, .5), (3, 2.5, 1.5)], [(2, 0.5, .5), (3, 1, 1), (2, 2, 2)],
              [(1, 0, 1), (2, 1, 1), (0, 2, 2)], [(2, 0, 3)]]
    for c in range(total):
        t0 = c * compas; raiz, notas = acordes[c % 4]
        ritmo = 2 <= c < fin and c != pausa; canta = c >= 4 and c < fin
        for b, g in golpes[c % 2]:
            for j, m in enumerate(notas):
                pon(piano(m), t0 + b * negra + j * 0.012, 0.10 * g, pan=(j - 1.5) * 0.15)
        if ritmo:
            for b in (0, 2.5) if c % 2 == 0 else (0, 1.5, 2.5):
                pon(bombo(0.32), t0 + b * negra, 0.5)
                pon(bajo(raiz, 0.6), t0 + b * negra, 0.30)
            for b in (1, 3):
                pon(ruido(0.2, 22, agudo=False), t0 + b * negra, 0.11)
            for i in range(8):
                pon(ruido(0.05, 80), t0 + i * negra / 2 + (sw if i % 2 else 0), 0.03 if i % 2 else 0.02, pan=0.25)
        if canta:
            for p, b, d in frases[(c - 4) % 8]:
                pon(pulsada(penta[p], 0.5 + d * negra, 3), t0 + b * negra + (sw if (b * 2) % 2 else 0), 0.17, pan=-0.15)
        if c == total - 1:
            for m in [36, 52, 55, 59, 64]:
                pon(piano(m, 3.2), t0, 0.12)

(producto if estilo == 'producto' else tranquila)()

def eco(x, seg, g):
    D = int(seg * SR); y = x.copy()
    for i in range(D, N, D):
        y[i:i + D] += g * y[i - D:i][: len(y[i:i + D])]
    return y
# Sala: ecos cortos cruzados entre canales, mezclados bajos.
revL = eco(L, 0.031, 0.5) * 0.5 + eco(R, 0.047, 0.45) * 0.5
revR = eco(R, 0.037, 0.5) * 0.5 + eco(L, 0.053, 0.45) * 0.5
L2 = L + 0.22 * revL; R2 = R + 0.22 * revR
t = np.arange(N) / SR
f = np.clip(t / 0.4, 0, 1) * np.clip((dur - t) / 2.5, 0, 1)
x = np.stack([L2 * f, R2 * f], 1)[: int(dur * SR)]
x = np.tanh(x * 1.6); x = x / np.abs(x).max() * 0.89
with wave.open(salida, 'wb') as w:
    w.setnchannels(2); w.setsampwidth(2); w.setframerate(SR)
    w.writeframes((x * 32767).astype('<i2').tobytes())
