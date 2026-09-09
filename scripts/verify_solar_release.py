#!/usr/bin/env python3
"""Audit the complete v4 dataset and compare Solar spike and integrated evidence.

Run from the repository root after `cargo run --release -- run`:
    uv run scripts/verify_solar_release.py
Writes a compact release audit to target/solar-release-audit.json.
"""
import collections
import csv
import hashlib
import json
import math
import statistics
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REVISION = "716e9cbcde88165f931173f1c1fda852ed63afa0"
BASELINE = "solc-0.8.36-viair-runs200"
ALIASES = {
    "solar-gas-200": "solar-716e9cbc-gas-runs200",
    "solar-size-1": "solar-716e9cbc-size-runs1",
    "solc-0.8.36-viair-200": BASELINE,
    "solc-0.8.36-legacy-200": "solc-0.8.36-legacy-runs200",
}


def read(path):
    return json.loads((ROOT / path).read_text())


def source_bundle(row):
    directory = (ROOT / row["source_path"]).parent
    return {
        str(path.relative_to(directory)): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in directory.rglob("*.sol")
    }


def compile_artifact(row):
    key = row["cache"]["compile"]["key"]
    return read(f".cache/bench-cli/compile/values/{key}.json")["value"]


def canonical_abi(artifact):
    # Top-level ABI entry order is immaterial; retain nested ordering and all fields.
    return sorted(json.dumps(entry, sort_keys=True) for entry in artifact["abi"])


def main():
    rows = read("results/normalized/results.json")
    manifest = read("results/normalized/run-manifest.json")
    model = read("results/normalized/report-model.json")
    assert manifest["harness_config"]["solc"] == "0.8.34"
    assert not ({"etherscan_api_key", "rpc_endpoints", "etherscan"} & manifest["harness_config"].keys())
    assert manifest["harness_config"]["optimizer"] and manifest["harness_config"]["via_ir"]
    solar = [r for r in rows if r["compiler"]["name"] == "solar"]
    assert len(model["profiles"]) == 127
    assert len({r["benchmark_id"] for r in rows}) == 64
    assert manifest["artifacts"] == 7827 and manifest["compile_failures"] == 296
    assert manifest["gas_records"] == 29367
    assert len(manifest["harness_shards"]) > 0
    source_contexts = {}
    for shard in manifest["harness_shards"]:
        path = ROOT / "foundry" / shard["path"]
        assert hashlib.sha256(path.read_bytes()).hexdigest() == shard["source_sha256"]
        for a in shard["artifacts"]:
            key = (a["benchmark_id"], a["implementation_id"], a["profile_id"])
            assert key not in source_contexts
            source_contexts[key] = shard["source_sha256"]
    assert len(source_contexts) == 7827
    assert len(solar) == 494 and all(r["status"] == "ok" for r in solar)
    for row in solar:
        meta, settings = row["compiler"]["metadata"], row["compiler"]["settings"]
        assert meta["source_revision"] == REVISION
        assert meta["solidity_version"] == "0.8.36"
        assert row["compiler"]["version"] == "0.2.0"
        assert settings["threads"] == 1 and settings["evmVersion"] == "prague"
        assert settings["metadata"] == {"appendCBOR": False, "bytecodeHash": "none"}
        expected_runs = 1 if "-size-" in row["profile_id"] else 200
        assert settings["optimizerRuns"] == expected_runs and "viaIR" not in settings
        for check in ("scenario_status_check", "profile_behavior_check"):
            assert row["correctness"][check] == "pass", (row["benchmark_id"], check)
        assert "fail" not in row["correctness"].values()
    evidence = [e for e in manifest["behavior_checks"] if e["compared_profile"].startswith("solar-")]
    assert len(evidence) == 128
    assert all(e["baseline_profile"] == BASELINE for e in evidence)
    assert sum(e["scenario_count"] for e in evidence) == 494
    assert sum(e["randomized"] for e in evidence) == 10
    assert sum(len(e["properties"]) for e in evidence) == 10

    artifacts = {}
    scenario_rows = {}
    for row in rows:
        if row["status"] != "ok":
            continue
        artifacts.setdefault((row["benchmark_id"], row["profile_id"]), row)
        gas = row["gas"]
        scenario_rows[(row["benchmark_id"], gas["scenario"], gas["state_access_profile"], row["profile_id"])] = row
    abi_pairs = 0
    for (benchmark, profile), row in artifacts.items():
        if not profile.startswith("solar-"):
            continue
        baseline = artifacts[(benchmark, BASELINE)]
        assert source_bundle(row) == source_bundle(baseline), benchmark
        assert row["source_hash"] == baseline["source_hash"]
        assert canonical_abi(compile_artifact(row)) == canonical_abi(compile_artifact(baseline)), benchmark
        abi_pairs += 1
    assert abi_pairs == 128

    spike_dir = ROOT / "docs/spikes/solar-716e9cbc"
    verified_bytecodes = 0
    for old in read("docs/spikes/solar-716e9cbc/artifact-hashes.json"):
        row = artifacts[(old["benchmark_id"], ALIASES[old["profile_id"]])]
        artifact = compile_artifact(row)
        assert old["source_hash"] == row["source_hash"]
        for key in ("creation_bytecode", "runtime_bytecode"):
            digest = hashlib.sha256(bytes.fromhex(artifact[key].removeprefix("0x"))).hexdigest()
            assert old[key + "_sha256"] == digest
        verified_bytecodes += 1
    assert verified_bytecodes == 253
    compared_gas = 0
    offsets = collections.defaultdict(set)
    deploy_offsets = collections.Counter()
    with (spike_dir / "gas.csv").open() as stream:
        for old in csv.DictReader(stream):
            key = (old["benchmark_id"], old["scenario"], old["state_access_profile"], ALIASES[old["profile_id"]])
            new = scenario_rows[key]
            offsets[key[:3]].add(new["gas"]["harness_call_gas"] - int(old["harness_call_gas"]))
            deploy_offsets[new["gas"]["internal_create_gas"] - int(old["internal_create_gas"])] += 1
            compared_gas += 1
    # Different harness compiler versions change wrapper overhead. The runtime
    # offsets are audited separately from compiled-contract identity.
    assert compared_gas == 985 and len(offsets) == 247
    nonuniform_offsets = {"|".join(k): sorted(values) for k, values in offsets.items() if len(values) != 1}
    # Gas-wrapper code is compiled in a different source context from the spike.
    # Preserve all offsets for diagnosis instead of treating them as contract changes.
    with (spike_dir / "size.csv").open() as stream:
        for old in csv.DictReader(stream):
            row = artifacts[(old["benchmark"], ALIASES[old["profile"]])]
            assert int(old["runtime_bytes"]) == row["bytecode"]["runtime_bytes_stripped"]

    def units(profile, metric, suites):
        result = {}
        for row in rows:
            if row["profile_id"] != profile or row["status"] != "ok" or row["suite"] not in suites:
                continue
            key = (row["suite"], row["benchmark_id"])
            if metric in ("harness_call_gas", "internal_create_gas"):
                gas = row["gas"]
                key += (gas["scenario"], gas["state_access_profile"], gas["deployment_variant"])
                value = gas[metric]
            elif metric == "compile_wall_ms":
                value = statistics.median(row["compile"]["wall_ms_samples"])
            else:
                value = row["bytecode"][metric]
            result.setdefault(key, value)
        return result

    stats = {}
    for profile in (ALIASES["solar-gas-200"], ALIASES["solar-size-1"]):
        stats[profile] = {}
        for scope, suites in (("headline", {"fixed", "scale"}), ("all", {"fixed", "scale", "real_derived"})):
            stats[profile][scope] = {}
            for metric in ("harness_call_gas", "runtime_bytes_stripped", "internal_create_gas", "compile_wall_ms"):
                a, b = units(BASELINE, metric, suites), units(profile, metric, suites)
                assert a.keys() == b.keys()
                ratios = [b[k] / a[k] for k in a]
                stats[profile][scope][metric] = {
                    "n": len(ratios),
                    "geomean_delta_pct": 100 * (math.exp(statistics.mean(map(math.log, ratios))) - 1),
                }
    failures = [(r["benchmark_id"], r["profile_id"], r.get("gas", {}).get("scenario"), k)
                for r in rows for k, v in r.get("correctness", {}).items() if v == "fail"]
    failed_artifacts = {(b, p) for b, p, _, _ in failures}
    assert len(failed_artifacts) == 13
    assert all(p.startswith(("vyper-0.4.0-", "vyper-0.5.0a1-", "solc-0.4.26-")) for _, p in failed_artifacts)
    result = {
        "run_id": manifest["run_id"], "profiles": 127,
        "artifacts": manifest["artifacts"], "compile_failures": manifest["compile_failures"],
        "gas_records": manifest["gas_records"], "behavior_pairs": len(manifest["behavior_checks"]),
        "solar_behavior_pairs": len(evidence), "source_and_abi_pairs": abi_pairs,
        "spike_bytecodes_reproduced": verified_bytecodes,
        "spike_gas_rows_compared": compared_gas,
        "spike_runtime_offsets_by_scenario": dict(collections.Counter(next(iter(v)) for v in offsets.values() if len(v) == 1)),
        "spike_nonuniform_runtime_offsets": nonuniform_offsets,
        "spike_deployment_offsets_by_row": dict(deploy_offsets), "historical_failing_artifacts": len(failed_artifacts),
        "solar_correctness": {k: dict(collections.Counter(str(r["correctness"][k]) for r in solar))
                              for k in ("scenario_status_check", "profile_behavior_check", "property_tests", "randomized_differential_check", "log_check")},
        "stats": stats,
    }
    (ROOT / "target/solar-release-audit.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
