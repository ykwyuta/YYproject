#!/usr/bin/env python3
"""Package an already-built release with its documentation and notices."""
from pathlib import Path
import platform
import shutil
import tempfile


def main():
    root = Path(__file__).resolve().parents[1]
    windows = platform.system() == "Windows"
    system = "windows" if windows else "linux"
    machine = platform.machine().lower()
    arch = {"amd64": "x64", "x86_64": "x64", "aarch64": "arm64", "arm64": "arm64"}.get(machine)
    if arch is None or platform.system() not in ("Windows", "Linux"):
        raise SystemExit(f"Unsupported package platform: {platform.system()} {machine}")
    names = ["yyproject.exe", "yyprojectw.exe"] if windows else ["yyproject"]
    binaries = [root / "target/release" / name for name in names]
    for source in binaries:
        if not source.is_file():
            raise SystemExit(f"Missing release binary: {source}")
    output = root / "target/dist"
    output.mkdir(parents=True, exist_ok=True)
    name = f"yyproject-{system}-{arch}"
    with tempfile.TemporaryDirectory(prefix="yyproject-package-") as temporary:
        folder = Path(temporary) / name
        folder.mkdir()
        for source in binaries:
            shutil.copy2(source, folder / source.name)
        for directory in ("docs", "help", "licenses"):
            shutil.copytree(root / directory, folder / directory)
        for filename in ("README.md", "NOTICE.md", "COPYING", "COPYING.EXCEPTION"):
            shutil.copy2(root / filename, folder / filename)
        archive = shutil.make_archive(str(output / name), "zip" if windows else "gztar", temporary, name)
    print(archive)


if __name__ == "__main__":
    main()
