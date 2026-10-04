from pathlib import Path

project = Path(SPEC).resolve().parents[2]
data = [
    (str(project / "emuluna" / "data"), "emuluna/data"),
    (str(project / "build" / "cores" / "libemuluna_host.so"), "build/cores"),
    (str(project / "build" / "cores" / "emuluna-chd-probe"), "build/cores"),
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
        "PySide6.QtQuick",
        "PySide6.QtQuickWidgets",
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
# Qt Quick's stock hook copies every QML module, including WebEngine and Quick3D.
# The Vulkan viewport imports only QtQuick's basic Image/Rectangle types.
qml_roots = {"QtQml", "QtQml/Models", "QtQml/WorkerScript", "QtQuick", "QtQuick/Window"}
def needed_qml(destination):
    marker = "PySide6/Qt/qml/"
    if marker not in destination:
        return True
    relative = destination.split(marker, 1)[1]
    directory = relative.rsplit("/", 1)[0] if "/" in relative else ""
    return directory in qml_roots
a.datas = [entry for entry in a.datas if needed_qml(entry[0])]
a.binaries = [entry for entry in a.binaries
              if Path(entry[0]).name not in host_graphics_runtimes
              and Path(entry[0]).name != "libQt6WebEngineCore.so.6"
              and Path(entry[0]).name != "QtWebEngineProcess"
              and needed_qml(entry[0])
              and "/plugins/qmltooling/" not in entry[0]]
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
