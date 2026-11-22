"""Renders brand/video/position-flow.html to an mp4: a headless Edge is driven over the DevTools protocol, the page's
clock is set frame by frame (`setT`), each frame is a screenshot, and ffmpeg (from the imageio-ffmpeg package) encodes
them. Usage: python brand/video/render.py [fps] [scale]"""
import base64
import json
import os
import subprocess
import sys
import tempfile
import time
import urllib.request

import imageio_ffmpeg
import websocket

FPS = int(sys.argv[1]) if len(sys.argv) > 1 else 30
SCALE = float(sys.argv[2]) if len(sys.argv) > 2 else 1.5
HERE = os.path.dirname(os.path.abspath(__file__))
PAGE = 'file:///' + os.path.join(HERE, 'position-flow.html').replace('\\', '/') + '?t=0'
OUT = os.path.join(HERE, 'kpf-position-flow.mp4')
EDGE = r'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe'
PORT = 9377

frames = tempfile.mkdtemp(prefix='kpf-frames-')
profile = tempfile.mkdtemp(prefix='kpf-edge-')
edge = subprocess.Popen([EDGE, '--headless=new', '--disable-gpu', '--hide-scrollbars', f'--remote-debugging-port={PORT}', '--remote-allow-origins=*', f'--user-data-dir={profile}', '--window-size=1280,720', PAGE],
                        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
try:
    target = None
    for _ in range(60):
        try:
            tabs = json.load(urllib.request.urlopen(f'http://127.0.0.1:{PORT}/json'))
            target = next(t for t in tabs if t.get('type') == 'page' and 'position-flow' in t.get('url', ''))
            break
        except Exception:
            time.sleep(0.25)
    ws = websocket.create_connection(target['webSocketDebuggerUrl'], max_size=None, timeout=60)
    n = 0

    def call(method, **params):
        global n
        n += 1
        ws.send(json.dumps({'id': n, 'method': method, 'params': params}))
        while True:
            m = json.loads(ws.recv())
            if m.get('id') == n:
                if 'error' in m:
                    raise RuntimeError(m['error'])
                return m.get('result', {})

    call('Emulation.setDeviceMetricsOverride', width=1280, height=720, deviceScaleFactor=SCALE, mobile=False)
    time.sleep(0.5)
    duration = call('Runtime.evaluate', expression='window.DURATION', returnByValue=True)['result']['value']
    total = int(duration * FPS)
    t0 = time.time()
    for i in range(total):
        call('Runtime.evaluate', expression=f'setT({i / FPS})')
        shot = call('Page.captureScreenshot', format='png')
        open(os.path.join(frames, f'f{i:05d}.png'), 'wb').write(base64.b64decode(shot['data']))
    print(f'{total} frames in {time.time() - t0:.0f} s')
finally:
    edge.terminate()

ff = imageio_ffmpeg.get_ffmpeg_exe()
# The music: synthesized for the length of the film (brand/video/music.py).
MUSIC = os.path.join(HERE, 'house.wav')
subprocess.run([sys.executable, os.path.join(HERE, 'music.py'), str(duration), MUSIC], check=True)
subprocess.run([ff, '-hide_banner', '-loglevel', 'error', '-y', '-framerate', str(FPS), '-i', os.path.join(frames, 'f%05d.png'),
                '-i', MUSIC, '-shortest',
                '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-crf', '18', '-preset', 'slow', '-movflags', '+faststart', '-c:a', 'aac', '-b:a', '160k', OUT], check=True)
print('written', OUT, os.path.getsize(OUT), 'bytes')
