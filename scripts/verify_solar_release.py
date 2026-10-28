#!/usr/bin/env python3
"""Audit a full benchmark run's Solar integration against the Solar spike evidence.

Run from the repository root after `cargo run --release -- run`:
    uv run scripts/verify_solar_release.py
Expectations are derived from the run itself and the spike files under
docs/spikes/solar-716e9cbc; the v4 release counts are recorded in
docs/releases/v4.md. Writes a compact audit to target/solar-release-audit.json.
"""
import collections
import csv
import hashlib
import json
import math
import statistics
import tomllib
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
    foundry = tomllib.loads((ROOT / "foundry/foundry.toml").read_text())
    assert manifest["harness_config"]["solc"] == foundry["profile"]["default"]["solc"]
    assert not ({"etherscan_api_key", "rpc_endpoints", "etherscan"} & manifest["harness_config"].keys())
    assert manifest["harness_config"]["optimizer"] and manifest["harness_config"]["via_ir"]
    solar = [r for r in rows if r["compiler"]["name"] == "solar"]
    assert len(model["profiles"]) == len(manifest["profiles"])
    ok_rows = [r for r in rows if r["status"] == "ok"]
    compiled = {(r["benchmark_id"], r["implementation_id"], r["profile_id"]) for r in ok_rows}
    assert manifest["artifacts"] == len(compiled)
    assert manifest["compile_failures"] == sum(r["status"] != "ok" for r in rows)
    assert manifest["gas_records"] == len(ok_rows)
    assert len(manifest["harness_shards"]) > 0
    source_contexts = {}
    for shard in manifest["harness_shards"]:
        path = ROOT / "foundry" / shard["path"]
        assert hashlib.sha256(path.read_bytes()).hexdigest() == shard["source_sha256"]
        for a in shard["artifacts"]:
            key = (a["benchmark_id"], a["implementation_id"], a["profile_id"])
            assert key not in source_contexts
            source_contexts[key] = shard["source_sha256"]
    assert source_contexts.keys() == compiled
    assert solar and all(r["status"] == "ok" for r in solar)
    for row in solar:
        meta, settings = row["compiler"]["metadata"], row["compiler"]["settings"]
        assert meta["source_revision"] == REVISION
        assert meta["solidity_version"] == "0.8.36"
        assert row["compiler"]["version"] == "0.2.0"
        assert settings["threads"] == 1 and settings["evmVersion"] == manifest["evm_version"]
        assert settings["metadata"] == {"appendCBOR": False, "bytecodeHash": "none"}
        expected_runs = 1 if "-size-" in row["profile_id"] else 200
        assert settings["optimizerRuns"] == expected_runs and "viaIR" not in settings
        for check in ("scenario_status_check", "profile_behavior_check"):
            assert row["correctness"][check] == "pass", (row["benchmark_id"], check)
        assert "fail" not in row["correctness"].values()
    benchmarks = {r["benchmark_id"] for r in rows}
    solar_profiles = {r["profile_id"] for r in solar}
    assert solar_profiles == {ALIASES["solar-gas-200"], ALIASES["solar-size-1"]}
    for profile in solar_profiles | {BASELINE}:
        assert {r["benchmark_id"] for r in ok_rows if r["profile_id"] == profile} == benchmarks, profile
    solar_artifacts = {(r["benchmark_id"], r["profile_id"]) for r in solar}
    evidence = [e for e in manifest["behavior_checks"] if e["compared_profile"].startswith("solar-")]
    assert {(e["benchmark_id"], e["compared_profile"]) for e in evidence} == solar_artifacts
    assert all(e["baseline_profile"] == BASELINE for e in evidence)
    assert sum(e["scenario_count"] for e in evidence) == len(solar)

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
    assert abi_pairs == len(solar_artifacts)

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
    # Observed failures elsewhere in the matrix are reported; none may affect the audited pairs.
    assert not {p for _, p in failed_artifacts} & (solar_profiles | {BASELINE})
    result = {
        "run_id": manifest["run_id"], "profiles": len(manifest["profiles"]), "benchmarks": len(benchmarks),
        "artifacts": manifest["artifacts"], "compile_failures": manifest["compile_failures"],
        "gas_records": manifest["gas_records"], "behavior_pairs": len(manifest["behavior_checks"]),
        "solar_behavior_pairs": len(evidence), "source_and_abi_pairs": abi_pairs,
        "solar_scenarios": len(solar),
        "solar_randomized_checks": sum(e["randomized"] for e in evidence),
        "solar_property_checks": sum(len(e["properties"]) for e in evidence),
        "spike_bytecodes_reproduced": verified_bytecodes,
        "spike_gas_rows_compared": compared_gas,
        "spike_runtime_offsets_by_scenario": dict(collections.Counter(next(iter(v)) for v in offsets.values() if len(v) == 1)),
        "spike_nonuniform_runtime_offsets": nonuniform_offsets,
        "spike_deployment_offsets_by_row": dict(deploy_offsets),
        "failing_artifacts": sorted("|".join(a) for a in failed_artifacts),
        "solar_correctness": {k: dict(collections.Counter(str(r["correctness"][k]) for r in solar))
                              for k in ("scenario_status_check", "profile_behavior_check", "property_tests", "randomized_differential_check", "log_check")},
        "stats": stats,
    }
    (ROOT / "target/solar-release-audit.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
