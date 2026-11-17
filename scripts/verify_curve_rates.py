#!/usr/bin/env python3
"""Run Curve rate conformance outside the scored matrix.

Requires forge, solc 0.8.35+, Vyper 0.4.3 and Fe 26.5.0. Set EVM_BENCH_SOLC,
EVM_BENCH_VYPER and EVM_BENCH_FE to use binaries outside PATH.
"""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
PORTS = ROOT / "benches/implementations/curve_stableswap_2coin"


def compiler(name):
    binary = os.environ.get(f"EVM_BENCH_{name.upper()}", name)
    path = shutil.which(binary)
    if path is None:
        raise SystemExit(f"Cannot find {binary}. Set EVM_BENCH_{name.upper()}.")
    return str(Path(path).resolve())


def main():
    solc, vyper, fe = (compiler(name) for name in ("solc", "vyper", "fe"))
    with tempfile.TemporaryDirectory(prefix="curve-rate-conformance-") as directory:
        root = Path(directory)
        (root / "src").mkdir()
        (root / "test").mkdir()
        shutil.copy(PORTS / "solidity/CurveStableSwap2CoinReal.sol", root / "src")
        shutil.copy(ROOT / "scripts/fixtures/CurveRates.t.sol", root / "test")
        (root / "foundry.toml").write_text(
            '[profile.default]\nfs_permissions = [{ access = "read", path = "./" }]\n'
        )
        vyper_hex = subprocess.check_output([
            vyper, "--evm-version", "prague", "--optimize", "gas", "-f", "bytecode",
            PORTS / "vyper/latest/CurveStableSwapNG.vy",
        ], text=True)
        subprocess.run([
            fe, "build", PORTS / "fe/CurveStableSwap2CoinReal.fe", "--standalone",
            "--contract", "CurveStableSwap2CoinReal", "-O", "2", "--emit", "bytecode",
            "--out-dir", root,
        ], check=True)
        for name, code in (("vyper", vyper_hex), ("fe", (root / "CurveStableSwap2CoinReal.bin").read_text())):
            (root / f"{name}.bin").write_bytes(bytes.fromhex(code.strip().removeprefix("0x")))
        subprocess.run([
            "forge", "test", "--root", root, "--offline", "--use", solc, "--via-ir",
            "--optimize", "--evm-version", "prague", "-v",
        ], cwd=root, check=True)


if __name__ == "__main__":
    main()
