"""A quiet house loop, synthesized (no sample, no recording): a soft kick on every beat, a closed hat off the beat,
a pad of four chords and a sub bass, at 120 beats a minute. Usage: python music.py SECONDS OUT.wav"""
import sys
import wave

import numpy as np

SR = 44100
SECONDS = float(sys.argv[1]) if len(sys.argv) > 1 else 90.0
OUT = sys.argv[2] if len(sys.argv) > 2 else 'house.wav'
BPM = 120.0
BEAT = 60.0 / BPM
n = int(SR * SECONDS)
t = np.arange(n) / SR
rng = np.random.default_rng(7)
L = np.zeros(n)
R = np.zeros(n)


def add(buf, start, sig, gain=1.0):
    i = int(start * SR)
    if i >= n:
        return
    j = min(n, i + len(sig))
    buf[i:j] += sig[:j - i] * gain


def env(length, attack, decay):
    x = np.arange(int(length * SR)) / SR
    return np.minimum(1.0, x / max(attack, 1e-4)) * np.exp(-x / decay)


# the kick: a sine that falls from 110 Hz to 45 Hz
kx = np.arange(int(0.32 * SR)) / SR
kick = np.sin(2 * np.pi * (45 * kx + (110 - 45) * 0.045 * (1 - np.exp(-kx / 0.045)))) * np.exp(-kx / 0.11)
# the hat: filtered noise, very short
hx = int(0.05 * SR)
hat = np.diff(rng.standard_normal(hx + 1)) * np.exp(-np.arange(hx) / SR / 0.012)
# the chords (Am7, Fmaj7, Cmaj7, G6), two bars each
NOTE = lambda m: 440.0 * 2 ** ((m - 69) / 12)
CHORDS = [[57, 60, 64, 67], [53, 57, 60, 64], [48, 52, 55, 59], [55, 59, 62, 64]]
ROOTS = [45, 41, 36, 43]

beats = int(SECONDS / BEAT) + 1
duck = np.ones(n)
for b in range(beats):
    bt = b * BEAT
    if bt >= 4 * BEAT:                      # the kick comes in after two bars... of pad alone (one bar = 4 beats)
        add(L, bt, kick, 0.55)
        add(R, bt, kick, 0.55)
        i = int(bt * SR)
        d = np.minimum(1.0, 0.35 + np.arange(int(0.28 * SR)) / (0.28 * SR))
        duck[i:i + len(d)] = np.minimum(duck[i:i + len(d)], d[:max(0, min(len(d), n - i))])
    if bt >= 8 * BEAT:                      # the hat a bar later, off the beat, a little to one side
        add(L, bt + BEAT / 2, hat, 0.10)
        add(R, bt + BEAT / 2, hat, 0.16)

bar = 4 * BEAT
for k in range(int(SECONDS / (2 * bar)) + 1):
    c = CHORDS[k % 4]
    start = k * 2 * bar
    length = 2 * bar
    x = np.arange(int(length * SR)) / SR
    shape = np.minimum(1.0, x / 0.9) * np.minimum(1.0, (length - x) / 0.9)
    for v, m in enumerate(c):
        f = NOTE(m)
        tone = (np.sin(2 * np.pi * f * x) + 0.35 * np.sin(2 * np.pi * 2 * f * x + 0.3) + 0.12 * np.sin(2 * np.pi * 3 * f * x)) * shape
        tone *= 1 + 0.06 * np.sin(2 * np.pi * 0.23 * x + v)
        add(L, start, tone, 0.050 * (1.0 if v % 2 == 0 else 0.7))
        add(R, start, tone, 0.050 * (0.7 if v % 2 == 0 else 1.0))
    # the bass: the root, off the beat, from the third bar on
    if start >= 8 * BEAT:
        f = NOTE(ROOTS[k % 4])
        for b in range(8):
            bx = np.arange(int(0.22 * SR)) / SR
            note = np.sin(2 * np.pi * f * bx) * np.minimum(1.0, bx / 0.01) * np.exp(-bx / 0.12)
            add(L, start + b * BEAT + BEAT / 2, note, 0.22)
            add(R, start + b * BEAT + BEAT / 2, note, 0.22)

# the pad and the bass give way to the kick; a slow fade at both ends
mix = np.stack([L, R]) * duck
fade = np.minimum(1.0, t / 2.5) * np.minimum(1.0, (SECONDS - t) / 4.0)
mix = np.tanh(mix * 1.1) * fade * 0.8
pcm = (np.clip(mix, -1, 1).T * 32767).astype('<i2')
with wave.open(OUT, 'wb') as w:
    w.setnchannels(2)
    w.setsampwidth(2)
    w.setframerate(SR)
    w.writeframes(pcm.tobytes())
print('written', OUT, round(SECONDS, 1), 's')
