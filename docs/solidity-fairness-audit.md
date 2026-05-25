# Solidity-side fairness audit

This is an opinionated audit of where the benchmark suite, as written, tilts
the comparison against Solidity. Findings #1 and #2 have been applied to the
repo; the rest are open work items captured here so they aren't lost.

---

## #1 — Headline scorecard compared Solidity's *worst* configuration to Vyper's *best* — **FIXED**

`crates/bench-cli/src/report.rs:20-23` previously read:

```rust
const SOL_CODEGEN_BASELINE: &str = "solc-latest-legacy-runs200";
const VYPER_GAS_CODEGEN:    &str = "vyper-latest-gas";
...
"baseline_profile": SOL_CODEGEN_BASELINE,
"comparison_profile": VYPER_GAS_CODEGEN,
```

The default head-to-head ratio displayed on the public site was therefore
**`legacy-runs200` vs. `gas` mode**. That is not a fair fight:

- **Solidity** was configured to the *legacy* Yul-less codegen with
  `optimizer_runs = 200`. `200` is the spec default — explicitly chosen to
  bias toward *bytecode size*, not runtime gas. Every real production
  gas-sensitive solc deployment (OpenZeppelin v5, Uniswap V3 Factory, Uniswap
  V4, 1inch, …) uses runs in the **10,000–1,000,000** range. The matrix had
  zero high-runs profiles. There wasn't even a `solc-latest-legacy-runs10000`
  profile to pick.
- **Vyper** was configured to `optimizer_mode = "gas"` — Vyper's most
  aggressive runtime-gas mode, which *is* the correct symmetric counterpart
  to "high `optimizer_runs` in solc."
- `solc-latest-viair-runs200` existed in the matrix — i.e. the modern
  pipeline that solves "stack too deep" and produces tighter runtime code —
  but the report defaulted the comparison to legacy. ViaIR was shown only as
  a secondary curve.

The honest comparison is `solc-latest-viair-runs<10000+>` vs.
`vyper-latest-gas`. The harness can already run viaIR; it just didn't pick it
for the headline number, and it never tried high runs.

**Fix applied.** A new profile
`compiler-profiles/solc-latest-viair-runs10000-latest-shared-evm.toml` was
added. `SOL_CODEGEN_BASELINE` (in `report.rs`) and `SOL_BASELINE` (in
`runner.rs`) both now point at it. The UI headline finding
(`report-ui/src/main.jsx`, `stableSolVsVyperGas` / `stableSolVsVyperSize`) was
updated to use a new `SOL_VIAIR_HIGH` constant pointing at the same profile.
The hero lede and Finding 01 card prose were also rewritten so that the
displayed text matches the new comparison (previously the math switched to
viaIR-runs=10000 but the prose still referred to "solc legacy", and the body
contained a hardcoded `9.6%` from the prior comparison). The hero now derives
the direction ("lower"/"higher") from the ratio sign so it is correct even if
the new comparison reverses outcome. The legacy and default-runs viaIR
profiles remain in the matrix for context; they're just no longer the default
Solidity bar on the headline chart.

---

## #2 — Loop counters in the scale families were not `unchecked` in Solidity — **FIXED**

`crates/bench-cli/src/scale.rs` (loop_bound, external_calls, events families)
previously emitted:

```rust
"for (uint256 i = 0; i < {n}; i++) { ... }"
```

In `solc ≥ 0.8`, `i++` is **checked arithmetic** by default — every iteration
pays for an overflow check. Every competent Solidity gas optimizer writes:

```solidity
for (uint256 i = 0; i < n;) {
    ...
    unchecked { ++i; }
}
```

Vyper's `range(n)` iteration counter has **no overflow check** — the bound is
the static literal `n`, so the compiler knows it can't overflow. So every
iteration of `loop_bound_N`, `events_N`, `external_calls_N`, plus the inner
loops in real-derived contracts, **systematically tilted ~30 gas/iteration in
Vyper's favor for no language-level reason** — just because the Solidity
source wasn't using the standard idiom. This was the single most embarrassing
thing on the `loop_bound_N` scaling chart.

**Fix applied.** All three Solidity scale-family templates now emit
`for (uint256 i = 0; i < n;) { ... unchecked { ++i; } }`. Inner arithmetic
(`total += i + 1`, etc.) is *intentionally* left checked because Vyper checks
the equivalent expressions too — the goal is parity, not handing Solidity a
free win.

---

## #3 — `external_calls_N` uses `max_outsize=0` in Vyper, not in Solidity

`crates/bench-cli/src/scale.rs` (external_calls_family):

```rust
// Solidity
"(bool ok,) = address(this).staticcall(abi.encodeWithSignature(\"ping(uint256)\", i));"

// Vyper
"raw_call(self, concat(method_id(\"ping(uint256)\"), abi_encode(i)),
         max_outsize=0, is_static_call=True)"
```

Vyper is told explicitly that the return data is 0 bytes — no return buffer,
no `RETURNDATACOPY`. Solidity uses the high-level `.staticcall` returning
`bytes memory`, then drops it via `(bool ok,)`. Under legacy codegen the
compiler still allocates and copies returndata; under viaIR it *may* DCE the
allocation but isn't guaranteed to. By using the high-level API on Solidity
and the low-level escape hatch on Vyper, the benchmark measures
convenience-vs-perf, not language-vs-language. On `external_calls_N=64` this
is dozens of unnecessary `MSTORE` / `RETURNDATACOPY` ops on Solidity's side.

**Fair fix:** use inline assembly on the Solidity side so it can also pass
`retOffset=0, retSize=0`:

```solidity
bytes memory data = abi.encodeWithSignature("ping(uint256)", i);
bool ok;
assembly {
    ok := staticcall(gas(), address(), add(data, 0x20), mload(data), 0, 0)
}
require(ok, "ping");
```

Or, simpler: drop `max_outsize=0` from the Vyper side. Either choice
equalises the cost; the current choice does neither.

---

## #4 — UniswapV2 Pair port preserves a Solidity footgun the Vyper port silently removes

`benches/implementations/uniswap_v2_pair/solidity/UniswapV2PairReal.sol:107, 133, 158`:

```solidity
(uint112 reserve0, uint112 reserve1,) = this.getReserves();
```

`this.getReserves()` is an **external self-call** (STATICCALL). It exists in
the original UniV2 Pair as an ABI quirk — but `mint`/`burn`/`swap` call it via
`this.` rather than reading the packed slot directly. The Vyper port
(`benches/implementations/uniswap_v2_pair/vyper/UniswapV2PairReal.vy:117-120, 146-147, 167-168`)
just does:

```vyper
old_reserve0: uint256 = self.reserve0
old_reserve1: uint256 = self.reserve1
```

So on every `mint`, `burn`, and `swap`:

- **Solidity** pays: STATICCALL (~700 gas warm) + entry dispatch + slot SLOAD
  + uint112/uint32 packing on return + returndata decode in caller — call it
  1.5–2k gas of pure overhead per swap.
- **Vyper** pays: 2 direct warm SLOADs (≈ 200 gas).

A "real-derived" comparison either preserves the quirk on *both* sides or
fixes it on *both* sides — anything else is measuring an artifact, not the
compiler. The bench spec already says this is a *clean-room model, not a
production claim* (`production_equivalence=false`), so the case for fixing
the Solidity port is strong.

**Fair fix:** the obvious choice for a clean-room benchmark is to change the
Solidity port to read `reserve0_/reserve1_` directly inline, matching Vyper.
Alternatively add a Vyper `getReserves()` external function and route every
internal call through it.

The current asymmetric port partially masks a real Solidity *advantage* —
the packed slot layout — because the call overhead on `getReserves` cancels
the SSTORE savings on `_update`.

---

## #5 — `MINIMUM_LIQUIDITY` (and friends) exposes a getter only in Solidity

`UniswapV2PairReal.sol:5`: `uint256 public constant MINIMUM_LIQUIDITY = 1000;`
`UniswapV2PairReal.vy:3`:  `MINIMUM_LIQUIDITY: constant(uint256) = 1000`

Solidity's `public constant` generates an external getter → extra ABI
selector, larger dispatch table, larger runtime bytecode. Vyper's `constant`
does not. The contracts therefore have **different ABIs**, and Solidity
carries a tiny per-call dispatch penalty on *every* selector.

Same pattern appears in the other real-derived models:

- `CurveStableSwap2CoinReal.sol`: `N_COINS`, `A_PRECISION`, `FEE_DENOMINATOR`
  all `public constant` — Vyper equivalents are non-public `constant`.
- `YearnVaultV3Real.sol`: `MAX_BPS`, `WAD` — same pattern.

**Fair fix:** drop `public` on the Solidity side (use a non-public constant),
or use `public(constant(...))` on the Vyper side. Pick one and apply to both
languages.

---

## #6 — Yearn vault relies on a `storage` pointer idiom Vyper lacks

`YearnVaultV3Real.sol:168` uses:

```solidity
Strategy storage s = strategies[strategy];
require(s.activation != 0, "inactive");
require(targetDebt <= s.maxDebt, "max");
uint256 previousDebt = s.currentDebt;
// ... etc, all references go through s
```

One `keccak256` to derive the struct base slot, four `SLOAD`s at fixed offsets
from that base. Predictable, cheap.

`YearnVaultV3Real.vy:182-202` has to write:

```vyper
assert self.strategies[strategy].activation != 0, "inactive"
assert target_debt <= self.strategies[strategy].maxDebt, "max"
previous_debt: uint256 = self.strategies[strategy].currentDebt
self.strategies[strategy].currentDebt = target_debt
self.strategies[strategy].balance += amount
```

Whether `self.strategies[strategy]` gets common-subexpression-eliminated into
a single keccak depends entirely on the Vyper optimizer. Modern Vyper + Venom
probably CSEs this; historical Vyper profiles (`0.2.16`, `0.3.10`, `0.4.0`
default codegen) likely don't — each access re-hashes the mapping key. So
historical Vyper bars on this chart include a per-access keccak penalty that
the equivalent historical Solidity bars don't.

This isn't a fixable benchmark bug per se — it's a real language-level
difference. But the report should *call it out* in the methodology rather
than let the chart speak as if every bar measures only compiler quality.

**Fair fix:** add a methodology note next to the Yearn benchmark saying
"Vyper has no `storage` pointer; repeated mapping accesses on the same key
depend on compiler CSE, which not all Vyper versions perform."

---

## #7 — Smaller paper cuts (additive)

### 7a — ERC20Minimal `name`/`symbol` type asymmetry

`Erc20Minimal.vy:3-4` uses `String[32]` / `String[8]` (Vyper-only tight
bounds). `Erc20Minimal.sol:5-6` uses `string public constant` (unbounded). At
runtime this is approximately equivalent for these literals, but a Solidity
engineer chasing parity would use `bytes32 / bytes8`. Idiomatic Solidity here
is slightly heavier than idiomatic Vyper purely because of the type choice.

### 7b — UniV2 `_sqrt` bounded vs unbounded loop

`UniswapV2PairReal.vy:_sqrt` uses `for _: uint256 in range(256):` — bounded
loop with an iteration counter. Solidity's `_sqrt` uses an unbounded `while`.
Tiny Vyper-side per-iteration cost, but worth noting that the *shapes* are
different.

### 7c — Modifier vs internal-call overhead in real-derived contracts

Solidity's `ready` and `onlyManager` are *modifiers* — inlined at every
external-function entry point by legacy codegen. Vyper's `_ready()` and
`_only_manager()` are real internal function calls (`JUMP`/`RETURN`-based)
unless the optimizer inlines them. For modifier-heavy contracts like Yearn
(both modifiers on every admin function), this is a small Solidity-favourable
asymmetry the current report doesn't isolate. A method-level breakdown of
"dispatch + modifier overhead" vs "real work" would surface this.

---

## #8 — Headline metric is `harness_call_gas`, not `harness_estimated_tx_gas`

`report.rs:272`:

```rust
"primary_metric": "harness_call_gas",
```

`harness_call_gas` is the delta of `gasleft()` across an inter-contract
`.call(...)`. It includes the dispatch table and intra-contract calldata
charges, but **not** the 21,000 base intrinsic + outside-tx calldata charges
that dominate user-perceived gas. `harness_estimated_tx_gas` is already
computed but not the default.

For a contract with 100k gas of real work, "Solidity is 5% slower than Vyper"
on `harness_call_gas` becomes "~4% slower" on `harness_estimated_tx_gas` once
the 21k intrinsic dilutes the ratio. The right answer to show users depends
on the question being asked — "which compiler generates better runtime
code?" (call gas) vs "which contract costs my users less per call?" (tx gas).
Both are valid; the current report buries the second behind a default.

**Fair fix:** add `harness_estimated_tx_gas` as a toggle in the UI with
equal prominence, and consider making it the default for the public-facing
chart.

---

## Summary of PR-level fixes (in order of impact)

1. ✅ Make `SOL_CODEGEN_BASELINE` `solc-latest-viair-runs10000`.
2. ✅ `unchecked { ++i; }` in scale-family Solidity loop counters.
3. Match `max_outsize=0` semantics in the external_calls Solidity template (inline assembly with `staticcall(..., 0, 0)`).
4. Pick *one* storage-access pattern for `UniswapV2PairReal` and apply it to both languages (preferred: direct storage reads on both sides).
5. Drop `public` on Solidity constants in real-derived models, or add `public(constant(...))` on the Vyper side. Same ABI on both sides.
6. Add a methodology callout for the Vyper "no storage pointer" issue on Yearn-style benchmarks.
7. Add `harness_estimated_tx_gas` as a toggle-equal primary metric in the UI; consider making it the default for the public-facing chart.
8. *(Optional)* Add `solc-latest-legacy-runs10000` and `solc-latest-viair-runs1000000` so the matrix can show the full optimizer_runs sensitivity curve, not just the 200/10000 endpoints.
