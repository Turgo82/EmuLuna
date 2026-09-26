"""Entry point shared by source installs and the packaged executable."""
import sys


def dispatch():
    # A frozen AppImage has one executable rather than a Python interpreter
    # capable of ``-m``. Internal child processes return through this dispatcher.
    if len(sys.argv) > 1 and sys.argv[1] == "--player-process":
        del sys.argv[1]
        from .player import main
        return main()
    if len(sys.argv) > 1 and sys.argv[1] == "--core-probe":
        path = sys.argv[2]
        from .core_manager import probe_child
        probe_child(path)
        return 0
    from .app import main
    return main()


raise SystemExit(dispatch())
