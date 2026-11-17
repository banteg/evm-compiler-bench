#!/usr/bin/env python3
"""Audit a full benchmark run's Solar integration and report statistics.

Run from the repository root after `cargo run --release -- run`:
    uv run scripts/verify_solar_release.py
Requires the locally generated harness sources and compile cache from that run.
For the v4 revision, also compare bytecode and wrapper overhead against the
pinned evidence under docs/spikes/solar-716e9cbc. Writes a compact audit to
target/solar-release-audit.json.
"""
import collections
import csv
import hashlib
import json
import subprocess
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SPIKE_REVISION = "716e9cbcde88165f931173f1c1fda852ed63afa0"
# Trusted integration targets. The legacy spike is an exact bytecode check only
# for its pinned compiler/source generation, not a baseline for later releases.
RELEASES = {
    SPIKE_REVISION: "0.8.36",
    "cb38e98632eebdf6c5bb60a792b83320c1312b28": "0.8.38",
}


def read(path):
    return json.loads((ROOT / path).read_text())


def source_bundle(row):
    source = ROOT / row["source_path"]
    assert source.is_file(), f"Missing materialized source {source}"
    directory = source.parent
    return {
        str(path.relative_to(directory)): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in directory.rglob("*.sol")
    }


def compile_artifact(row):
    key = row["cache"]["compile"]["key"]
    assert key, "Solar release verification requires a cache-enabled run"
    return read(f".cache/bench-cli/compile/values/{key}.json")["value"]


def canonical_abi(artifact):
    # Top-level ABI entry order is immaterial; retain nested ordering and all fields.
    return sorted(json.dumps(entry, sort_keys=True) for entry in artifact["abi"])


def verify_v4_spike(artifacts, scenario_rows):
    aliases = {
        "solar-gas-200": "solar-716e9cbc-gas-runs200",
        "solar-size-1": "solar-716e9cbc-size-runs1",
        "solc-0.8.36-viair-200": "solc-0.8.36-viair-runs200",
        "solc-0.8.36-legacy-200": "solc-0.8.36-legacy-runs200",
    }
    spike_dir = ROOT / "docs/spikes/solar-716e9cbc"
    verified_bytecodes = 0
    for old in read("docs/spikes/solar-716e9cbc/artifact-hashes.json"):
        row = artifacts[(old["benchmark_id"], aliases[old["profile_id"]])]
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
            key = (old["benchmark_id"], old["scenario"], old["state_access_profile"], aliases[old["profile_id"]])
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
            row = artifacts[(old["benchmark"], aliases[old["profile"]])]
            assert int(old["runtime_bytes"]) == row["bytecode"]["runtime_bytes_stripped"]
    return {
        "spike_bytecodes_reproduced": verified_bytecodes,
        "spike_gas_rows_compared": compared_gas,
        "spike_runtime_offsets_by_scenario": dict(collections.Counter(next(iter(v)) for v in offsets.values() if len(v) == 1)),
        "spike_nonuniform_runtime_offsets": nonuniform_offsets,
        "spike_deployment_offsets_by_row": dict(deploy_offsets),
    }


def main():
    rows = read("results/normalized/results.json")
    manifest = read("results/normalized/run-manifest.json")
    model = read("results/normalized/report-model.json")
    foundry = tomllib.loads((ROOT / "foundry/foundry.toml").read_text())
    assert model["manifest"] == manifest, "Run manifest differs from the report model"
    args = manifest["environment"]["command_line"]
    assert not any(arg.split("=")[0] in ("--profile", "--benchmark") for arg in args), "Expected unfiltered run"
    assert manifest["harness_config"]["solc"] == foundry["profile"]["default"]["solc"]
    assert not ({"etherscan_api_key", "rpc_endpoints", "etherscan"} & manifest["harness_config"].keys())
    assert manifest["harness_config"]["optimizer"] and manifest["harness_config"]["via_ir"]
    solar = [r for r in rows if r["compiler"]["name"] == "solar"]
    assert len(model["profiles"]) == len(manifest["profiles"])
    del model
    ok_rows = [r for r in rows if r["status"] == "ok"]
    compiled = {(r["benchmark_id"], r["implementation_id"], r["profile_id"]) for r in ok_rows}
    assert manifest["artifacts"] == len(compiled)
    assert manifest["compile_failures"] == sum(r["status"] != "ok" for r in rows)
    assert manifest["gas_records"] == len(ok_rows)
    assert len(manifest["harness_shards"]) > 0
    source_contexts = {}
    for shard in manifest["harness_shards"]:
        path = ROOT / "foundry" / shard["path"]
        assert hashlib.sha256(path.read_bytes()).hexdigest() == shard["source_sha256"], (
            f"{path} does not match the generated harness for run {manifest['run_id']}"
        )
        for a in shard["artifacts"]:
            key = (a["benchmark_id"], a["implementation_id"], a["profile_id"])
            assert key not in source_contexts
            source_contexts[key] = shard["source_sha256"]
    assert source_contexts.keys() == compiled
    assert solar and all(r["status"] == "ok" for r in solar)
    revisions = {r["compiler"]["metadata"]["source_revision"] for r in solar}
    assert len(revisions) == 1, revisions
    (revision,) = revisions
    assert revision in RELEASES, f"Unrecognized Solar release revision {revision}"
    compatibility = RELEASES[revision]
    baseline_profile = f"solc-{compatibility}-viair-runs200"
    for row in solar:
        meta, settings = row["compiler"]["metadata"], row["compiler"]["settings"]
        assert meta["source_revision"] == revision
        assert meta["solidity_version"] == compatibility
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
    assert solar_profiles == {f"solar-{revision[:8]}-gas-runs200", f"solar-{revision[:8]}-size-runs1"}
    # Verify raw scenario coverage before any score-side exclusions. Oversize
    # deployments and missing optional timings may reduce the scored panel.
    coverage = collections.defaultdict(set)
    audited_profiles = solar_profiles | {baseline_profile}
    for row in ok_rows:
        if row["profile_id"] not in audited_profiles:
            continue
        gas = row["gas"]
        unit = (row["benchmark_id"], gas["scenario"], gas["state_access_profile"], gas["deployment_variant"])
        profile_units = coverage[row["profile_id"]]
        assert unit not in profile_units, ("duplicate scenario", row["profile_id"], unit)
        profile_units.add(unit)
    assert all(coverage[p] == coverage[baseline_profile] for p in solar_profiles)
    for profile in solar_profiles | {baseline_profile}:
        assert {r["benchmark_id"] for r in ok_rows if r["profile_id"] == profile} == benchmarks, profile
    solar_artifacts = {(r["benchmark_id"], r["profile_id"]) for r in solar}
    evidence = [e for e in manifest["behavior_checks"] if e["compared_profile"].startswith("solar-")]
    assert {(e["benchmark_id"], e["compared_profile"]) for e in evidence} == solar_artifacts
    assert all(e["baseline_profile"] == baseline_profile for e in evidence)
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
        baseline = artifacts[(benchmark, baseline_profile)]
        assert source_bundle(row) == source_bundle(baseline), benchmark
        assert row["source_hash"] == baseline["source_hash"]
        assert canonical_abi(compile_artifact(row)) == canonical_abi(compile_artifact(baseline)), benchmark
        abi_pairs += 1
    assert abi_pairs == len(solar_artifacts)

    spike = verify_v4_spike(artifacts, scenario_rows) if revision == SPIKE_REVISION else None
    stats = json.loads(subprocess.check_output([
        "node", str(ROOT / "scripts/solar-report-stats.mjs"),
        str(ROOT / "results/normalized/report-model.json"), baseline_profile, *sorted(solar_profiles),
    ]))
    failures = [(r["benchmark_id"], r["profile_id"], r.get("gas", {}).get("scenario"), k)
                for r in rows for k, v in r.get("correctness", {}).items() if v == "fail"]
    failed_artifacts = {(b, p) for b, p, _, _ in failures}
    # Observed failures elsewhere in the matrix are reported; none may affect the audited pairs.
    assert not {p for _, p in failed_artifacts} & (solar_profiles | {baseline_profile})
    result = {
        "run_id": manifest["run_id"], "profiles": len(manifest["profiles"]), "benchmarks": len(benchmarks),
        "artifacts": manifest["artifacts"], "compile_failures": manifest["compile_failures"],
        "gas_records": manifest["gas_records"], "behavior_pairs": len(manifest["behavior_checks"]),
        "solar_behavior_pairs": len(evidence), "source_and_abi_pairs": abi_pairs,
        "solar_scenarios": len(solar),
        "solar_randomized_checks": sum(e["randomized"] for e in evidence),
        "solar_property_checks": sum(len(e["properties"]) for e in evidence),
        "solar_revision": revision, "baseline": baseline_profile,
        "size_evidence": "measured" if any(
            isinstance(r["bytecode"].get("within_size_limits"), bool) for r in ok_rows
        ) else "legacy_template_sizes",
        "v4_spike": spike,
        "failing_artifacts": sorted("|".join(a) for a in failed_artifacts),
        "solar_correctness": {k: dict(collections.Counter(str(r["correctness"][k]) for r in solar))
                              for k in ("scenario_status_check", "profile_behavior_check", "property_tests", "randomized_differential_check", "log_check")},
        "stats": stats,
    }
    (ROOT / "target/solar-release-audit.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
