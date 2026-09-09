#!/usr/bin/env python3
"""Check Vyper source migrations against stable compiler behavior.

Run after the full benchmark pipeline: uv run scripts/verify_vyper_compat.py
Uses the materialized source variants and recorded compiler binaries. Writes
standalone Foundry tests, compiler inputs, and an audit under target/vyper-compat.
No downloads or Python dependencies are needed.
"""

import hashlib
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "target/vyper-compat"
VAULT = "benches/implementations/yearn_vault_v2/vyper/latest/Vault.vy"
PAIR = "benches/implementations/uniswap_v2_pair/vyper/UniswapV2PairReal.vy"
CURVE = "benches/implementations/curve_stableswap_2coin/vyper/latest/CurveStableSwapNG.vy"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def variant(profile, source):
    return (ROOT / "target/bench-source-variants" / profile / source).read_text()


def check_historical_scenarios():
    rows = [json.loads(line) for line in (ROOT / "results/raw/foundry-gas.jsonl").read_text().splitlines()]
    wanted = {
        ("yearn_vault_v2", f"vyper-0.3.10-{mode}")
        for mode in ["gas", "codesize", "none"]
    } | {
        (benchmark, f"vyper-0.3.7-{mode}")
        for benchmark in ["yearn_vault_v2", "curve_stableswap_2coin"]
        for mode in ["default", "none"]
    }
    indexed = {(r["benchmark_id"], r["profile_id"], r["scenario"]): r for r in rows}
    fields = ["call_succeeded", "scenario_status_ok", "return_hash", "observer_hash", "log_hash"]
    checked = []
    for row in rows:
        if (row["benchmark_id"], row["profile_id"]) not in wanted:
            continue
        baseline = indexed[(row["benchmark_id"], "vyper-latest-gas", row["scenario"])]
        assert row["scenario_status_ok"], row
        for field in fields:
            assert row[field] == baseline[field], (row["benchmark_id"], row["profile_id"], row["scenario"], field)
        checked.append({key: row[key] for key in ["benchmark_id", "profile_id", "scenario"]})
    assert {(r["benchmark_id"], r["profile_id"]) for r in checked} == wanted
    return {"baseline": "vyper-latest-gas", "fields": fields, "scenarios": checked}


def compile_probe(row, source):
    profile = row["profile_id"]
    compiler = row["compiler"]
    binary = Path(compiler["binary_path"])
    if not binary.is_absolute():
        binary = ROOT / binary
    assert digest(binary.read_bytes()) == compiler["binary_sha256"]
    path = OUT / f"{profile}.vy"
    path.write_text(source)
    settings = compiler["settings"]
    args = [str(binary), str(path), "-f", "bytecode", "--evm-version", settings["evmVersion"]]
    # 0.3.7 exposes --no-optimize instead of named optimizer modes.
    if compiler["version"] == "0.3.7":
        if settings["optimize"] == "none":
            args.append("--no-optimize")
    else:
        args += ["-O", settings["optimize"]]
    if settings.get("experimentalCodegen"):
        args.append("--experimental-codegen")
    output = subprocess.run(args, capture_output=True, text=True)
    if output.returncode:
        raise RuntimeError(f"{profile}: {output.stdout}{output.stderr}")
    code = output.stdout.strip().removeprefix("0x")
    assert code and all(ch in "0123456789abcdefABCDEF" for ch in code)
    return code, {
        "profile": profile,
        "version": compiler["version"],
        "binary_sha256": compiler["binary_sha256"],
        "probe_source_sha256": digest(source.encode()),
        "creation_bytecode_sha256": digest(bytes.fromhex(code)),
        "command": args,
    }


def main():
    (OUT / "test").mkdir(parents=True, exist_ok=True)
    rows = json.loads((ROOT / "results/normalized/results.json").read_text())
    historical = check_historical_scenarios()
    profiles = ["vyper-latest-gas"] + [
        f"vyper-prerelease-{mode}{suffix}"
        for mode in ["gas", "codesize", "none"]
        for suffix in ["", "-venom"]
    ] + ["vyper-0.3.7-default", "vyper-0.3.7-none"]
    creations, audit = [], []
    for profile in profiles:
        row = next(r for r in rows if r["profile_id"] == profile and r["benchmark_id"] == "curve_stableswap_2coin")
        curve = variant(profile, CURVE)
        # Copy the arithmetic body. The external probe needs a different name:
        # 0.3.7 reserves 'exp' for external entrypoints.
        exp = "@external\n@pure\n" + curve[curve.index("def exp("):].split("\n@", 1)[0].replace("def exp(", "def curve_exp(", 1)
        if profile.startswith("vyper-0.3.7"):
            source = exp
        else:
            vault, pair = variant(profile, VAULT), variant(profile, PAIR)
            start = vault.index('    if nameOverride == "":')
            end = vault.index("    decimals: uint256 =", start)
            sqrt = "math.isqrt" if "math.isqrt(" in pair else "isqrt"
            source = (
                ("import math\n" if sqrt.startswith("math.") else "")
                + "from ethereum.ercs import IERC20Detailed\nname: String[64]\nsymbol: String[32]\n"
                + "@external\ndef probe(token: address, nameOverride: String[64], symbolOverride: String[32]) -> (String[64], String[32]):\n"
                + vault[start:end]
                + "    return self.name, self.symbol\n\n"
                + f"@external\n@pure\ndef sqrt(x: uint256) -> uint256:\n    return {sqrt}(x)\n\n"
                + exp
            )
        code, evidence = compile_probe(row, source)
        creations.append(f'targets.push(deploy(hex"{code}"));')
        audit.append(evidence)
    solc_row = next(r for r in rows if r["profile_id"] == "solc-latest-noopt")
    solc = Path(solc_row["compiler"]["binary_path"])
    if not solc.is_absolute():
        solc = ROOT / solc
    (OUT / "foundry.toml").write_text(
        '[profile.default]\ntest = "test"\n'
        + f"solc = {json.dumps(str(solc))}\n"
        + 'evm_version = "prague"\noptimizer = true\nvia_ir = true\n'
        + "[profile.default.fuzz]\nruns = 256\n"
    )
    (OUT / "test/Compatibility.t.sol").write_text(TEST.replace("CONSTRUCTORS", "\n".join(creations)))
    result = subprocess.run(["forge", "test", "--root", str(OUT), "--offline", "-vv"], capture_output=True, text=True)
    (OUT / "forge.log").write_text(result.stdout + result.stderr)
    (OUT / "audit.json").write_text(json.dumps({"compilers": audit, "historical_differential": historical, "forge_exit_code": result.returncode}, indent=2) + "\n")
    print(f"Historical scenarios matching stable Vyper: {len(historical['scenarios'])}")
    print(result.stdout + result.stderr)
    result.check_returncode()


TEST = '''pragma solidity ^0.8.36;
interface IProbe {
    function sqrt(uint256 x) external pure returns (uint256);
    function curve_exp(int256 x) external pure returns (uint256);
    function probe(address token, string calldata n, string calldata s) external returns (string memory, string memory);
}
contract Token {
    string public symbol;
    constructor(uint256 length) {
        bytes memory x = new bytes(length);
        for (uint256 i; i < length; i++) x[i] = "x";
        symbol = string(x);
    }
}
contract CompatibilityTest {
    address[] targets;
    function deploy(bytes memory code) internal returns (address a) {
        assembly { a := create(0, add(code, 32), mload(code)) }
        require(a != address(0));
    }
    function setUp() public { CONSTRUCTORS }
    function testFuzzSqrt(uint256 x) public view {
        uint256 expected = IProbe(targets[0]).sqrt(x);
        require(expected <= type(uint128).max && expected * expected <= x, "sqrt lower bound");
        require(expected == type(uint128).max || (expected + 1) * (expected + 1) > x, "sqrt upper bound");
        for (uint256 i = 1; i < 7; i++) require(IProbe(targets[i]).sqrt(x) == expected, "sqrt changed");
    }
    function testSqrtBoundaries() public view {
        uint256[12] memory xs = [uint256(0), 1, 2, 3, 4, 8, 9, 15, 16, 17, type(uint256).max - 1, type(uint256).max];
        for (uint256 i; i < xs.length; i++) testFuzzSqrt(xs[i]);
    }
    function checkSymbol(uint256 length, bool autoName, bool autoSymbol, bool shouldPass) internal {
        address token = address(new Token(length));
        bytes memory callData = abi.encodeCall(IProbe.probe, (token, autoName ? "" : "name override", autoSymbol ? "" : "symbol override"));
        (bool referenceOK, bytes memory referenceData) = targets[0].call(callData);
        require(referenceOK == shouldPass, "stable symbol boundary");
        for (uint256 i = 1; i < 7; i++) {
            (bool ok, bytes memory data) = targets[i].call(callData);
            require(ok == referenceOK, "symbol acceptance changed");
            if (ok) require(keccak256(data) == keccak256(referenceData), "symbol contents changed");
        }
        if (referenceOK) {
            (string memory n, string memory s) = abi.decode(referenceData, (string, string));
            require(keccak256(bytes(n)) == keccak256(bytes(autoName ? string.concat(Token(token).symbol(), " yVault") : "name override")), "name bytes");
            require(keccak256(bytes(s)) == keccak256(bytes(autoSymbol ? string.concat("yv", Token(token).symbol()) : "symbol override")), "symbol bytes");
        }
    }
    function testSymbolBoundaries() public {
        checkSymbol(0, true, true, true); checkSymbol(1, true, true, true);
        checkSymbol(2, true, true, false); checkSymbol(30, true, true, false); checkSymbol(31, true, true, false);
        checkSymbol(1, true, false, true); checkSymbol(2, true, false, false);
        checkSymbol(57, true, false, false); checkSymbol(58, true, false, false);
        checkSymbol(1, false, true, true); checkSymbol(2, false, true, false);
        checkSymbol(30, false, true, false); checkSymbol(31, false, true, false);
        checkSymbol(65, false, false, true);
    }
    function checkExp(int256 x) internal view {
        bytes memory input = abi.encodeCall(IProbe.curve_exp, (x));
        (bool referenceOK, bytes memory referenceData) = targets[0].staticcall(input);
        require(referenceOK == (x < 135305999368893231589), "stable exp boundary");
        for (uint256 i = 1; i < targets.length; i++) {
            (bool ok, bytes memory data) = targets[i].staticcall(input);
            require(ok == referenceOK, "exp acceptance changed");
            if (ok) require(keccak256(data) == keccak256(referenceData), "exp changed");
        }
        if (x <= -41446531673892822313) require(abi.decode(referenceData, (uint256)) == 0, "exp underflow");
    }
    function testFuzzExp(uint256 seed) public view {
        checkExp(int256(seed % 176752531042786053905) - 41446531673892822314);
    }
    function testExpBoundaries() public view {
        checkExp(type(int256).min); checkExp(-41446531673892822314);
        checkExp(-41446531673892822313); checkExp(-41446531673892822312);
        checkExp(-1); checkExp(0); checkExp(1);
        checkExp(135305999368893231588); checkExp(135305999368893231589); checkExp(type(int256).max);
    }
}
'''

if __name__ == "__main__":
    main()
