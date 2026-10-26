#!/usr/bin/env python3
"""Package an already-built release with its documentation and notices."""
import argparse
from pathlib import Path
import platform
import shutil
import tempfile


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument("--target-dir",type=Path)
    args=parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    target=(args.target_dir or root/"target").resolve()
    windows = platform.system() == "Windows"
    system = "windows" if windows else "linux"
    machine = platform.machine().lower()
    arch = {"amd64": "x64", "x86_64": "x64", "aarch64": "arm64", "arm64": "arm64"}.get(machine)
    if arch is None or platform.system() not in ("Windows", "Linux"):
        raise SystemExit(f"Unsupported package platform: {platform.system()} {machine}")
    names = ["yyproject.exe", "yyprojectw.exe"] if windows else ["yyproject"]
    binaries = [target / "release" / name for name in names]
    for source in binaries:
        if not source.is_file():
            raise SystemExit(f"Missing release binary: {source}")
    demo = root / "demo/yyproject-demo.sqlite3"
    if not demo.is_file() or any(Path(str(demo) + suffix).exists() for suffix in ("-wal", "-shm")):
        raise SystemExit("Demo DB is missing or has WAL/SHM sidecars; close demo connections before packaging")
    output = root / "target/dist"
    output.mkdir(parents=True, exist_ok=True)
    name = f"yyproject-{system}-{arch}"
    with tempfile.TemporaryDirectory(prefix="yyproject-package-") as temporary:
        folder = Path(temporary) / name
        folder.mkdir()
        for source in binaries:
            shutil.copy2(source, folder / source.name)
        for directory in ("docs", "help", "licenses", "demo", "tools"):
            shutil.copytree(root / directory, folder / directory, ignore=shutil.ignore_patterns("__pycache__", "*.pyc"))
        for filename in ("README.md", "NOTICE.md", "COPYING", "COPYING.EXCEPTION", "compose.yaml", ".env.example"):
            shutil.copy2(root / filename, folder / filename)
        archive = shutil.make_archive(str(output / name), "zip" if windows else "gztar", temporary, name)
    print(archive)


if __name__ == "__main__":
    main()
