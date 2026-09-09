#!/usr/bin/env python3
"""Isolated Solar feasibility spike, using the existing sources and Foundry checks.

Requires a full benchmark run with solc 0.8.36 in the local compile cache and
Solar main at the revision below built with `cargo +1.96.0 build --release
--locked -p solar-compiler --bin solar` in target/solar-spike/upstream.
No production profiles or result files are modified.
"""
import argparse
import concurrent.futures
import copy
import csv
import hashlib
import json
import math
import os
import platform
from pathlib import Path
import re
import shutil
import statistics
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
WORK = ROOT / "target/solar-spike"
UPSTREAM = WORK / "upstream"
REVISION = "716e9cbcde88165f931173f1c1fda852ed63afa0"
SOLAR = UPSTREAM / "target/release/solar"
SOLC = ROOT / ".cache/toolchains/solc/0.8.36/solc-macosx-amd64-v0.8.36+commit.8a079791"
SOLC_SHA256 = "d4abcf0b3e24b7948ddfd64c374d26c3214648717777790ecb936979054a129d"
PROFILES = {
    "solc-0.8.36-legacy-200": ("solc", False, 200),
    "solc-0.8.36-viair-200": ("solc", True, 200),
    "solar-gas-200": ("solar", None, 200),
    "solar-size-1": ("solar", None, 1),
}
BASELINE = "solc-0.8.36-viair-200"


def sha(data):
    return hashlib.sha256(data).hexdigest()


def dump(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2) + "\n")


def prepare():
    rows = json.loads((ROOT / "results/normalized/results.json").read_text())
    selected = {}
    for row in rows:
        if row["profile_id"] == "solc-latest-viair-runs200":
            selected.setdefault(row["benchmark_id"], row)
    assert len(selected) == 64, "Generate the complete baseline matrix first"
    cases = []
    for bench, row in sorted(selected.items()):
        assert row["compiler"]["version"] == "0.8.36"
        cache = ROOT / ".cache/bench-cli/compile/values" / (row["cache"]["compile"]["key"] + ".json")
        entry = json.loads(cache.read_text())
        assert entry["kind"] == "artifact", bench
        artifact = entry["value"]
        source = Path(artifact["source_path"])
        assert sha(source.read_bytes()) == artifact["source_hash"], bench
        sources = {}
        for path in sorted(source.parent.rglob("*.sol")):
            name = str(path.relative_to(source.parent))
            sources[name] = {"content": path.read_text()}
            out = WORK / "sources" / bench / name
            out.parent.mkdir(parents=True, exist_ok=True)
            out.write_bytes(path.read_bytes())
        scenario = ROOT / (artifact["scenario_path"] or f"benches/scenarios/{bench}.yaml")
        scenario_dest = WORK / "scenarios" / f"{bench}.yaml"
        scenario_dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(scenario, scenario_dest)
        artifact["source_path"] = str(WORK / "sources" / bench / source.name)
        artifact["scenario_path"] = str(scenario_dest)
        artifact["scenario_hash"] = sha(scenario_dest.read_bytes())
        cases.append({"benchmark": bench, "source_name": source.name,
                      "sources": sources, "template": artifact})
    dump(WORK / "cases.json", cases)
    dump(WORK / "input-provenance.json", {
        "repository_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "baseline_run": json.loads((ROOT / "results/normalized/run-manifest.json").read_text())["run_id"],
        "sources": {c["benchmark"]: {name: sha(s["content"].encode()) for name, s in c["sources"].items()} for c in cases},
        "scenarios": {c["benchmark"]: c["template"]["scenario_hash"] for c in cases},
        "harness_sources": {str(p.relative_to(ROOT)): sha(p.read_bytes()) for p in
                            (ROOT / "crates/bench-cli/src").rglob("*") if p.is_file()},
    })
    print(f"Prepared {len(cases)} identical source bundles", flush=True)


def compile_cases(only=None, repeats=1):
    assert subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=UPSTREAM, text=True).strip() == REVISION
    assert not subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=no"], cwd=UPSTREAM)
    assert sha(SOLC.read_bytes()) == SOLC_SHA256
    compilers = {}
    for name, binary in [("solar", SOLAR), ("solc", SOLC)]:
        compilers[name] = {
            "name": name, "version": f"0.2.0-dev+{REVISION[:12]}" if name == "solar" else "0.8.36",
            "binary_path": str(binary), "binary_sha256": sha(binary.read_bytes()),
            "version_output": subprocess.check_output([binary, "--version"], text=True),
            "download_source": f"https://github.com/paradigmxyz/solar/tree/{REVISION}" if name == "solar" else
                "https://binaries.soliditylang.org/macosx-amd64/" + SOLC.name,
            "metadata": {"revision": REVISION, "build_profile": "release", "rust": "1.96.0"} if name == "solar" else {},
        }
    assert REVISION in compilers["solar"]["version_output"], "Solar binary does not match the source revision"
    dump(WORK / "toolchains.json", compilers)
    dump(WORK / "environment.json", {
        "platform": platform.platform(), "machine": platform.machine(),
        "python": platform.python_version(),
        "rust_build": subprocess.check_output(["rustc", "+1.96.0", "--version"], text=True).strip(),
        "forge_version": subprocess.check_output(["forge", "--version"], text=True),
        "solar_build_command": "cargo +1.96.0 build --release --locked -p solar-compiler --bin solar",
        "solar_source_clean": True, "repeated_timing_architecture": "x86_64",
        "compiler_architecture_command": "/usr/bin/arch -x86_64",
    })
    artifacts, failures, coverage = [], [], []
    for case in json.loads((WORK / "cases.json").read_text()):
        bench = case["benchmark"]
        if only and bench not in only:
            continue
        for profile, (compiler, via_ir, runs) in PROFILES.items():
            settings = {"evmVersion": "prague", "optimizer": {"enabled": True, "runs": runs},
                        "metadata": {"appendCBOR": False, "bytecodeHash": "none"},
                        "outputSelection": {"*": {"*": ["abi", "evm.bytecode.object", "evm.deployedBytecode.object"]}}}
            if via_ir is not None:
                settings["viaIR"] = via_ir
            payload = {"language": "Solidity", "sources": case["sources"], "settings": settings}
            dest = WORK / "compiles" / bench / profile
            dump(dest / "input.json", payload)
            # solc is universal while this Rust toolchain targets x86_64. Use the
            # same architecture and launcher for both when comparing wall time.
            command = ["/usr/bin/arch", "-x86_64", compilers[compiler]["binary_path"], "--standard-json"]
            if compiler == "solar":
                command += ["--threads", "1", "--color", "never"]
            timings, outputs = [], []
            try:
                for repeat in range(repeats):
                    start = time.perf_counter()
                    result = subprocess.run(command, input=json.dumps(payload), capture_output=True, text=True, timeout=180)
                    timings.append((time.perf_counter() - start) * 1000)
                    (dest / f"output-{repeat}.json").write_text(result.stdout)
                    (dest / f"stderr-{repeat}.txt").write_text(result.stderr)
                    output = json.loads(result.stdout) if result.stdout else {}
                    errors = [e for e in output.get("errors", []) if e.get("severity") == "error"]
                    if result.returncode or errors:
                        raise ValueError(json.dumps({"exit_code": result.returncode, "errors": errors, "stderr": result.stderr}))
                    contract = output.get("contracts", {}).get(case["source_name"], {}).get(case["template"]["contract_name"], {})
                    creation = contract.get("evm", {}).get("bytecode", {}).get("object", "")
                    runtime = contract.get("evm", {}).get("deployedBytecode", {}).get("object", "")
                    if not creation or not runtime:
                        raise ValueError("Missing creation or runtime bytecode despite successful exit")
                    bytes.fromhex(creation)
                    bytes.fromhex(runtime)
                    outputs.append((creation, runtime, contract["abi"]))
                assert all(o == outputs[0] for o in outputs), "Non-deterministic compilation"
            except (ValueError, KeyError, subprocess.TimeoutExpired, AssertionError) as error:
                failure = {"benchmark": bench, "profile": profile, "error": str(error), "wall_ms_samples": timings}
                failures.append(failure)
                coverage.append({"benchmark": bench, "profile": profile, "status": "compile_error"})
                dump(dest / "status.json", failure)
                print("FAIL", bench, profile, str(error)[:200], flush=True)
                continue
            creation, runtime, abi = outputs[0]
            if profile == BASELINE:
                assert creation == case["template"]["creation_bytecode"], bench
                assert runtime == case["template"]["runtime_bytecode"], bench
            a, b = len(creation) // 2, len(runtime) // 2
            artifact = copy.deepcopy(case["template"])
            artifact.update(profile_id=profile, compiler=compilers[compiler], abi=abi,
                            creation_bytecode=creation, runtime_bytecode=runtime,
                            compiler_settings={**settings, "threads": 1} if compiler == "solar" else settings,
                            compile={"wall_ms_samples": timings, "cpu_ms_samples": [], "peak_rss_kib": 0},
                            bytecode={"creation_bytes": a, "creation_bytes_stripped": a, "runtime_bytes": b,
                                      "runtime_bytes_stripped": b, "initcode_bytes": a, "linked_runtime_bytes": b,
                                      "eip170_margin_bytes": 24576-b, "eip3860_margin_bytes": 49152-a,
                                      "code_deposit_gas": 200*b}, cache={"status": "disabled", "key": "", "invalidated_by": []})
            artifacts.append(artifact)
            status = {"benchmark": bench, "profile": profile, "status": "ok", "runtime_bytes": b,
                      "creation_bytes": a, "wall_ms_samples": timings, "input_sha256": sha(json.dumps(payload, sort_keys=True).encode()),
                      "creation_sha256": sha(bytes.fromhex(creation)), "runtime_sha256": sha(bytes.fromhex(runtime))}
            coverage.append(status)
            dump(dest / "status.json", status)
            print("OK", bench, profile, "runtime", b, "compile_ms", round(statistics.median(timings), 2), flush=True)
    suffix = "-selected" if only else ""
    dump(WORK / f"artifacts{suffix}.json", artifacts)
    dump(WORK / f"compile-failures{suffix}.json", failures)
    dump(WORK / f"coverage{suffix}.json", coverage)


def build_bridge():
    project = WORK / "bridge"
    (project / "src").mkdir(parents=True, exist_ok=True)
    cargo = (ROOT / "crates/bench-cli/Cargo.toml").read_text()
    cargo = cargo.replace('name = "evm-compiler-bench"', 'name = "solar-spike"')
    cargo = cargo.replace('edition.workspace = true', 'edition = "2024"').replace('license.workspace = true', 'license = "MIT"')
    cargo = cargo.replace('repository.workspace = true', '')
    (project / "Cargo.toml").write_text(cargo + "\n[workspace]\n")
    (project / "Cargo.lock").write_text((ROOT / "Cargo.lock").read_text().replace('name = "evm-compiler-bench"', 'name = "solar-spike"'))
    modules = "#![allow(dead_code)]\n" + "\n".join(
        f'#[path = "{ROOT}/crates/bench-cli/src/{m}.rs"] mod {m};'
        for m in ("cache", "harness", "models", "runner", "scenarios", "util", "foundry_jobs"))
    bridge = r'''
mod baselines {
    use crate::models::CompiledArtifact;
    use std::collections::BTreeMap;
    pub fn comparison_pairs(a: &[CompiledArtifact]) -> Vec<(String, usize, usize)> {
        a.iter().enumerate().filter(|(_, x)| x.compiler.name == "solar").filter_map(|(i, x)| {
            a.iter().position(|y| y.benchmark_id == x.benchmark_id && y.profile_id == "solc-0.8.36-viair-200")
                .map(|j| (x.benchmark_id.clone(), j, i))
        }).collect()
    }
    pub fn baseline_pairs(a: &[CompiledArtifact]) -> BTreeMap<String, (usize, usize)> {
        comparison_pairs(a).into_iter().map(|(name, left, right)| (name, (left, right))).collect()
    }
}
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let root = std::path::Path::new(&args[1]);
    let artifacts = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    let compiled = models::CompileSet { profiles: vec![], artifacts, failures: vec![] };
    let catalog = scenarios::load_scenario_catalog(root, None, &[])?;
    let gas = runner::run_foundry(root, "prague", &compiled, &catalog, false)?;
    std::fs::write(root.join("gas.json"), serde_json::to_vec_pretty(&gas)?)?;
    Ok(())
}
'''
    (project / "src/main.rs").write_text(modules + bridge)
    subprocess.run(["cargo", "build", "--release", "--offline", "--manifest-path", str(project / "Cargo.toml"),
                    "--target-dir", str(ROOT / "target")], check=True)


def measure(only=None):
    artifacts = json.loads((WORK / "artifacts.json").read_text())
    groups = {}
    for artifact in artifacts:
        groups.setdefault(artifact["benchmark_id"], []).append(artifact)
    selected = [(b, a) for b, a in groups.items() if (not only or b in only)
                and any(x["compiler"]["name"] == "solar" for x in a)
                and any(x["profile_id"] == BASELINE for x in a)]

    def run_case(item):
        bench, group = item
        run = WORK / "runs" / bench
        (run / "foundry/test").mkdir(parents=True, exist_ok=True)
        (run / "benches/scenarios").mkdir(parents=True, exist_ok=True)
        shutil.copyfile(WORK / "scenarios" / f"{bench}.yaml", run / "benches/scenarios" / f"{bench}.yaml")
        config = (ROOT / "foundry/foundry.toml").read_text() + f'\nsolc = "{SOLC}"\n'
        (run / "foundry/foundry.toml").write_text(config)
        dump(run / "artifacts.json", group)
        start = time.monotonic()
        with (run / "run.log").open("w") as log:
            try:
                result = subprocess.run([ROOT / "target/release/solar-spike", run, run / "artifacts.json"],
                                        stdout=log, stderr=subprocess.STDOUT, timeout=600,
                                        env=dict(os.environ, EVM_BENCH_FOUNDRY_JOBS="1"))
                code = result.returncode
            except subprocess.TimeoutExpired:
                code = "timeout"
        status = {"benchmark": bench, "exit_code": code, "wall_seconds": time.monotonic()-start,
                  "profiles": [a["profile_id"] for a in group]}
        dump(run / "status.json", status)
        print("BEHAVIOR", bench, code, round(status["wall_seconds"], 1), "s", flush=True)
        return status

    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        results = list(pool.map(run_case, selected))
    dump(WORK / ("runtime-status-selected.json" if only else "runtime-status.json"), results)


def summarize():
    artifacts = json.loads((WORK / "artifacts.json").read_text())
    coverage = json.loads((WORK / "coverage.json").read_text())
    statuses = json.loads((WORK / "runtime-status.json").read_text())
    by_artifact = {(a["benchmark_id"], a["profile_id"]): a for a in artifacts}
    abi_matches = 0
    normalize_abi = lambda abi: sorted(json.dumps(entry, sort_keys=True) for entry in abi)
    for artifact in artifacts:
        if artifact["compiler"]["name"] == "solar":
            reference = by_artifact[artifact["benchmark_id"], BASELINE]
            assert normalize_abi(artifact["abi"]) == normalize_abi(reference["abi"]), artifact["benchmark_id"]
            abi_matches += 1
    gas, failed_runtime, passed = [], [], []
    test_counts = {"gas": 0, "differential": 0, "randomized": 0, "property": 0}
    for status in statuses:
        run = WORK / "runs" / status["benchmark"]
        if status["exit_code"] != 0:
            failed_runtime.append(status)
            continue
        rows = json.loads((run / "gas.json").read_text())
        if not all(r["scenario_status_ok"] for r in rows):
            failed_runtime.append({**status, "error": "unexpected scenario outcome"})
            continue
        gas.extend(rows)
        passed.append(status["benchmark"])
        for path in (run / "foundry/test").glob("GeneratedBenchShard*.t.sol"):
            for name in re.findall(r"function (test\w+)\(", path.read_text()):
                category = next((k for prefix, k in [("testGas", "gas"), ("testDiff", "differential"),
                                                     ("testRandom", "randomized"), ("testProperty", "property")]
                                 if name.startswith(prefix)), None)
                if category:
                    test_counts[category] += 1
    # The scenario catalog enforces unique names; each name fixes its deployment variant.
    gas_key = lambda r: (r["benchmark_id"], r["scenario"], r["state_access_profile"], r["profile_id"])
    lookup = {gas_key(r): r for r in gas}
    assert len(lookup) == len(gas), "duplicate gas rows"
    geomean = lambda values: math.exp(statistics.mean(math.log(x) for x in values)) if values else None
    comparisons = {}
    for profile in ["solar-gas-200", "solar-size-1"]:
        paired = [r for r in gas if r["profile_id"] == profile and (*gas_key(r)[:-1], BASELINE) in lookup]
        gas_ratios = [r["harness_call_gas"] / lookup[(*gas_key(r)[:-1], BASELINE)]["harness_call_gas"] for r in paired]
        size_ratios, per_benchmark = [], []
        for bench in sorted({r["benchmark_id"] for r in paired}):
            a, b = by_artifact[bench, BASELINE], by_artifact[bench, profile]
            assert a["source_hash"] == b["source_hash"]
            ratio = b["bytecode"]["runtime_bytes"] / a["bytecode"]["runtime_bytes"]
            size_ratios.append(ratio)
            rows = [r for r in paired if r["benchmark_id"] == bench]
            ratios = [r["harness_call_gas"] / lookup[(*gas_key(r)[:-1], BASELINE)]["harness_call_gas"] for r in rows]
            per_benchmark.append({"benchmark": bench, "scenarios": len(rows),
                                  "gas_geomean_ratio": geomean(ratios), "runtime_bytes_ratio": ratio,
                                  "baseline_runtime_bytes": a["bytecode"]["runtime_bytes"],
                                  "solar_runtime_bytes": b["bytecode"]["runtime_bytes"]})
        comparisons[profile] = {"benchmarks": len(size_ratios), "scenarios": len(gas_ratios),
                                "gas_geomean_ratio": geomean(gas_ratios),
                                "runtime_bytes_geomean_ratio": geomean(size_ratios),
                                "per_benchmark": per_benchmark}
    summary = {"solar_revision": REVISION, "baseline": BASELINE, "evm_version": "prague",
               "compile_coverage": {p: {"ok": sum(c["profile"] == p and c["status"] == "ok" for c in coverage),
                                         "failed": sum(c["profile"] == p and c["status"] != "ok" for c in coverage)} for p in PROFILES},
               "runtime_passed_benchmarks": passed, "runtime_failures": failed_runtime,
               "verified_gas_records": len(gas), "full_abi_matches": abi_matches,
               "test_counts": test_counts, "comparisons": comparisons,
               "measurement_note": "Gas includes harness overhead and expected-revert scenarios. Ratios use only whole benchmarks whose generated harness passed. Compilation failures and runtime failures remain separate; no performance credit for invalid cases."}
    repeated = WORK / "artifacts-selected.json"
    if repeated.exists():
        timings = json.loads(repeated.read_text())
        with (WORK / "compile-times.csv").open("w") as f:
            writer = csv.writer(f, lineterminator="\n")
            writer.writerow(["benchmark", "profile", "samples", "median_wall_ms", "wall_ms_samples"])
            for a in timings:
                original = by_artifact[a["benchmark_id"], a["profile_id"]]
                assert a["creation_bytecode"] == original["creation_bytecode"]
                assert a["runtime_bytecode"] == original["runtime_bytecode"]
                samples = a["compile"]["wall_ms_samples"]
                writer.writerow([a["benchmark_id"], a["profile_id"], len(samples), statistics.median(samples), json.dumps(samples)])
        summary["repeated_compilation"] = {"artifacts": len(timings),
            "invocations": sum(len(a["compile"]["wall_ms_samples"]) for a in timings),
            "bytecode_matches_initial_run": True}
    dump(WORK / "summary.json", summary)
    with (WORK / "gas.csv").open("w") as f:
        fields = ["benchmark_id", "scenario", "state_access_profile", "profile_id",
                  "expected_success", "call_succeeded", "scenario_status_ok", "harness_call_gas", "internal_create_gas",
                  "return_hash", "observer_hash", "log_hash"]
        writer = csv.DictWriter(f, fields, extrasaction="ignore", lineterminator="\n")
        writer.writeheader()
        writer.writerows(sorted(gas, key=gas_key))
    with (WORK / "size.csv").open("w") as f:
        writer = csv.writer(f, lineterminator="\n")
        writer.writerow(["benchmark", "profile", "creation_bytes", "runtime_bytes", "runtime_checks"])
        for a in artifacts:
            writer.writerow([a["benchmark_id"], a["profile_id"], a["bytecode"]["creation_bytes"],
                             a["bytecode"]["runtime_bytes"], "pass" if a["benchmark_id"] in passed else "not_passed"])
    print(json.dumps({k: v for k, v in summary.items() if k != "comparisons"}, indent=2))
    for p, comparison in comparisons.items():
        print(p, {k: v for k, v in comparison.items() if k != "per_benchmark"})


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("stage", choices=["prepare", "compile", "bridge", "measure", "summarize"])
    parser.add_argument("--benchmark", action="append")
    parser.add_argument("--repeats", type=int, default=1)
    args = parser.parse_args()
    {"prepare": prepare, "bridge": build_bridge, "summarize": summarize,
     "compile": lambda: compile_cases(args.benchmark, args.repeats),
     "measure": lambda: measure(args.benchmark)}[args.stage]()
