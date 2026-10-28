#!/usr/bin/env python3
"""Integration regression: a wrong Fe LP mint must fail even with gas caching.

Build the CLI in release mode first. Requires forge, solc, Vyper and Fe 26.5.0
or later, resolved through the normal toolchain cache or EVM_BENCH_*
environment variables.
"""
import json
import shutil
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "target/release/evm-compiler-bench"


def run(root):
    result = subprocess.run(
        [str(BINARY), "--root", str(root), "run", "--offline",
         "--benchmark", "uniswap_v2_pair", "--profile", "fe-latest-O2",
         "--profile", "solc-latest-viair-runs200"],
        text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
    )
    if result.returncode == 0:
        rows = json.loads((root / "results/normalized/results.json").read_text())
        assert rows and all(row["compile"]["status"] == "ok" for row in rows), result.stdout
        fe_rows = [row for row in rows if row["language"] == "fe"]
        assert len(fe_rows) == 24, result.stdout
        assert all(row["correctness"]["baseline_differential_check"] == "pass" for row in fe_rows)
    return result


with tempfile.TemporaryDirectory(prefix="fe-parity-regression-") as directory:
    root = Path(directory)
    for name in ("benches", "compiler-profiles", "schemas", "foundry"):
        shutil.copytree(ROOT / name, root / name,
                        ignore=shutil.ignore_patterns("cache", "out", "GeneratedBench*.t.sol"))
    # Keep test gas/compile caches isolated, while reusing downloaded toolchains.
    (root / ".cache").mkdir()
    (root / ".cache/toolchains").symlink_to(ROOT / ".cache/toolchains", target_is_directory=True)
    (root / "report-ui").mkdir()
    (root / "report-ui/dist").symlink_to(ROOT / "report-ui/dist", target_is_directory=True)
    first = run(root)
    assert first.returncode == 0, first.stdout
    cached = run(root)
    assert cached.returncode == 0, cached.stdout
    assert "behavior: 1 verified pairs cached, 0 pairs to test" in cached.stdout, cached.stdout
    assert "loaded 48 rows from cache" in cached.stdout, cached.stdout

    source = root / "benches/implementations/uniswap_v2_pair/fe/UniswapV2PairReal.fe"
    original = source.read_text()
    expression = "isqrt(amount0 * amount1) - MINIMUM_LIQUIDITY"
    assert original.count(expression) == 1
    source.write_text(original.replace(expression, expression + " + 1"))
    mutated = run(root)
    assert mutated.returncode != 0, "Incorrect LP mint unexpectedly passed"
    # The harness runs forge quietly; rerun the failed shard to see the reasons.
    project = root / "target/foundry-jobs/GeneratedBehaviorShard000.t.sol/foundry"
    shard = subprocess.run(
        ["forge", "test", "--root", str(project), "--match-path",
         "test/GeneratedBehaviorShard000.t.sol", "--match-test", "testDiff_.*_vs_fe_",
         "--evm-version", "prague", "--via-ir", "--optimize"],
        text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
    )
    assert "[FAIL: differential return mismatch]" in shard.stdout, shard.stdout
    print("PASS: correct and cached runs pass; incorrect Fe mint fails parity")
