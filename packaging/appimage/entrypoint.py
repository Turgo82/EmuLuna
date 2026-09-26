"""PyInstaller entry point; normal source runs continue through ``python -m emuluna``."""
from emuluna.__main__ import dispatch  # noqa: F401 - importing runs the dispatcher
