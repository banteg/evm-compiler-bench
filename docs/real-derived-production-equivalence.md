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
- Language-level ABI decoder timing and exact revert bytes are not required to
  match for idiomatic cross-language ports unless the upstream contract exposes
  or depends on them. The semantic boundary, such as accepted lengths, rejected
  lengths, and post-state, still must be covered.
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
| `uniswap_v2_pair` | Vendored upstream Solidity `UniswapV2Pair.sol` at the pinned blob. | Vyper port covers the pair hot path and LP-token surface. | Vyper `Bytes[65536]` callback bound vs upstream unbounded `bytes calldata`; full factory behavior is represented by a benchmark fixture; final ABI/revert audit still pending. |
| `curve_stableswap_2coin` | Vendored upstream Vyper `CurveStableSwapNG.vy` at the pinned blob. | Solidity port covers a two-coin NG deployment across standard, oracle, rebasing, and ERC4626 harness tokens, plus three-coin and eight-coin standard-token coverage. | Solidity port has moved to constructor-driven `N_COINS` for the covered standard-token paths, but not every NG action is covered at every coin count; factory/views dependencies are harness fixtures; DynArray decoder details remain approximate. |
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
  topology, or a language-level bound. This can be acceptable for production
  equivalence only when the semantic boundary is covered and the source behavior
  does not expose or depend on the lower-level detail.
- Incomplete: still blocks `production_equivalence: true`.

## Granular Inventory

### `uniswap_v2_pair`

| Surface | Status | Notes |
| --- | --- | --- |
| Upstream pair source | Exact source | `UniswapV2Pair.sol` is vendored at the pinned blob. |
| Factory ownership of `initialize` | Fixture-exact | The pair-side factory gate and CREATE2 deployment path are covered; the full upstream factory contract is not the benchmark target. |
| LP ERC20 metadata, balances, allowances, transfers | Exact counterpart surface | Metadata, balances, `approve`, `transfer`, insufficient-balance transfer rejection, finite/infinite-allowance `transferFrom`, and insufficient-allowance rejection are implemented and scenario-covered. |
| Pair identity and EIP-712 getters | Exact counterpart surface | `factory`, `PERMIT_TYPEHASH`, and `MINIMUM_LIQUIDITY` getters are directly scenario-covered; deployment-specific `token0`, `token1`, and `DOMAIN_SEPARATOR` values are covered through normalized observers. |
| EIP-2612 permit | Exact counterpart surface | Valid signatures, invalid signatures, expired deadlines, and acceptance after post-deploy chain-id drift through the constructor-time domain separator are covered. |
| Reserve packing and `getReserves` | Exact counterpart surface | Vyper packs `reserve0`, `reserve1`, and `blockTimestampLast` into the upstream bit layout; reserve overflow rejection is covered. |
| Mint, burn, swap, skim, sync | Exact counterpart surface | Initial/subsequent mint, initial mint below `MINIMUM_LIQUIDITY` rejection, burn and no-staged-LP burn rejection, invariant swap, zero-output and insufficient-liquidity swap guards, drift, skim, sync, and timestamp wrap paths are covered. |
| Protocol-fee `kLast` behavior | Exact counterpart surface | Fee-on minting and fee-off reset are covered. |
| Optional-return token handling | Exact counterpart surface | No-return transfer-out paths are covered for swap, burn, and skim. |
| Flash-swap callback | Approximate | Non-empty, reentrant, larger-than-old-1024-byte, larger-than-old-4096-byte, and exact-65536-byte data are covered, but Vyper still has a `Bytes[65536]` ABI bound while upstream Solidity accepts unbounded `bytes calldata`. |
| Revert data and ABI boundary behavior | Incomplete | Success/failure is covered for important paths, but exhaustive revert-data and decoder-boundary parity has not been audited. |
| Storage layout | Approximate | Packed reserves intentionally match; the rest is idiomatic Vyper storage and not full layout-compatible. |

Immediate chips:

- Decide whether the `Bytes[65536]` bound is an accepted language-policy
  exception, or keep the port non-production-equivalent for truly unbounded
  callback calldata.
- Decide whether the benchmark remains pair-only or must include the full
  upstream factory implementation.
- Complete the ABI/revert audit for the pair ABI outside the current scenarios.

### `curve_stableswap_2coin`

| Surface | Status | Notes |
| --- | --- | --- |
| Upstream pool source | Exact source | `CurveStableSwapNG.vy` is vendored at the pinned blob. |
| Two-coin constructor setup | Exact counterpart surface for `N_COINS = 2` | The harness deploys matched standard, oracle-rate, rebasing, and ERC4626 two-coin pools. |
| Dynamic `N_COINS` generality | Partial | Upstream supports constructor-driven coin counts up to `MAX_COINS = 8`; the Solidity port now has dynamic array state plus three-coin liquidity, imbalanced liquidity deposit, `get_dy`/`get_dx`, `calc_token_amount` deposit/withdraw, and `dynamic_fee` quotes, exchange, exchange_received, proportional withdrawal, imbalanced withdrawal, one-coin withdrawal, oracle-update scenarios, and eight-coin initial liquidity, imbalanced liquidity deposit, `get_dy`, `get_dx`, `dynamic_fee`, `calc_token_amount` deposit/withdraw, `calc_withdraw_one_coin`, endpoint exchange, endpoint exchange slippage rejection, endpoint exchange_received, proportional withdrawal, imbalanced withdrawal, one-coin withdrawal, one-coin withdrawal slippage rejection, and oracle-decay coverage. |
| Add/remove liquidity and exchange paths | Exact counterpart surface for two coins | Balanced, imbalanced, one-coin, standard exchange, and `exchange_received` paths are covered. |
| NG stored-rate, oracle, rebasing, ERC4626 behavior | Exact counterpart surface for fixtures | Constructor-provided multipliers, oracles, rebasing flags, and ERC4626 rates are covered through deterministic fixtures. |
| Moving-average oracle decay | Exact counterpart surface | Price and D oracle scenarios advance time and cover exponential decay. |
| Dynamic/off-peg fees and admin fees | Exact counterpart surface | Fee quotes, exchange accounting, and admin-fee withdrawal are covered. |
| Admin controls | Exact counterpart surface | Ramp, stop-ramp, fee updates, moving-average windows, public admin-fee withdrawal, and non-admin rejection for factory-admin-gated setters are covered. |
| LP token and permit | Exact counterpart surface | EOA and ERC1271 permit success plus invalid permit failure are covered. |
| Factory and views dependencies | Fixture-exact | Both implementations call the benchmark-provided factory/views fixture. |
| `StableSwapViews` call topology in Solidity | Exact counterpart surface for quote views | The Solidity port now mirrors upstream by routing `get_dy`, `get_dx`, `dynamic_fee`, and `calc_token_amount` through `factory.views_implementation()`. |
| Vyper `DynArray[MAX_COINS]` ABI bounds | Approximate | Too-long arrays and ignored extra entries are covered, but Solidity enforces this with runtime checks, not Vyper decoder behavior. |
| Storage layout | Approximate | The Solidity port is idiomatic and not storage-layout-compatible. |

Immediate chips:

- Decide whether production equivalence means a full generic NG Solidity port
  or an explicitly production-equivalent two-coin specialization.
- If full NG is the target, replace the fixed two-coin arrays and loops with
  constructor-driven `N_COINS` behavior and add non-two-coin scenarios.

### `yearn_vault_v3`

| Surface | Status | Notes |
| --- | --- | --- |
| Upstream vault source | Exact source | `VaultV3.vy` is vendored at the pinned blob. |
| Blueprint/minimal-proxy deployment | Exact source path | The upstream Vyper benchmark is deployed through the blueprint/clone path. |
| ERC4626 deposit, mint, withdraw, redeem | Exact counterpart surface, audit pending | Direct/default-argument overloads, deposit-all, no-return/false-return asset transfers, zero/max-uint conversion boundaries, and direct deposit-limit equality are scenario-covered. |
| ERC20 share accounting and permit | Exact counterpart surface, audit pending | Transfers, receiver rejection, approvals, finite/infinite allowance spends, EIP-712 permit before/after initialization, permit after chain-id changes, expired permits, and invalid permits are covered. |
| Role bitmasks and role-manager handoff | Exact counterpart surface, audit pending | Set/add/remove role, delegated execution, bounds, pending transfer, and acceptance are covered. |
| Metadata setters | Exact counterpart surface, audit pending | Name and symbol setters plus Vyper string length success/failure boundaries are covered with Solidity runtime checks under the semantic-boundary policy. |
| Strategy registry and debt management | Exact counterpart surface, audit pending | Add, revoke, force revoke, inactive-management rejection, re-add after revoke/force-revoke, max debt, debt increase/decrease, minimum-idle clipping and no-available-idle return, report gain moving current debt above max debt, unrealized-loss assessment boundaries, max-loss defaults, strategy maxDeposit/maxRedeem limits, unrealized-loss queue breaks, shutdown pull-only, and buy-debt clipping/rejection paths are covered. |
| Report accounting and locked profit | Exact counterpart surface, audit pending | Profit, loss, self-report idle gain/loss, self-report idle gain with accountant fees/refunds, self-report zero-effective clipped refund, accountant fees/refunds, gain plus clipped refund locking, gain that moves current debt above max debt, gain/fee equality, gain/fee/refund exact offset, gain-with-refund net-loss fee recalculation, net-positive, exact-offset, and net-negative mixed loss/fee/refund reports, refund clipping to partial and zero effective refunds, refund clipping after accountant state mutation, zero-return accountant reports, loss/no-lock/net-loss fee recalculation, partial-unlock profit/loss reports with accountant effects, protocol fees, excessive-fee failure, reentrancy failure, unlock-over-time, and zero-reset paths are covered. |
| Default/custom withdrawal queues | Exact counterpart surface, audit pending | Default queue, custom queue, forced default queue, queue order, duplicate entries, full-queue append skipping, long-queue failures, strategy maxRedeem limits, zero-redeem after full unrealized loss, and partial/over strategy redeems are covered. |
| Limit modules and accountant dependencies | Fixture-exact | Deterministic and refund-mutating accountant mocks cover important fee/refund paths; deterministic module mocks cover accept/reject and exact-limit paths, zero and high-return deposit/withdraw execution and capping, receiver/owner-specific asset/share max-view returns, post-gain and non-1:1 partial-unlock `maxMint`/`maxRedeem` conversion, exact-limit deposit/mint/withdraw/redeem execution, zero/vault-receiver short-circuiting before deposit-module calls, and reverting calls, but arbitrary third-party behavior is not exhaustive. Direct deposit-limit equality is covered outside the module path. |
| Cross-feature sequence behavior | Incomplete | Withdrawal and redeem sequences now cover three management orderings, including a three-strategy repeated-transition path; more adversarial third-party behavior remains. |
| Vyper `String` and `DynArray` bounds | Exact counterpart surface, audit pending | Accepted and rejected lengths are covered at the semantic boundary; exact decoder timing and revert bytes are intentionally out of scope unless source behavior depends on them. |
| Function-by-function parity audit | Incomplete | The source-to-port checklist now maps every upstream function and tracks the remaining branch gaps in `docs/yearn-v3-source-port-checklist.md`. |
| Storage layout | Approximate | Full storage-layout compatibility is intentionally false for the idiomatic Solidity port. |

Immediate chips:

- Close the open Yearn branch gaps tracked in
  `docs/yearn-v3-source-port-checklist.md`.
- Add edge-case scenarios for repeated transitions across roles, queues, debt,
  reports, modules, shutdown, withdrawals, and redeems.
- Expand third-party module/strategy mock coverage and any remaining unusual
  third-party accountant paths beyond refund balance/allowance mutation.

## `uniswap_v2_pair`

Exact now:

- The Solidity benchmark implementation is the pinned upstream
  `contracts/UniswapV2Pair.sol`.
- Supporting upstream Solidity sources for ERC20, factory, interfaces, and
  libraries are vendored next to the pair source.
- The Vyper port implements the matched pair API: initialization, reserves,
  mint, burn, swap, skim, sync, fee-on `kLast`, cumulative prices, LP ERC20
  accounting, and permit.
- The Vyper port uses packed reserves with the upstream bit layout, including
  the upstream uint112 reserve overflow guard, and
  `default_return_value=True` transfer handling for no-return ERC20s while
  still rejecting explicit false-return transfers.
- Scenarios cover initial and subsequent mints, initial mint rejection below
  `MINIMUM_LIQUIDITY`, `token0`, `token1`, `factory`, `DOMAIN_SEPARATOR`,
  factory CREATE2 deployment, token0-input and upstream
  token1-input swap invariant checks, zero-output and insufficient-liquidity
  swap guards, one-wei over-output K rejection, no-staged-LP burn rejection,
  no-return token transfers, false-return transfer rejection across swap, burn,
  and skim, flash callback repayment through callback data above the old 4 KiB
  port bound and at the current 64 KiB bound, flash reentrancy rejection,
  fee-on/off behavior, timestamp wrapping, reserve overflow rejection, LP
  transfer/allowance failures, and permit success/failure.
- The generated differential harness normalizes deployment-specific addresses
  and compares event/log hashes for the listed scenarios.

Remaining:

- Upstream `swap` accepts unbounded `bytes calldata`. Vyper requires a bounded
  byte array and the current port uses `Bytes[65536]`, with scenarios now
  covering payloads above the old 4 KiB port bound and exactly at the current
  64 KiB bound. Decide whether this
  language-level bound keeps the benchmark non-production-equivalent, or
  document an explicit policy exception before removing the excluded feature.
- The benchmark CREATE2 fixture exercises the pair's factory-owned initialize
  path and `feeTo`, but it is not the full upstream `UniswapV2Factory`.
- The final audit still needs to check revert reasons or decoder failures where
  they matter, and any ABI entry whose boundary behavior is not already covered
  by direct getter, permit, LP-token, reserve, or pair-action scenarios.

Suggested next chips:

- Decide whether the remaining 64 KiB Vyper callback-data bound is an accepted
  language-level semantic boundary, or keep it listed as an excluded feature.
- Decide whether full upstream factory behavior is in scope for this benchmark
  or whether the benchmark is explicitly "Pair only".

## `curve_stableswap_2coin`

Exact now:

- The Vyper benchmark implementation is the pinned upstream
  `contracts/main/CurveStableSwapNG.vy`.
- The Solidity port implements a matched constructor-driven pool API for the
  covered paths, including add/remove liquidity, exchange, `exchange_received`,
  one-coin withdrawal, amplification and fee admin controls, LP ERC20
  accounting, permit, moving averages, stored rates, and admin-fee accounting.
- Scenarios cover standard ERC20s, no-return ERC20s, oracle-rate assets,
  donation-before-first-deposit handling, initial and imbalanced three-coin and
  eight-coin liquidity, three-coin `get_dy`/`get_dx`, `calc_token_amount` deposit/withdraw, and `dynamic_fee` quote views, eight-coin quote views including `calc_token_amount` deposit/withdraw, three-coin exchange and
  `exchange_received`, proportional three-coin and eight-coin withdrawal,
  imbalanced three-coin and eight-coin withdrawal, three-coin and eight-coin one-coin withdrawal,
  eight-coin endpoint exchange and endpoint exchange slippage rejection,
  eight-coin one-coin withdrawal slippage rejection,
  rebasing asset behavior, ERC4626 rate scaling, dynamic fees, admin controls,
  slippage and invalid coin reverts, and Vyper DynArray length edges for the
  two-coin deployment.
- Price and D oracle scenarios now advance time and exercise the upstream NG
  exponential moving-average decay path for two-coin, three-coin, and
  eight-coin deployments rather than only same-block oracle upkeep.
- Permit scenarios now cover both EOA EIP-712 signatures and the upstream
  ERC1271 smart-contract-wallet validation path.
- The generated differential harness normalizes deployment-specific pool and
  coin addresses and compares event/log hashes for the listed scenarios.

Remaining:

- Upstream `CurveStableSwapNG` is generic over `N_COINS` from constructor input
  up to `MAX_COINS = 8`; the Solidity port now has dynamic array state and
  three-coin initial-liquidity, imbalanced-liquidity deposit, `get_dy`/`get_dx`, `calc_token_amount` deposit/withdraw, and `dynamic_fee` quotes, exchange, proportional-withdrawal,
  imbalanced-withdrawal, one-coin-withdrawal, oracle-update scenarios, and
  eight-coin initial-liquidity, imbalanced-liquidity deposit, `get_dy`, `get_dx`, `dynamic_fee`,
  `calc_token_amount` deposit/withdraw, `calc_withdraw_one_coin`, endpoint-exchange,
  endpoint-exchange slippage rejection,
  endpoint `exchange_received`,
  proportional-withdrawal, imbalanced-withdrawal, one-coin-withdrawal, and
  one-coin-withdrawal slippage rejection
  coverage, plus eight-coin oracle-decay coverage, but every NG action has not
  been repeated at every possible
  constructor coin count.
- The factory, admin, fee receiver, rate oracle, rebasing token, and ERC4626
  dependencies are deterministic benchmark fixtures, not full upstream
  deployments.
- Solidity runtime length checks approximate Vyper `DynArray` decoder bounds;
  revert data and decoder timing are not exact.

Suggested next chips:

- Decide whether every NG action must be repeated at `N_COINS = 8`, or whether
  the current three-coin surface plus eight-coin boundary checks are the desired
  production-equivalence boundary for standard-token dynamic-N behavior.

## `yearn_vault_v3`

Exact now:

- The Vyper benchmark implementation is the pinned upstream
  `contracts/VaultV3.vy`.
- The benchmark deploys the Vyper source through the blueprint/minimal-proxy
  path used by the upstream vault.
- The Solidity port covers the listed upstream vault API: initialization,
  ERC4626 deposit/mint/withdraw/redeem overloads, ERC20 share transfers,
  permit, role bitmasks, role-manager handoff, mutable metadata, deposit and
  withdraw limit modules, post-shutdown deposit-limit management rejection,
  default/custom queues, strategy add/revoke/debt flows, process-report
  accounting, locked-profit shares, shutdown, and
  optional-return asset transfer/transferFrom/approve handling.
- Scenarios now cover self-report idle asset accrual/loss, self-report idle
  gains with accountant fees/refunds, accountant fees/refunds,
  gain plus clipped refund locking, receiver/owner-specific limit-module
  asset/share max-view returns, post-gain and non-1:1 partial-unlock
  `maxMint`/`maxRedeem` conversion, mixed loss/fee/refund reports,
  gain-with-refund net-loss fee recalculation, zero-return accountant reports,
  refund clipping by balance/allowance, refund balance/allowance mutation
  during accountant reports, excessive-fee rejection, reentrant accountant
  rejection, realized and unrealized loss paths, protocol-fee splits on gain
  and loss reports, locked-profit zero reset, module acceptance/rejection,
  high-return withdraw-limit capping, withdraw module `max_loss` and
  strategy-queue argument forwarding, long-queue bounds, no-return/false-return
  asset handling, and permit before/after initialization plus after chain-id
  changes.
- Strategy edge scenarios now cover limited and zero `maxDeposit` behavior
  during debt increases, minimum-idle clipping/restoration on debt changes,
  limited `maxRedeem`, max-debt-below-current, shutdown pull-only, and actual
  redeem variance during debt decreases.
- Strategy withdrawal scenarios now cover limited `maxRedeem` caps and
  zero-redeem queue fallthrough.
- Strategy withdrawal accounting now covers actual redeem returns below and
  above the vault-requested amount.
- Buy-debt scenarios now cover over-current-debt clipping and zero-share
  rejection.
- First-class management scenarios now measure max-debt updates, additive role
  grants, pending role-manager transfer, accountant updates, default-queue
  toggles, withdraw-limit module updates, and shutdown with an active deposit
  limit module.
- The scenario set includes three combined management sequences that mutate
  delegated roles, role-manager authority, the default/custom queue, debt,
  reporting, limit modules, shutdown state, and then withdraw or redeem from the
  post-sequence vault state. The newest sequence exercises a three-strategy
  queue rewrite, repeated default-queue toggles, debt increase/decrease cycles,
  multiple reports, repeated module updates, and delegated emergency shutdown.
- The generated differential harness normalizes deployment-specific vault,
  asset, strategy, accountant, and module addresses and compares event/log
  hashes for the listed scenarios.
- `docs/yearn-v3-source-port-checklist.md` maps every upstream external and
  internal function to the Solidity port and tracks the remaining branch gaps.

Remaining:

- The source-to-port checklist still has open branch gaps before the spec can
  honestly set `production_equivalence: true`.
- Third-party accountant behavior is represented by deterministic and
  refund-mutating harness mocks; deposit-limit module, withdraw-limit module,
  and strategy behavior is represented by deterministic harness mocks. The
  current scenarios cover important boundary paths and receiver/owner
  short-circuits, but not exhaustive adversarial or unusual implementations
  behind those interfaces.
- The current sequence coverage proves three representative combined management
  orderings, including repeated transitions across queues, debt, reports,
  modules, and role-manager handoff. Third-party edge cases remain to be
  covered.
- Vyper bounded `String[64]`, `String[32]`, and `DynArray[address, MAX_QUEUE]`
  success/failure boundaries are covered. The Solidity port uses runtime
  checks rather than Vyper decoder rejection, which is acceptable under the
  semantic-boundary policy because the vault does not expose or depend on the
  exact decoder timing or revert bytes.
- Exact storage layout compatibility is intentionally false for the Solidity
  port. That is acceptable for an idiomatic source comparison only after the
  behavior audit proves no storage-layout-dependent surface is being claimed.

Suggested next chips:

- Close the open branch gaps listed in
  `docs/yearn-v3-source-port-checklist.md`.
- Add more edge-case scenarios for repeated role, queue, debt, report, module,
  shutdown, withdrawal, and redeem transitions.
