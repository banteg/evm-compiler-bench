"""Inspect the Python environment of a Vyper console script (standard library only)."""

import base64
import hashlib
import importlib.metadata
import importlib.util
import json
import os
from pathlib import Path
import site
import sys
import sysconfig


def tree_hash(root):
    """Hash actual installed bytes, including package data and native extensions."""
    digest = hashlib.sha256()
    visited = set()

    def fail(error):
        raise error

    for directory, dirs, files in os.walk(root, followlinks=True, onerror=fail):
        directory = Path(directory)
        real = directory.resolve()
        if real in visited:
            raise ValueError(f"repeated directory in Python package tree: {directory}")
        visited.add(real)
        dirs[:] = sorted(d for d in dirs if d != "__pycache__")
        for name in sorted(files):
            path = directory / name
            relative = path.relative_to(root).as_posix()
            digest.update(relative.encode() + b"\0")
            digest.update(hashlib.sha256(path.read_bytes()).digest())
    return digest.hexdigest()


def describe(launcher):
    # `python -c` otherwise puts the working directory ahead of installed packages.
    launcher_dir = Path(launcher).resolve().parent
    if not getattr(sys.flags, "safe_path", False):
        sys.path[0] = str(launcher_dir)
    spec = importlib.util.find_spec("vyper")
    if spec is None or not spec.origin:
        raise ValueError("cannot locate the Vyper package")
    compiler = Path(spec.origin).resolve().parent
    if not (compiler / "cli" / "vyper_compile.py").is_file():
        raise ValueError("Vyper console entry point is missing")

    # An adjacent interpreter can belong to a different installation. Match the
    # launcher to Vyper's installed distribution before trusting its environment.
    launcher = Path(launcher).resolve()
    binary_hash = hashlib.sha256(launcher.read_bytes()).digest()
    launcher_digest = base64.urlsafe_b64encode(binary_hash).decode().rstrip("=")
    distribution = importlib.metadata.distribution("vyper")
    if not any(
        file.name == launcher.name and file.locate().resolve() == launcher
        and file.hash and file.hash.mode == "sha256" and file.hash.value == launcher_digest
        for file in distribution.files or ()
    ):
        raise ValueError("launcher does not match the installed Vyper distribution record")

    site_dirs = {Path(p).resolve() for p in [*site.getsitepackages(), site.getusersitepackages()]}
    interpreter_paths = {Path(sysconfig.get_path(k)).resolve() for k in ("stdlib", "platstdlib")}
    if dynload := sysconfig.get_config_var("DESTSHARED"):
        interpreter_paths.add(Path(dynload).resolve())
    roots = []
    for entry in sys.path:
        root = Path(entry).resolve()
        if root in site_dirs or root.name in {"site-packages", "dist-packages"}:
            if root.is_dir() and root not in roots:
                roots.append(root)
        elif root.exists() and root not in {launcher_dir, compiler.parent, *interpreter_paths}:
            raise ValueError(f"unidentified Python import path: {root}")

    packages = []
    for dist in importlib.metadata.distributions():
        name = dist.metadata["Name"]
        direct_url = json.loads(dist.read_text("direct_url.json") or "{}")
        if direct_url.get("dir_info", {}).get("editable") and name.lower() != "vyper":
            raise ValueError(f"editable dependency outside the installed environment: {name}")
        packages.append(f"{name}=={dist.version}")
    if not roots:
        raise ValueError("no installed package directory found")

    # The compiler tree is included explicitly for editable Vyper checkouts.
    env = hashlib.sha256()
    for root in roots:
        env.update(bytes.fromhex(tree_hash(root)))
    return {
        "vyper_package_sha256": tree_hash(compiler),
        "python_environment_sha256": env.hexdigest(),
        "python_binary_sha256": hashlib.sha256(Path(sys.executable).read_bytes()).hexdigest(),
        "python_version": sys.version,
        "python_packages": "\n".join(sorted(packages, key=str.casefold)),
    }


if __name__ == "__main__":
    print(json.dumps(describe(sys.argv[1]), sort_keys=True))
