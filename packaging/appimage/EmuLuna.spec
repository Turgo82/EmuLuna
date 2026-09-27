from pathlib import Path

project = Path(SPEC).resolve().parents[2]
data = [
    (str(project / "emuluna" / "data"), "emuluna/data"),
    (str(project / "build" / "cores" / "libemuluna_host.so"), "build/cores"),
    (str(project / "LICENSE"), "."),
    (str(project / "THIRD_PARTY_NOTICES.md"), "."),
]

a = Analysis(
    [str(project / "packaging" / "appimage" / "entrypoint.py")],
    pathex=[str(project)],
    binaries=[],
    datas=data,
    hiddenimports=[
        "PySide6.QtMultimedia",
        "PySide6.QtOpenGLWidgets",
        "PySide6.QtSvg",
        "emuluna.player",
        "emuluna.core_manager",
    ],
    excludes=["PySide6.QtWebEngineCore", "PySide6.QtWebEngineWidgets"],
    noarchive=False,
)
# The AppImage must use the host compiler runtimes. Bundling Ubuntu's older
# copies ahead of Fedora's Mesa/DRM stack prevents libEGL from loading the GPU
# driver, leaving the gameplay QOpenGLWidget without a drawable window.
host_graphics_runtimes = {"libstdc++.so.6", "libgcc_s.so.1"}
a.binaries = [entry for entry in a.binaries
              if Path(entry[0]).name not in host_graphics_runtimes]
pyz = PYZ(a.pure)
exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name="EmuLuna",
    console=False,
    icon=str(project / "emuluna" / "data" / "branding" / "emuluna.png"),
)
coll = COLLECT(
    exe,
    a.binaries,
    a.datas,
    strip=False,
    upx=False,
    name="EmuLuna",
)
