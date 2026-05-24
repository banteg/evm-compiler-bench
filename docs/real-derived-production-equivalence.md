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
- Vyper bounded `String[64]`, `String[32]`, and `DynArray[address, MAX_QUEUE]`
  ABI behavior is approximated in Solidity with runtime checks; success/failure
  is covered, but decoder timing and revert data are not exact.
- Exact storage layout compatibility is intentionally false for the Solidity
  port. That is acceptable for an idiomatic source comparison only after the
  behavior audit proves no storage-layout-dependent surface is being claimed.

Suggested next chips:

- Build a source-to-port checklist from every external and internal Yearn
  function, then close or scenario-cover each unchecked branch.
- Add sequence tests that combine role changes, queue changes, debt changes,
  reports, module changes, shutdown, and withdrawals in the same run.
