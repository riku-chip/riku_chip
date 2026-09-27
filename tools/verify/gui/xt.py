"""Driver XTest para probar riku-gui sin tocar el mouse (X11 / XWayland).

    python3 xt.py <win_id> click X Y | move X Y | drag X1 Y1 X2 Y2 MS | wheel N
                           type TEXTO | key NOMBRE | keytap NOMBRE [shift]
                           focus | raise | shot SALIDA.png

Coordenadas relativas a la ventana. <win_id> sale de
`xwininfo -root -tree | grep 'riku-gui")'`. Requiere libXtst, xwd y Pillow."""
import ctypes, sys, time, subprocess, struct
from PIL import Image
x11 = ctypes.CDLL("libX11.so.6"); xt = ctypes.CDLL("libXtst.so.6")
x11.XOpenDisplay.restype = ctypes.c_void_p
x11.XStringToKeysym.restype = ctypes.c_ulong
import os
dpy = ctypes.c_void_p(x11.XOpenDisplay(os.environ.get("DISPLAY", ":0").encode()))
win = sys.argv[1]
def origin():
    out = subprocess.check_output(["xwininfo", "-id", win]).decode()
    gx = int(out.split("Absolute upper-left X:")[1].split()[0]); gy = int(out.split("Absolute upper-left Y:")[1].split()[0])
    return gx, gy
def flush(): x11.XFlush(dpy); time.sleep(0.15)
def move(x, y):
    ox, oy = origin(); xt.XTestFakeMotionEvent(dpy, -1, ox + x, oy + y, 0); flush()
def click(x, y):
    move(x, y); xt.XTestFakeButtonEvent(dpy, 1, 1, 0); flush(); xt.XTestFakeButtonEvent(dpy, 1, 0, 0); flush()
def key(name, shift=False):
    code = x11.XKeysymToKeycode(dpy, x11.XStringToKeysym(name.encode()))
    sh = x11.XKeysymToKeycode(dpy, x11.XStringToKeysym(b"Shift_L"))
    if shift: xt.XTestFakeKeyEvent(dpy, sh, 1, 0)
    xt.XTestFakeKeyEvent(dpy, code, 1, 0); xt.XTestFakeKeyEvent(dpy, code, 0, 0)
    if shift: xt.XTestFakeKeyEvent(dpy, sh, 0, 0)
    flush()
NAMES = {"_": ("underscore", True), " ": ("space", False), "-": ("minus", False)}
def type_text(t):
    for ch in t:
        if ch in NAMES: key(*NAMES[ch])
        elif ch.isupper(): key(ch, True)
        else: key(ch)
def shot(out):
    subprocess.check_call(["xwd", "-id", win, "-out", "/tmp/_s.xwd"])
    d = open("/tmp/_s.xwd", "rb").read(); h = struct.unpack(">25I", d[:100])
    off = h[0] + h[19] * 12; w, hh, bpl = h[4], h[5], h[12]
    Image.frombuffer("RGB", (w, hh), d[off:off + bpl * hh], "raw", "BGRX" if h[7] == 0 else "XRGB", bpl, 1).save(out)
cmd = sys.argv[2]
if cmd == "click": click(int(sys.argv[3]), int(sys.argv[4]))
elif cmd == "move": move(int(sys.argv[3]), int(sys.argv[4]))
elif cmd == "type": type_text(sys.argv[3])
elif cmd == "key": key(sys.argv[3])
elif cmd == "shot": time.sleep(0.6); shot(sys.argv[3])
if cmd == "focus":
    x11.XSetInputFocus(dpy, ctypes.c_ulong(int(win, 16)), 1, 0); flush()
if cmd == "wheel":
    n = int(sys.argv[3]); b = 4 if n > 0 else 5
    for _ in range(abs(n)):
        xt.XTestFakeButtonEvent(dpy, b, 1, 0); xt.XTestFakeButtonEvent(dpy, b, 0, 0); x11.XFlush(dpy); time.sleep(0.03)
    time.sleep(0.4)
if cmd == "drag":
    x1, y1, x2, y2, ms = map(int, sys.argv[3:8]); steps = 12
    ox, oy = origin()
    xt.XTestFakeMotionEvent(dpy, -1, ox + x1, oy + y1, 0); x11.XFlush(dpy); time.sleep(0.05)
    xt.XTestFakeButtonEvent(dpy, 1, 1, 0); x11.XFlush(dpy)
    for s in range(1, steps + 1):
        time.sleep(ms / 1000.0 / steps)
        xt.XTestFakeMotionEvent(dpy, -1, ox + x1 + (x2 - x1) * s // steps, oy + y1 + (y2 - y1) * s // steps, 0); x11.XFlush(dpy)
    xt.XTestFakeButtonEvent(dpy, 1, 0, 0); x11.XFlush(dpy)
if cmd == "keytap":
    key(sys.argv[3], len(sys.argv) > 4)
if cmd == "raise":
    w = ctypes.c_ulong(int(win, 16))
    x11.XMapRaised(dpy, w); x11.XFlush(dpy); time.sleep(0.3)
    # _NET_ACTIVE_WINDOW: pedir al WM que la active (des-minimiza en WSLg)
    class XClientMessageEvent(ctypes.Structure):
        _fields_ = [("type", ctypes.c_int), ("serial", ctypes.c_ulong), ("send_event", ctypes.c_int), ("display", ctypes.c_void_p),
                    ("window", ctypes.c_ulong), ("message_type", ctypes.c_ulong), ("format", ctypes.c_int), ("data", ctypes.c_long * 5)]
    class XEvent(ctypes.Union):
        _fields_ = [("xclient", XClientMessageEvent), ("pad", ctypes.c_long * 24)]
    x11.XInternAtom.restype = ctypes.c_ulong; x11.XDefaultRootWindow.restype = ctypes.c_ulong
    ev = XEvent(); ev.xclient.type = 33; ev.xclient.window = w.value; ev.xclient.format = 32
    ev.xclient.message_type = x11.XInternAtom(dpy, b"_NET_ACTIVE_WINDOW", 0); ev.xclient.data[0] = 1
    root = x11.XDefaultRootWindow(dpy)
    x11.XSendEvent(dpy, ctypes.c_ulong(root), 0, (1 << 20) | (1 << 19), ctypes.byref(ev)); x11.XFlush(dpy); time.sleep(0.5)
