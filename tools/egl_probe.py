#!/usr/bin/env python3
"""Probe what GL/GLES this machine's EGL actually exposes.

eglinfo lists EGL platforms but does not create a context, so it never reports
the GL version. This does: bind the ES API, get a config + context, make it
current (surfaceless if the driver allows it) and ask the driver directly.

Run it in the target session, e.g.
  sudo -u dashboard -H env XDG_RUNTIME_DIR=/tmp/dashboard-runtime \
       WAYLAND_DISPLAY=wayland-0 python3 egl_probe.py
"""
import ctypes
import sys

EGL_DEFAULT_DISPLAY = 0
EGL_NO_SURFACE = 0
EGL_NO_CONTEXT = 0
EGL_OPENGL_ES_API = 0x30A0
EGL_OPENGL_ES2_BIT = 0x0004
EGL_OPENGL_ES3_BIT = 0x0040
EGL_RENDERABLE_TYPE = 0x3040
EGL_SURFACE_TYPE = 0x3033
EGL_PBUFFER_BIT = 0x0001
EGL_NONE = 0x3038
EGL_CONTEXT_CLIENT_VERSION = 0x3098

GL_VENDOR = 0x1F00
GL_RENDERER = 0x1F01
GL_VERSION = 0x1F02
GL_SHADING_LANGUAGE_VERSION = 0x8B8C

egl = ctypes.CDLL("libEGL.so.1")
gl = ctypes.CDLL("libGLESv2.so.2")

egl.eglGetDisplay.restype = ctypes.c_void_p
egl.eglGetDisplay.argtypes = [ctypes.c_void_p]
egl.eglInitialize.restype = ctypes.c_uint
egl.eglInitialize.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_int), ctypes.POINTER(ctypes.c_int)]
egl.eglQueryString.restype = ctypes.c_char_p
egl.eglQueryString.argtypes = [ctypes.c_void_p, ctypes.c_int]
egl.eglBindAPI.restype = ctypes.c_uint
egl.eglBindAPI.argtypes = [ctypes.c_uint]
egl.eglChooseConfig.restype = ctypes.c_uint
egl.eglChooseConfig.argtypes = [
    ctypes.c_void_p, ctypes.POINTER(ctypes.c_int), ctypes.POINTER(ctypes.c_void_p),
    ctypes.c_int, ctypes.POINTER(ctypes.c_int),
]
egl.eglCreateContext.restype = ctypes.c_void_p
egl.eglCreateContext.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.POINTER(ctypes.c_int)]
egl.eglMakeCurrent.restype = ctypes.c_uint
egl.eglMakeCurrent.argtypes = [ctypes.c_void_p] * 4
egl.eglGetError.restype = ctypes.c_int
gl.glGetString.restype = ctypes.c_char_p
gl.glGetString.argtypes = [ctypes.c_uint]

dpy = egl.eglGetDisplay(EGL_DEFAULT_DISPLAY)
maj, mnr = ctypes.c_int(), ctypes.c_int()
if not egl.eglInitialize(dpy, ctypes.byref(maj), ctypes.byref(mnr)):
    print(f"eglInitialize failed (EGL error 0x{egl.eglGetError():x})")
    sys.exit(1)
print(f"EGL {maj.value}.{mnr.value}")
ext = (egl.eglQueryString(dpy, 0x3055) or b"").decode()          # EGL_EXTENSIONS
apis = (egl.eglQueryString(dpy, 0x308D) or b"").decode()          # EGL_CLIENT_APIS
print("client APIs:", apis)
print("surfaceless_context:", "EGL_KHR_surfaceless_context" in ext)

if not egl.eglBindAPI(EGL_OPENGL_ES_API):
    print("eglBindAPI(OpenGL ES) failed")
    sys.exit(1)

for label, rtype in (("ES2", EGL_OPENGL_ES2_BIT), ("ES3", EGL_OPENGL_ES3_BIT)):
    cfg_attr = [EGL_RENDERABLE_TYPE, rtype, EGL_SURFACE_TYPE, EGL_PBUFFER_BIT, EGL_NONE]
    cfg = ctypes.c_void_p()
    n = ctypes.c_int()
    ok = egl.eglChooseConfig(
        dpy,
        (ctypes.c_int * len(cfg_attr))(*cfg_attr),
        ctypes.byref(cfg), 1, ctypes.byref(n),
    )
    if not ok or n.value == 0:
        print(f"{label}: no config")
        continue
    ver = 2 if label == "ES2" else 3
    ctx_attr = [EGL_CONTEXT_CLIENT_VERSION, ver, EGL_NONE]
    ctx = egl.eglCreateContext(
        dpy, cfg, EGL_NO_CONTEXT, (ctypes.c_int * len(ctx_attr))(*ctx_attr)
    )
    if not ctx:
        print(f"{label}: context creation failed (0x{egl.eglGetError():x})")
        continue
    # surfaceless make-current is what a headless compositor client would use
    made = egl.eglMakeCurrent(dpy, EGL_NO_SURFACE, EGL_NO_SURFACE, ctx)
    if not made:
        print(f"{label}: eglMakeCurrent(surfaceless) failed (0x{egl.eglGetError():x})")
        continue
    gv = (gl.glGetString(GL_VERSION) or b"").decode()
    gr = (gl.glGetString(GL_RENDERER) or b"").decode()
    gg = (gl.glGetString(GL_VENDOR) or b"").decode()
    sl = (gl.glGetString(GL_SHADING_LANGUAGE_VERSION) or b"").decode()
    print(f"{label}: OK  GL_VERSION={gv!r}")
    print(f"      GL_RENDERER={gr!r}  GL_VENDOR={gg!r}")
    print(f"      GLSL={sl!r}")
