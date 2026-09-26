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
