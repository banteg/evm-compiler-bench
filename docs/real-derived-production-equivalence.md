# Real-Derived Production-Equivalence Inventory

Last audited: 2026-05-24 on `dev`.

This is the work queue for flipping any real-derived benchmark to
`production_equivalence: true`. The upstream source-language side is exact only
when the checked-in implementation path matches the spec `source_path` and
`git hash-object` matches `source_blob`; `cargo run --release -- validate`
enforces that gate.

A counterpart-language port is production-equivalent only after all of these
are true:

- The port is an idiomatic implementation of the upstream contract's full
  externally observable behavior, not a scenario-scoped subset.
- The spec has no `excluded_features`.
- ABI shape, access control, accounting, external-call behavior, token return
  handling, events, and intended success/revert behavior have been audited.
- Scenario coverage is broad enough to catch cross-feature interactions, not
  only one-call happy paths.
- A benchmark-scoped no-cache run and `validate` pass. The full matrix and
  report build are still required before release.

Storage slot layout is tracked separately. It does not have to match when the
comparison is intentionally idiomatic, unless upstream behavior depends on that
layout.

## Summary

| Benchmark | Exact source-language side | Counterpart status | Main blockers |
| --- | --- | --- | --- |
| `uniswap_v2_pair` | Vendored upstream Solidity `UniswapV2Pair.sol` at the pinned blob. | Vyper port covers the pair hot path and LP-token surface. | Vyper `Bytes[4096]` callback bound vs upstream unbounded `bytes calldata`; full factory behavior is represented by a benchmark fixture; final ABI/revert audit still pending. |
| `curve_stableswap_2coin` | Vendored upstream Vyper `CurveStableSwapNG.vy` at the pinned blob. | Solidity port covers a two-coin NG deployment across standard, oracle, rebasing, and ERC4626 harness tokens. | Solidity port is fixed at `N_COINS = 2`; upstream is constructor-driven up to 8 coins; delegated `StableSwapViews` call topology is internalized; factory/views dependencies are harness fixtures. |
| `yearn_vault_v3` | Vendored upstream Vyper `VaultV3.vy` at the pinned blob. | Solidity port covers the main vault API, management paths, strategy accounting, modules, queues, and permit. | Full parity audit is still pending for cross-feature sequences and third-party module/accountant/strategy edge cases; Vyper bounded `String` and `DynArray` ABI behavior is approximated with Solidity runtime checks. |

## Status Legend

- Exact source: the checked-in source-language implementation is upstream at
  the pinned blob, and `validate` enforces the hash.
- Exact counterpart surface: the counterpart-language port implements the same
  externally observable behavior for that surface and the listed scenarios
  differentially cover it.
- Fixture-exact: the production contract behavior is exercised through a
  deterministic benchmark fixture. This can prove the contract-side behavior,
  but not the full upstream dependency implementation.
- Approximate: the success/failure behavior is intentionally matched while some
  lower-level detail differs, usually decoder timing, revert data, call
  topology, or a language-level bound.
- Incomplete: still blocks `production_equivalence: true`.

## Granular Inventory

### `uniswap_v2_pair`

| Surface | Status | Notes |
| --- | --- | --- |
| Upstream pair source | Exact source | `UniswapV2Pair.sol` is vendored at the pinned blob. |
| Factory ownership of `initialize` | Fixture-exact | The pair-side factory gate and CREATE2 deployment path are covered; the full upstream factory contract is not the benchmark target. |
| LP ERC20 metadata, balances, allowances, transfers | Exact counterpart surface | Metadata, allowances, `transfer`, and `transferFrom` are implemented and scenario-covered. |
| EIP-2612 permit | Exact counterpart surface, audit pending | Valid and invalid signatures are covered; chain-id/domain-separator edge cases remain on the audit list. |
| Reserve packing and `getReserves` | Exact counterpart surface | Vyper packs `reserve0`, `reserve1`, and `blockTimestampLast` into the upstream bit layout. |
| Mint, burn, swap, skim, sync | Exact counterpart surface | Initial/subsequent mint, burn, invariant swap, drift, skim, sync, and timestamp wrap paths are covered. |
| Protocol-fee `kLast` behavior | Exact counterpart surface | Fee-on minting and fee-off reset are covered. |
| Optional-return token handling | Exact counterpart surface | No-return transfer-out paths are covered for swap, burn, and skim. |
| Flash-swap callback | Approximate | Non-empty, reentrant, and larger-than-old-1024-byte data are covered, but Vyper still has a `Bytes[4096]` ABI bound while upstream Solidity accepts unbounded `bytes calldata`. |
| Revert data and ABI boundary behavior | Incomplete | Success/failure is covered for important paths, but exhaustive revert-data and decoder-boundary parity has not been audited. |
| Storage layout | Approximate | Packed reserves intentionally match; the rest is idiomatic Vyper storage and not full layout-compatible. |

Immediate chips:

- Decide whether the `Bytes[4096]` bound is an accepted language-policy
  exception, or add an over-4096-byte divergence scenario and keep the port
  non-production-equivalent.
- Decide whether the benchmark remains pair-only or must include the full
  upstream factory implementation.
- Complete the ABI/revert audit for the pair ABI outside the current scenarios.

### `curve_stableswap_2coin`

| Surface | Status | Notes |
| --- | --- | --- |
| Upstream pool source | Exact source | `CurveStableSwapNG.vy` is vendored at the pinned blob. |
| Two-coin constructor setup | Exact counterpart surface for `N_COINS = 2` | The harness deploys matched standard, oracle-rate, rebasing, and ERC4626 two-coin pools. |
| Dynamic `N_COINS` generality | Incomplete | Upstream supports constructor-driven coin counts up to `MAX_COINS = 8`; the Solidity port hard-codes two coins. |
| Add/remove liquidity and exchange paths | Exact counterpart surface for two coins | Balanced, imbalanced, one-coin, standard exchange, and `exchange_received` paths are covered. |
| NG stored-rate, oracle, rebasing, ERC4626 behavior | Exact counterpart surface for fixtures | Constructor-provided multipliers, oracles, rebasing flags, and ERC4626 rates are covered through deterministic fixtures. |
| Moving-average oracle decay | Exact counterpart surface | Price and D oracle scenarios advance time and cover exponential decay. |
| Dynamic/off-peg fees and admin fees | Exact counterpart surface | Fee quotes, exchange accounting, and admin-fee withdrawal are covered. |
| Admin controls | Exact counterpart surface | Ramp, stop-ramp, fee updates, moving-average windows, and admin gating are covered. |
| LP token and permit | Exact counterpart surface | EOA and ERC1271 permit success plus invalid permit failure are covered. |
| Factory and views dependencies | Fixture-exact | The upstream Vyper side calls the benchmark-provided factory/views fixture. |
| `StableSwapViews` call topology in Solidity | Incomplete | The Solidity port currently computes `get_dy`, `get_dx`, `dynamic_fee`, and `calc_token_amount` internally instead of calling `factory.views_implementation()`. |
| Vyper `DynArray[MAX_COINS]` ABI bounds | Approximate | Too-long arrays and ignored extra entries are covered, but Solidity enforces this with runtime checks, not Vyper decoder behavior. |
| Storage layout | Approximate | The Solidity port is idiomatic and not storage-layout-compatible. |

Immediate chips:

- Mirror the `factory.views_implementation()` quote-view call boundary in the
  Solidity port, then rerun the targeted Curve benchmark.
- Decide whether production equivalence means a full generic NG Solidity port
  or an explicitly production-equivalent two-coin specialization.
- If full NG is the target, replace the fixed two-coin arrays and loops with
  constructor-driven `N_COINS` behavior and add non-two-coin scenarios.

### `yearn_vault_v3`

| Surface | Status | Notes |
| --- | --- | --- |
| Upstream vault source | Exact source | `VaultV3.vy` is vendored at the pinned blob. |
| Blueprint/minimal-proxy deployment | Exact source path | The upstream Vyper benchmark is deployed through the blueprint/clone path. |
| ERC4626 deposit, mint, withdraw, redeem | Exact counterpart surface, audit pending | Direct and default-argument overload paths are scenario-covered. |
| ERC20 share accounting and permit | Exact counterpart surface, audit pending | Transfers, approvals, EIP-712 permit before/after initialization, and invalid permits are covered. |
| Role bitmasks and role-manager handoff | Exact counterpart surface, audit pending | Set/add/remove role, delegated execution, bounds, pending transfer, and acceptance are covered. |
| Metadata setters | Approximate | Name and symbol setters plus Vyper string length failures are covered with Solidity runtime checks. |
| Strategy registry and debt management | Exact counterpart surface, audit pending | Add, revoke, force revoke, max debt, debt increase/decrease, max-loss defaults, and buy-debt paths are covered. |
| Report accounting and locked profit | Exact counterpart surface, audit pending | Profit, loss, accountant fees/refunds, protocol fees, excessive-fee failure, reentrancy failure, and unlock-over-time paths are covered. |
| Default/custom withdrawal queues | Exact counterpart surface, audit pending | Default queue, custom queue, forced default queue, queue order, and long-queue failures are covered. |
| Limit modules and accountant dependencies | Fixture-exact | Deterministic mocks cover important accept/reject paths, but arbitrary third-party behavior is not exhaustive. |
| Cross-feature sequence behavior | Incomplete | One combined management sequence is covered; alternate orderings and repeated transitions remain. |
| Vyper `String` and `DynArray` decoder details | Approximate | Length success/failure is covered, but decoder timing and revert data are not exact. |
| Function-by-function parity audit | Incomplete | The Solidity port is broad, but each upstream branch has not been checked off against the pinned Vyper source. |
| Storage layout | Approximate | Full storage-layout compatibility is intentionally false for the idiomatic Solidity port. |

Immediate chips:

- Build and keep a source-to-port checklist for every Yearn external and
  internal function branch.
- Add sequence scenarios for alternate orderings of roles, queues, debt,
  reports, modules, shutdown, and withdrawals.
- Expand third-party accountant/module/strategy mock coverage for unusual but
  interface-valid implementations.

## `uniswap_v2_pair`

Exact now:

- The Solidity benchmark implementation is the pinned upstream
  `contracts/UniswapV2Pair.sol`.
- Supporting upstream Solidity sources for ERC20, factory, interfaces, and
  libraries are vendored next to the pair source.
- The Vyper port implements the matched pair API: initialization, reserves,
  mint, burn, swap, skim, sync, fee-on `kLast`, cumulative prices, LP ERC20
  accounting, and permit.
- The Vyper port uses packed reserves with the upstream bit layout and
  `default_return_value=True` transfer handling for no-return ERC20s.
- Scenarios cover initial and subsequent mints, factory CREATE2 deployment,
  swap invariant checks, no-return token transfers, flash callback repayment,
  flash reentrancy rejection, fee-on/off behavior, timestamp wrapping, and
  permit success/failure.
- The generated differential harness normalizes deployment-specific addresses
  and compares event/log hashes for the listed scenarios.

Remaining:

- Upstream `swap` accepts unbounded `bytes calldata`. Vyper requires a bounded
  byte array and the current port uses `Bytes[4096]`. Decide whether this
  language-level bound keeps the benchmark non-production-equivalent, or
  document an explicit policy exception before removing the excluded feature.
- The benchmark CREATE2 fixture exercises the pair's factory-owned initialize
  path and `feeTo`, but it is not the full upstream `UniswapV2Factory`.
- The final audit still needs to check revert reasons or decoder failures where
  they matter, permit/domain separator behavior across chain-id changes, and
  every ABI entry outside the current scenarios.

Suggested next chips:

- Add a targeted over-4096-byte flash callback divergence scenario, or make the
  bounded-by-language policy decision explicit.
- Decide whether full upstream factory behavior is in scope for this benchmark
  or whether the benchmark is explicitly "Pair only".

## `curve_stableswap_2coin`

Exact now:

- The Vyper benchmark implementation is the pinned upstream
  `contracts/main/CurveStableSwapNG.vy`.
- The Solidity port implements a matched two-coin pool API, including
  add/remove liquidity, exchange, `exchange_received`, one-coin withdrawal,
  amplification and fee admin controls, LP ERC20 accounting, permit, moving
  averages, stored rates, and admin-fee accounting.
- Scenarios cover standard ERC20s, no-return ERC20s, oracle-rate assets,
  rebasing asset behavior, ERC4626 rate scaling, dynamic fees, admin controls,
  slippage and invalid coin reverts, and Vyper DynArray length edges for the
  two-coin deployment.
- Price and D oracle scenarios now advance time and exercise the upstream NG
  exponential moving-average decay path rather than only same-block oracle
  upkeep.
- Permit scenarios now cover both EOA EIP-712 signatures and the upstream
  ERC1271 smart-contract-wallet validation path.
- The generated differential harness normalizes deployment-specific pool and
  coin addresses and compares event/log hashes for the listed scenarios.

Remaining:

- Upstream `CurveStableSwapNG` is generic over `N_COINS` from constructor input
  up to `MAX_COINS = 8`; the Solidity port hard-codes `N_COINS = 2` and uses
  fixed-size two-element internal arrays.
- Upstream `get_dy`, `get_dx`, `dynamic_fee`, and `calc_token_amount` delegate
  to `factory.views_implementation()`. The Solidity port internalizes those
  calculations, so behavior may match while call topology and gas shape do not.
- The factory, admin, fee receiver, rate oracle, rebasing token, and ERC4626
  dependencies are deterministic benchmark fixtures, not full upstream
  deployments.
- Solidity runtime length checks approximate Vyper `DynArray` decoder bounds;
  revert data and decoder timing are not exact.

Suggested next chips:

- Decide whether the target is the full NG contract or a production-equivalent
  two-coin specialization. If it is full NG, replace the Solidity port's
  fixed-size arrays and loops with constructor-driven `N_COINS` behavior.
- Mirror the delegated views call boundary, or document why an idiomatic
  internalized Solidity view path is acceptable for the comparison.
- Expand scenarios beyond two coins before claiming full NG equivalence.

## `yearn_vault_v3`

Exact now:

- The Vyper benchmark implementation is the pinned upstream
  `contracts/VaultV3.vy`.
- The benchmark deploys the Vyper source through the blueprint/minimal-proxy
  path used by the upstream vault.
- The Solidity port covers the listed upstream vault API: initialization,
  ERC4626 deposit/mint/withdraw/redeem overloads, ERC20 share transfers,
  permit, role bitmasks, role-manager handoff, mutable metadata, deposit and
  withdraw limit modules, default/custom queues, strategy add/revoke/debt
  flows, process-report accounting, locked-profit shares, shutdown, and
  optional-return asset transfers.
- Scenarios now cover self-report idle asset accrual, accountant fees/refunds,
  excessive-fee rejection, reentrant accountant rejection, realized and
  unrealized loss paths, module acceptance/rejection, long-queue bounds, and
  permit before/after initialization.
- First-class management scenarios now measure max-debt updates, additive role
  grants, pending role-manager transfer, accountant updates, default-queue
  toggles, withdraw-limit module updates, and shutdown with an active deposit
  limit module.
- The scenario set includes a combined management sequence that mutates
  delegated roles, the default queue, debt, reporting, limit modules, shutdown
  state, and then withdraws from the post-sequence vault state.
- The generated differential harness normalizes deployment-specific vault,
  asset, strategy, accountant, and module addresses and compares event/log
  hashes for the listed scenarios.

Remaining:

- The Solidity port still needs a function-by-function and sequence-level
  parity audit against the pinned Vyper source before the spec can honestly set
  `production_equivalence: true`.
- Third-party accountant, deposit-limit module, withdraw-limit module, and
  strategy behavior is represented by deterministic harness mocks. The current
  scenarios cover important boundary paths but not exhaustive adversarial or
  unusual implementations behind those interfaces.
- The current sequence coverage proves one representative combined management
  ordering; other orderings and repeated transitions remain to be covered.
- Vyper bounded `String[64]`, `String[32]`, and `DynArray[address, MAX_QUEUE]`
  ABI behavior is approximated in Solidity with runtime checks; success/failure
  is covered, but decoder timing and revert data are not exact.
- Exact storage layout compatibility is intentionally false for the Solidity
  port. That is acceptable for an idiomatic source comparison only after the
  behavior audit proves no storage-layout-dependent surface is being claimed.

Suggested next chips:

- Build a source-to-port checklist from every external and internal Yearn
  function, then close or scenario-cover each unchecked branch.
- Add more sequence tests for alternate role, queue, debt, report, module,
  shutdown, and withdrawal orderings.
