#!/usr/bin/env python3
"""Install the pinned PostgreSQL into runtime/pgsql, for Companion to run as its
own private database on this PC (Windows, Linux).

When COMPANION_DATABASE_URL is not set, Companion starts a private PostgreSQL
from runtime/pgsql (its files in the data folder, reachable only from this PC
with a generated password) and stops it when Companion closes. The backend
tests do the same with a throwaway database.

The version is the one in runtime/postgresql.lock.json, and every download must
match the SHA-256 recorded there:
  Windows: the official EnterpriseDB build; only bin, lib and share are kept
           (pgAdmin, StackBuilder, the manuals and headers stay out).
  Linux:   built from the official source; needs a C compiler, make,
           bison and flex (e.g. `sudo apt install build-essential bison flex`).
The download (and the Linux build folder) is deleted afterwards unless
--keep-download.

A server that is not a PC uses its own PostgreSQL instead: set
COMPANION_DATABASE_URL and this script is not needed.

Usage: python scripts/get-postgres.py [--force] [--keep-download]
"""

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tarfile
import urllib.request
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LOCK = json.loads((ROOT / "runtime" / "postgresql.lock.json").read_text(encoding="utf-8"))
DOWNLOADS = ROOT / "build" / "postgresql"
OUTPUT = ROOT / "runtime" / "pgsql"
STAGING = ROOT / "runtime" / "pgsql.staging"
WINDOWS = sys.platform == "win32"


def step(message):
    print(f"==> {message}", flush=True)


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as file:
        for block in iter(lambda: file.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def download(entry):
    """The file `entry` names, downloaded unless already here, matching its SHA-256."""
    target = DOWNLOADS / entry["url"].rsplit("/", 1)[-1]
    if target.is_file() and sha256(target) == entry["sha256"]:
        return target
    DOWNLOADS.mkdir(parents=True, exist_ok=True)
    step(f"Downloading {entry['url']}")
    partial = target.with_name(target.name + ".part")
    with urllib.request.urlopen(entry["url"]) as response, open(partial, "wb") as file:
        shutil.copyfileobj(response, file, 1 << 20)
    actual = sha256(partial)
    if actual != entry["sha256"]:
        partial.unlink()
        sys.exit(f"The download does not match runtime/postgresql.lock.json "
                 f"(SHA-256 {actual}, expected {entry['sha256']}). It was deleted; nothing was installed.")
    partial.replace(target)
    return target


def install_windows(archive):
    step("Extracting bin, lib and share")
    with zipfile.ZipFile(archive) as zipped:
        for name in zipped.namelist():
            keep = name.startswith(("pgsql/bin/", "pgsql/lib/", "pgsql/share/")) or name == "pgsql/server_license.txt"
            if not keep or name.endswith("/"):
                continue
            target = STAGING / name[len("pgsql/"):]
            target.parent.mkdir(parents=True, exist_ok=True)
            with zipped.open(name) as source, open(target, "wb") as file:
                shutil.copyfileobj(source, file, 1 << 20)


def install_from_source(archive):
    build = DOWNLOADS / "src"
    if build.exists():
        shutil.rmtree(build)
    step("Unpacking the source")
    with tarfile.open(archive) as tar:
        tar.extractall(build, filter="data")
    source = next(build.iterdir())
    step("Building (a few minutes)")
    # No ICU, readline or zlib: Companion's database uses the C locale and
    # never needs psql's line editing or compressed dumps, so the build needs
    # only a compiler, make, bison and flex. No rpath: the result is moved
    # after install, so Companion points the programs at lib/ when it runs them.
    def run(*command):
        subprocess.run(command, cwd=source, check=True)
    run("./configure", f"--prefix={STAGING}", "--without-icu", "--without-readline", "--without-zlib", "--disable-rpath")
    run("make", f"-j{os.cpu_count() or 2}")
    run("make", "install")
    shutil.rmtree(STAGING / "include", ignore_errors=True)
    shutil.copy(source / "COPYRIGHT", STAGING / "server_license.txt")
    shutil.rmtree(build)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--force", action="store_true", help="reinstall even if runtime/pgsql holds the pinned version")
    parser.add_argument("--keep-download", action="store_true", help="keep the download in build/postgresql")
    args = parser.parse_args()

    entry = LOCK["windows"] if WINDOWS else LOCK["source"]
    marker = OUTPUT / "INSTALLED.json"
    if not args.force and marker.is_file() and json.loads(marker.read_text(encoding="utf-8-sig")).get("sha256") == entry["sha256"]:
        step(f"PostgreSQL {LOCK['version']} is already installed in runtime/pgsql")
        return

    archive = download(entry)
    step("The download matches the pinned SHA-256")
    if STAGING.exists():
        shutil.rmtree(STAGING)
    (install_windows if WINDOWS else install_from_source)(archive)
    for tool in ("initdb", "pg_ctl", "postgres"):
        if not (STAGING / "bin" / (tool + (".exe" if WINDOWS else ""))).is_file():
            sys.exit(f"bin/{tool} is missing from the result; nothing was installed.")
    (STAGING / "INSTALLED.json").write_text(json.dumps({"version": LOCK["version"], "sha256": entry["sha256"]}), encoding="utf-8")
    if OUTPUT.exists():
        shutil.rmtree(OUTPUT)
    STAGING.rename(OUTPUT)
    if not args.keep_download:
        archive.unlink()
    size = sum(path.stat().st_size for path in OUTPUT.rglob("*") if path.is_file()) / 2**20
    step(f"PostgreSQL {LOCK['version']} installed in runtime/pgsql ({size:.0f} MB)")


if __name__ == "__main__":
    main()
