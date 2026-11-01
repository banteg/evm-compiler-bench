"""Exercise the factory correctness gate with local toolchains and a built CLI."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else ROOT / "target/release/evm-compiler-bench"


def run(root, **environment):
    return subprocess.run(
        [str(BINARY), "--root", str(root), "run", "--offline", "--benchmark",
         "uniswap_v2_factory", "--profile", "solc-latest-legacy-runs200",
         "--profile", "vyper-latest-gas"],
        env={**os.environ, "FOUNDRY_OFFLINE": "true", **environment}, text=True, stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )


with tempfile.TemporaryDirectory(prefix="factory-parity-") as tmp:
    root = Path(tmp)
    for name in ("benches", "compiler-profiles", "schemas", "foundry"):
        shutil.copytree(ROOT / name, root / name,
                        ignore=shutil.ignore_patterns("cache", "out", "Generated*.t.sol"))
    (root / ".cache").mkdir()
    (root / ".cache/toolchains").symlink_to(ROOT / ".cache/toolchains", target_is_directory=True)
    (root / "report-ui").mkdir()
    (root / "report-ui/dist").symlink_to(ROOT / "report-ui/dist", target_is_directory=True)
    valid = run(root)
    assert valid.returncode == 0, valid.stdout
    rows = json.loads((root / "results/normalized/results.json").read_text())
    creation = [r for r in rows if r["gas"]["scenario"].startswith("create_pair")]
    assert len(creation) == 4
    assert all(r["correctness"]["golden_behavior_check"] == "pass" for r in creation)
    assert all(r["correctness"]["baseline_differential_check"] == "pass" for r in creation)

    filtered = run(root, FOUNDRY_NO_MATCH_TEST="^testDiff_")
    assert filtered.returncode != 0, "Filtered-out behavior tests were credited"
    assert "Forge" in filtered.stdout and "GeneratedBehaviorShard" in filtered.stdout, filtered.stdout
    evidence = root / "results/raw/behavior-checks.json"
    assert json.loads(evidence.read_text()) == []

    vyper = root / "benches/implementations/uniswap_v2_factory/vyper/UniswapV2FactoryReal.vy"
    original = vyper.read_text()
    for old, new in [
        ("    extcall Pair(pair).initialize(token0, token1)\n", ""),
        ("    return pair\n", "    return empty(address)\n"),
    ]:
        assert original.count(old) == 1
        vyper.write_text(original.replace(old, new))
        mutant = run(root)
        assert mutant.returncode != 0 and "scenario postcondition failed" in mutant.stdout, mutant.stdout
        raw = [json.loads(line) for line in (root / "results/raw/foundry-gas.jsonl").read_text().splitlines()]
        fails = [r for r in raw if r["golden_check_passed"] is False]
        assert {(r["profile_id"], r["scenario"]) for r in fails} == {
            ("vyper-latest-gas", "create_pair"), ("vyper-latest-gas", "create_pair_reverse_order"),
        }
        assert json.loads(evidence.read_text()) == []
    print("PASS: filtered shard and both incorrect factory implementations rejected")
