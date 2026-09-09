#!/usr/bin/env python3
"""Isolated solx gas/size spike; requires verified solx and solc in WORK/bin.

Run from the repository root: python3 scripts/solx_spike.py
All generated inputs, outputs, and Foundry tests stay in target/solx-spike.
The Rust bridge reuses the repository's gas/scenario/property harness verbatim,
substituting a same-source compiler pair for its Solidity/Vyper baseline pair.
"""
import hashlib
import csv
import json
import os
from pathlib import Path
import re
import shutil
import statistics
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
WORK = ROOT / "target/solx-spike"
CASES = {
    "erc20_minimal": ("Erc20Minimal", "Erc20Minimal.sol"),
    "vault_deposit_withdraw": ("VaultDepositWithdraw", "VaultDepositWithdraw.sol"),
    "amm_pair_subset": ("AmmPairSubset", "AmmPairSubset.sol"),
    "merkle_verifier": ("MerkleVerifier", "MerkleVerifier.sol"),
    "uniswap_v2_pair": ("UniswapV2Pair", "latest/UniswapV2PairReal.sol"),
}
PROFILES = {
    "solc-legacy-200": ("solc", False, {"enabled": True, "runs": 200}),
    "solc-viair-200": ("solc", True, {"enabled": True, "runs": 200}),
    "solx-O3": ("solx", False, {"mode": "3", "sizeFallback": False}),
    "solx-Oz": ("solx", False, {"mode": "z", "sizeFallback": False}),
}
BINARIES = {
    "solc": ("https://binaries.soliditylang.org/macosx-amd64/solc-macosx-amd64-v0.8.34+commit.80d5c536",
             "0a2829292697dda542e4e365bb63fbd6d3ed51537140222a880ab760cffa7746"),
    "solx": ("https://github.com/NomicFoundation/solx/releases/download/0.1.8/solx-macosx-v0.1.8",
             "5eda882c060d88b113876e91078ee35471f95e33772679732e339902a3dd4ec2"),
}


def sha(data):
    return hashlib.sha256(data).hexdigest()


def dump(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + "\n")


def compile_cases():
    toolchains = {}
    for name in ("solc", "solx"):
        binary = WORK / "bin" / name
        url, digest = BINARIES[name]
        if sha(binary.read_bytes()) != digest:
            raise RuntimeError(f"Unexpected {name} binary; expected {url}")
        version = subprocess.check_output([binary, "--version"], text=True)
        toolchains[name] = dict(name=name, version="0.8.34" if name == "solc" else "0.1.8",
            binary_path=str(binary), binary_sha256=sha(binary.read_bytes()),
            download_source=url, version_output=version,
            metadata={"solidity_frontend": "0.8.34"})
    dump(WORK / "toolchains.json", toolchains)
    artifacts, failures = [], []
    for bench, (contract_name, relative) in CASES.items():
        source = ROOT / "benches/implementations" / bench / "solidity" / relative
        sources = {}
        for path in sorted(source.parent.rglob("*.sol")):
            text = re.sub(r"pragma solidity[^;]*;", "pragma solidity =0.8.34;", path.read_text())
            key = str(path.relative_to(source.parent))
            sources[key] = {"content": text}
            materialized = WORK / "sources" / bench / key
            materialized.parent.mkdir(parents=True, exist_ok=True)
            materialized.write_text(text)
        for profile, (compiler, via_ir, optimizer) in PROFILES.items():
            settings = dict(evmVersion="cancun", viaIR=via_ir, optimizer=optimizer,
                metadata={"bytecodeHash": "none", "appendCBOR": False},
                outputSelection={"*": {"*": ["abi", "evm.bytecode.object", "evm.deployedBytecode.object"]}})
            data = dict(language="Solidity", sources=sources, settings=settings)
            dest = WORK / "compiles" / bench / profile
            dump(dest / "input.json", data)
            command = [toolchains[compiler]["binary_path"], "--standard-json"]
            if compiler == "solx":
                command += ["--threads", "1"]
            start = time.monotonic()
            result = subprocess.run(command, input=json.dumps(data), text=True, capture_output=True)
            elapsed = (time.monotonic() - start) * 1000
            (dest / "stdout.json").write_text(result.stdout)
            (dest / "stderr.txt").write_text(result.stderr)
            output = json.loads(result.stdout) if result.stdout else {}
            errors = [e for e in output.get("errors", []) if e.get("severity") == "error"]
            if result.returncode or errors:
                failures.append(dict(benchmark=bench, profile=profile, errors=errors, stderr=result.stderr))
                print("FAIL", bench, profile, errors, result.stderr, flush=True)
                continue
            c = output["contracts"][source.name][contract_name]
            creation, runtime = c["evm"]["bytecode"]["object"], c["evm"]["deployedBytecode"]["object"]
            a, b = len(bytes.fromhex(creation)), len(bytes.fromhex(runtime))
            scenario = ROOT / "benches/scenarios" / f"{bench}.yaml"
            artifacts.append(dict(benchmark_id=bench, implementation_id="solidity",
                suite="real_derived" if bench == "uniswap_v2_pair" else "fixed",
                language="solidity", contract_name=contract_name, profile_id=profile,
                compiler=toolchains[compiler], compiler_settings=settings, metadata_mode="off",
                source_path=str(WORK / "sources" / bench / source.name),
                source_hash=sha(sources[source.name]["content"].encode()),
                scenario_path=str(scenario), scenario_hash=sha(scenario.read_bytes()),
                abi=c["abi"], creation_bytecode=creation, runtime_bytecode=runtime,
                compile=dict(wall_ms_samples=[elapsed], cpu_ms_samples=[], peak_rss_kib=0),
                bytecode=dict(creation_bytes=a, creation_bytes_stripped=a, runtime_bytes=b,
                    runtime_bytes_stripped=b, initcode_bytes=a, linked_runtime_bytes=b,
                    eip170_margin_bytes=24576-b, eip3860_margin_bytes=49152-a, code_deposit_gas=200*b)))
            print(bench, profile, "runtime", b, "creation", a, flush=True)
    dump(WORK / "artifacts.json", artifacts)
    dump(WORK / "compile-failures.json", failures)
    if failures:
        raise RuntimeError("Compile failures; inspect before measuring")


def build_bridge():
    project = WORK / "bridge"
    (project / "src").mkdir(parents=True, exist_ok=True)
    cargo = (ROOT / "crates/bench-cli/Cargo.toml").read_text()
    cargo = cargo.replace('name = "evm-compiler-bench"', 'name = "solx-spike"')
    cargo = cargo.replace('edition.workspace = true', 'edition = "2024"')
    cargo = cargo.replace('license.workspace = true', 'license = "MIT"')
    cargo = cargo.replace('repository.workspace = true', '')
    (project / "Cargo.toml").write_text(cargo + "\n[workspace]\n")
    (project / "Cargo.lock").write_text((ROOT / "Cargo.lock").read_text().replace(
        'name = "evm-compiler-bench"', 'name = "solx-spike"'))
    modules = "#![allow(dead_code)]\n" + "\n".join(
        f'#[path = "{ROOT}/crates/bench-cli/src/{m}.rs"] mod {m};'
        for m in ("cache", "harness", "models", "runner", "scenarios", "util"))
    # Only change pair selection. Both artifacts retain honest Solidity identities.
    bridge = r'''
mod baselines {
    use crate::models::CompiledArtifact;
    use std::collections::BTreeMap;
    pub fn baseline_pairs(a: &[CompiledArtifact]) -> BTreeMap<String, (usize, usize)> {
        let profile = std::env::var("SPIKE_PAIR").unwrap();
        a.iter().enumerate().filter(|(_, x)| x.profile_id == profile).filter_map(|(i, x)| {
            a.iter().position(|y| y.benchmark_id == x.benchmark_id && y.profile_id == "solc-viair-200")
                .map(|j| (x.benchmark_id.clone(), (j, i)))
        }).collect()
    }
    pub fn comparison_pairs(a: &[CompiledArtifact]) -> Vec<(String, usize, usize)> {
        baseline_pairs(a).into_iter().map(|(name, (left, right))| (name, left, right)).collect()
    }
}
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let root = std::path::Path::new(&args[1]);
    let artifacts = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    let compiled = models::CompileSet { profiles: vec![], artifacts, failures: vec![] };
    let catalog = scenarios::load_scenario_catalog(root, None, &[])?;
    let gas = runner::run_foundry(root, "cancun", &compiled, &catalog, false)?;
    std::fs::write(root.join("gas.json"), serde_json::to_vec_pretty(&gas)?)?;
    Ok(())
}
'''
    (project / "src/main.rs").write_text(modules + bridge)
    subprocess.run(["cargo", "build", "--release", "--offline", "--manifest-path", str(project / "Cargo.toml"),
        "--target-dir", str(ROOT / "target")], check=True)


def measure():
    for mode in ("solx-O3", "solx-Oz"):
        run = WORK / mode
        (run / "foundry/test").mkdir(parents=True, exist_ok=True)
        (run / "benches/scenarios").mkdir(parents=True, exist_ok=True)
        for bench in CASES:
            shutil.copy(ROOT / "benches/scenarios" / f"{bench}.yaml", run / "benches/scenarios")
        config = re.sub(r'^solc\s*=.*$', '', (ROOT / "foundry/foundry.toml").read_text(), flags=re.MULTILINE)
        harness_solc = next((ROOT / ".cache/toolchains/solc/0.8.35").glob("solc-*"))
        (run / "foundry/foundry.toml").write_text(config + f'\nsolc = "{harness_solc}"\n')
        env = dict(os.environ, SPIKE_PAIR=mode)
        subprocess.run([ROOT / "target/release/solx-spike", run, WORK / "artifacts.json"], env=env, check=True)


def summarize():
    artifacts = json.loads((WORK / "artifacts.json").read_text())
    first = json.loads((WORK / "solx-O3/gas.json").read_text())
    second = json.loads((WORK / "solx-Oz/gas.json").read_text())
    key = lambda r: (r["benchmark_id"], r["scenario"], r["state_access_profile"], r["profile_id"])
    if sorted(first, key=key) != sorted(second, key=key):
        raise RuntimeError("Gas records differ between the O3 and Oz behavior-check runs")
    if len(first) != 164 or not all(r["scenario_status_ok"] for r in first):
        raise RuntimeError("Unexpected scenario count or status")
    by_key = {key(r): r for r in first}
    if len(by_key) != len(first):
        raise RuntimeError("Duplicate gas records")
    sizes = {(a["benchmark_id"], a["profile_id"]): a["bytecode"] for a in artifacts}
    summary = []
    for bench in CASES:
        result = {"benchmark": bench, "runtime_bytes": {
            p: sizes[bench, p]["runtime_bytes"] for p in PROFILES}}
        for p in ("solx-O3", "solx-Oz"):
            ratios = [r["harness_call_gas"] / by_key[(bench, r["scenario"],
                r["state_access_profile"], "solc-viair-200")]["harness_call_gas"]
                for r in first if r["benchmark_id"] == bench and r["profile_id"] == p and r["expected_success"]]
            result[p] = dict(successful_scenarios=len(ratios),
                median_gas_delta_pct=100 * (statistics.median(ratios)-1),
                runtime_delta_pct=100 * (sizes[bench, p]["runtime_bytes"] /
                    sizes[bench, "solc-viair-200"]["runtime_bytes"]-1))
        summary.append(result)
    dump(WORK / "summary.json", summary)
    with (WORK / "gas.csv").open("w") as f:
        writer = csv.writer(f)
        writer.writerow(["benchmark", "scenario", "access", "expected_success", *PROFILES])
        for bench, scenario, access in sorted({key(r)[:3] for r in first}):
            writer.writerow([bench, scenario, access, by_key[bench, scenario, access, "solc-viair-200"]["expected_success"],
                *[by_key[bench, scenario, access, p]["harness_call_gas"] for p in PROFILES]])
    dump(WORK / "provenance.json", {
        "repository_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "forge_version": subprocess.check_output(["forge", "--version"], text=True),
        "harness_source_sha256": {str(p.relative_to(ROOT)): sha(p.read_bytes())
            for p in (ROOT / "crates/bench-cli/src").rglob("*") if p.is_file()},
        "source_hashes": {a["benchmark_id"]: a["source_hash"] for a in artifacts},
        "gas_rows_per_run": len(first), "gas_records_identical_between_runs": True,
    })
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    compile_cases()
    build_bridge()
    measure()
    summarize()
