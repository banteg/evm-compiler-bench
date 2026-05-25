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

Every real-derived spec also declares lane metadata:

- `upstream_exact_historical`: exact pinned protocol source, compiled with its
  historical compiler lane. This is provenance/reference material, not the
  intended production-conformance benchmark source.
- `latest_syntax_original`: upstream-derived original source modernized to the
  checked-in latest syntax baseline. Older compiler rows are produced from this
  source by generated pragma and compatibility rewrites where possible.
- `latest_idiomatic`: modern high-level source in each language; this is the
  intended headline compiler comparison lane.
- `production_conformance`: broad real-contract behavior coverage against
  upstream-derived scope. This lane is for parity and regression evidence, not
  for latest-vs-latest headline comparisons.
- `diagnostic_layout_matched`: manual packing, assembly, unsafe math, or other
  parity tricks useful for diagnosis but not headline comparison.
- `fixture_scoped_port`: real contract behavior over deterministic benchmark
  fixtures. This is a source/counterpart lane for harness-dependent ports, not
  a benchmark-level headline lane.

`source_lane` is the checked-in source-language original side,
`counterpart_lane` is the cross-language port side, and `comparison_lane` is the
benchmark-level lane used by legacy report consumers. Current real-derived
specs use `comparison_lane: production_conformance` and
`counterpart_lane: fixture_scoped_port`. The target source side is
`source_lane: latest_syntax_original`, with pinned upstream files retained only
as provenance/reference inputs; benchmarks still marked
`upstream_exact_historical` have not been modernized yet.

## Summary

| Benchmark | Comparison lane | Source lane | Counterpart lane | Exact source-language side | Counterpart status | Main blockers |
| --- | --- | --- | --- | --- | --- | --- |
| `uniswap_v2_pair` | `production_conformance` | `latest_syntax_original` | `fixture_scoped_port` | Latest-syntax Solidity modernization of upstream `UniswapV2Pair.sol`, with pinned upstream retained for provenance. | Vyper port covers the pair hot path and LP-token surface. | Full factory behavior is represented by a benchmark fixture; final ABI/revert audit still pending. |
| `curve_stableswap_2coin` | `production_conformance` | `latest_syntax_original` | `fixture_scoped_port` | Latest-syntax Vyper modernization of upstream `CurveStableSwapNG.vy`, with pinned upstream retained for provenance. | Solidity port covers constructor-driven NG deployments across two-coin standard, oracle, rebasing, and ERC4626 harness tokens, plus standard-token coverage for every `N_COINS` value from 2 through 8. | Not every NG action is covered at every coin count; factory/views dependencies are harness fixtures; revert-data and decoder-timing details remain approximate. |
| `yearn_vault_v3` | `production_conformance` | `latest_syntax_original` | `fixture_scoped_port` | Latest-syntax Vyper modernization of upstream `VaultV3.vy`, with pinned upstream retained for provenance. | Solidity port covers the main vault API, management paths, strategy accounting, modules, queues, and permit. | Full parity audit is still pending for cross-feature sequences and third-party module/accountant/strategy edge cases; Vyper bounded `String` and `DynArray` ABI behavior is approximated with Solidity runtime checks. |

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
| Pair source-language original | Latest-syntax original | `UniswapV2PairReal.sol` is a latest-syntax Solidity modernization of the pinned upstream pair source; the pinned upstream file remains provenance/reference input only. |
| Factory ownership of `initialize` | Fixture-exact | The pair-side factory gate, CREATE2 deployment path, pair mappings, `allPairs` tracking and public getter bounds, `feeTo`/`feeToSetter` authorization including old-setter rejection after authority transfer, same-order and reverse-order duplicate-pair guards, and invalid-pair guards are covered; the full upstream factory contract is not the benchmark target. |
| LP ERC20 metadata, balances, allowances, transfers | Exact counterpart surface | Metadata, balances, `approve`, `transfer`, upstream zero-recipient transfer behavior, insufficient-balance transfer rejection, finite/infinite-allowance `transferFrom`, upstream zero-recipient `transferFrom` behavior, and insufficient-allowance rejection are implemented and scenario-covered. |
| Pair identity and EIP-712 getters | Exact counterpart surface | `factory`, `PERMIT_TYPEHASH`, and `MINIMUM_LIQUIDITY` getters are directly scenario-covered; deployment-specific `token0`, `token1`, and `DOMAIN_SEPARATOR` values are covered through normalized observers. |
| EIP-2612 permit | Exact counterpart surface | Valid signatures, invalid signatures, expired deadlines, and acceptance after post-deploy chain-id drift through the constructor-time domain separator are covered. |
| Reserve packing and `getReserves` | Exact counterpart surface | Vyper packs `reserve0`, `reserve1`, and `blockTimestampLast` into the upstream bit layout; reserve overflow rejection is covered. |
| Mint, burn, swap, skim, sync | Exact counterpart surface | Initial/subsequent mint, upstream zero-recipient LP mint behavior, initial mint below `MINIMUM_LIQUIDITY` rejection, burn including zero-recipient output transfers and no-staged-LP burn rejection, invariant and dual-output swaps, zero-output and insufficient-liquidity swap guards, drift, skim including zero-recipient transfers, sync, and timestamp wrap paths are covered. |
| Protocol-fee `kLast` behavior | Exact counterpart surface | Fee-on minting and fee-off reset are covered. |
| Optional-return token handling | Exact counterpart surface | No-return transfer-out paths are covered for swap, burn, and skim. |
| Flash-swap callback | Exact counterpart surface under idiomatic-scope policy | Non-empty, reentrant, larger-than-old-1024-byte, larger-than-old-4096-byte, and exact-65536-byte data are covered. The Vyper port keeps an idiomatic `Bytes[65536]` ABI bound instead of emulating Solidity's unbounded `bytes calldata`; this is tracked as a language-level semantic boundary rather than a port implementation gap. |
| Revert data and ABI boundary behavior | Partial | Success/failure is covered for important paths plus unknown selectors, truncated initializer, pair-action, LP-token approve/transfer/transferFrom, malformed swap head, missing/short/overlapping dynamic calldata tails, and truncated permit payloads including signature-tail truncation. Exact revert bytes and exhaustive decoder-boundary parity have not been audited. |
| Storage layout | Tracked separately | Packed reserves intentionally match because pair behavior depends on uint112/uint32 reserve semantics; the rest is idiomatic Vyper storage and outside the claimed behavioral equivalence surface unless a slot-dependent behavior is added. |

Immediate chips:

- Decide whether the benchmark remains pair-only or must include the full
  upstream factory implementation beyond the covered fixture factory branches.
- Complete the remaining ABI/revert audit for exact revert bytes and malformed calldata cases outside the current selector, fixed-argument, dynamic-tail, and permit truncation scenarios.

### `curve_stableswap_2coin`

| Surface | Status | Notes |
| --- | --- | --- |
| Upstream pool source | Exact source | `CurveStableSwapNG.vy` is vendored at the pinned blob. |
| Two-coin constructor setup | Exact counterpart surface for `N_COINS = 2` | The harness deploys matched standard, oracle-rate, rebasing, and ERC4626 two-coin pools. |
| Dynamic `N_COINS` generality | Partial | Upstream supports constructor-driven coin counts up to `MAX_COINS = 8`; the Solidity port now has dynamic array state plus standard-token coverage for every `N_COINS` value from 2 through 8. Counts 3, 5, and 8 carry representative full action coverage across liquidity, quote, exchange, withdrawal, fee, and oracle paths; counts 4, 6, and 7 now include targeted constructor-sizing plus endpoint quote, midpoint quote/exchange, optimistic-transfer exchange, deposit/withdraw quote, one-coin withdrawal quote/action, imbalanced withdrawal, proportional withdrawal, or dynamic-fee coverage for intermediate coin indexing. |
| Add/remove liquidity and exchange paths | Exact counterpart surface for two coins | Balanced, imbalanced, one-coin, standard exchange, and `exchange_received` paths are covered. |
| NG stored-rate, oracle, rebasing, ERC4626 behavior | Exact counterpart surface for fixtures | Constructor-provided multipliers, oracles, rebasing flags, and ERC4626 rates are covered through deterministic fixtures. |
| Moving-average oracle decay | Exact counterpart surface | Price and D oracle scenarios advance time and cover exponential decay. |
| Dynamic/off-peg fees and admin fees | Exact counterpart surface | Fee quotes, exchange accounting, and admin-fee withdrawal are covered. |
| Admin controls | Exact counterpart surface | Ramp, stop-ramp, fee updates, moving-average windows, public admin-fee withdrawal, and non-admin rejection for factory-admin-gated setters are covered. |
| LP token and permit | Exact counterpart surface | EOA and ERC1271 permit success plus invalid permit failure are covered. |
| Factory and views dependencies | Fixture-exact | Both implementations call the benchmark-provided factory/views fixture. |
| `StableSwapViews` call topology in Solidity | Exact counterpart surface for quote views | The Solidity port now mirrors upstream by routing `get_dy`, `get_dx`, `dynamic_fee`, and `calc_token_amount` through `factory.views_implementation()`. |
| Vyper `DynArray[MAX_COINS]` ABI bounds | Approximate | Too-long arrays and ignored extra entries are covered, but Solidity enforces this with runtime checks, not Vyper decoder behavior. |
| Storage layout | Tracked separately | The Solidity port is idiomatic and not storage-layout-compatible; this is outside the claimed behavioral equivalence surface unless a slot-dependent behavior is added. |

Immediate chips:

- Decide whether every NG action must be repeated at each constructor coin
  count through `MAX_COINS`, or whether the current constructor-driven port
  plus representative three-, five-, and eight-coin coverage is the intended
  production-equivalence boundary.

### `yearn_vault_v3`

| Surface | Status | Notes |
| --- | --- | --- |
| Vault source-language original | Latest-syntax original | `VaultV3.vy` is a latest-syntax Vyper modernization of the pinned upstream vault source; the pinned upstream file remains provenance/reference input only. |
| Blueprint/minimal-proxy deployment | Exact source path | The upstream Vyper benchmark is deployed through the blueprint/clone path. |
| ERC4626 deposit, mint, withdraw, redeem | Exact counterpart surface, audit pending | Direct/default-argument overloads, deposit-all, no-return/false-return asset transfers, zero/max-uint conversion boundaries, and direct deposit-limit equality are scenario-covered. |
| ERC20 share accounting and permit | Exact counterpart surface, audit pending | Transfers, receiver rejection, approvals, finite/infinite allowance spends, EIP-712 permit before/after initialization, permit after chain-id changes, expired permits, and invalid permits are covered. |
| Role bitmasks and role-manager handoff | Exact counterpart surface, audit pending | Set/add/remove role, delegated execution, bounds, pending transfer, and acceptance are covered. |
| Metadata setters | Exact counterpart surface, audit pending | Name and symbol setters plus Vyper string length success/failure boundaries are covered with Solidity runtime checks under the semantic-boundary policy. |
| Strategy registry and debt management | Exact counterpart surface, audit pending | Add, revoke, force revoke, inactive-management rejection, re-add after revoke/force-revoke, max debt, debt increase/decrease, minimum-idle clipping and no-available-idle return, report gain moving current debt above max debt, unrealized-loss assessment boundaries, max-loss defaults, strategy maxDeposit/maxRedeem limits, unrealized-loss queue breaks, shutdown pull-only, and buy-debt inactive/current-debt/amount/clipping/rejection paths are covered. |
| Report accounting and locked profit | Exact counterpart surface, audit pending | Profit, loss, self-report idle gain/loss, self-report idle gain/loss with accountant fees/refunds and protocol-fee splits, self-report zero-effective clipped refund, accountant fees/refunds and protocol-fee splits on zero strategy reports, gain plus clipped refund locking, gain that moves current debt above max debt, gain/fee equality, gain/fee/refund exact offset, gain-with-refund net-positive locking, gain-with-refund net-loss fee recalculation, simultaneous strategy gain/loss with accountant effects and protocol-fee splitting, net-positive, exact-offset, and net-negative mixed loss/fee/refund reports, refund clipping to partial and zero effective refunds, refund clipping after accountant state mutation on zero, gain, and loss reports, refund allowance reduction during zero, gain, and loss reports, zero-return accountant reports, loss/no-lock/net-loss fee recalculation, no-lock refund reports, same-strategy partial-unlock profit/loss reports with accountant effects, cross-strategy loss reporting after another strategy's partially unlocked profit report, protocol fees, excessive-fee failure, reentrancy failure, unlock-over-time, and zero-reset paths are covered. |
| Default/custom withdrawal queues | Exact counterpart surface, audit pending | Default queue, custom queue, forced default queue, queue order, duplicate entries, full-queue append skipping, long-queue failures, strategy maxRedeem limits, zero-redeem after full unrealized loss, and partial/over strategy redeems are covered. |
| Limit modules and accountant dependencies | Fixture-exact | Deterministic and refund-mutating accountant mocks cover important fee/refund paths; deterministic module mocks cover accept/reject and exact-limit paths, zero and high-return deposit/withdraw/redeem execution and capping, high-return deposit-limit module execution through both deposit and mint, active-module maxDeposit after existing vault assets, receiver/owner-specific asset/share max-view returns including zero-return special cases, max-loss-specific and queue-specific withdraw module argument forwarding through max views and execution paths, receiver-specific deposit and mint execution/rejection, owner-specific withdraw execution and rejection, post-gain and non-1:1 partial-unlock `maxMint`/`maxRedeem` conversion, exact-limit deposit/mint/withdraw/redeem execution, deposit/mint/withdraw/redeem over-limit rejection, zero/vault-receiver short-circuiting before deposit-module calls, and reverting deposit/withdraw-module calls across asset and share max views including zero-balance owners, but arbitrary third-party behavior is not exhaustive. Direct deposit-limit equality is covered outside the module path. |
| Cross-feature sequence behavior | Incomplete | Withdrawal and redeem sequences now cover six management orderings, including active non-shutdown withdrawal, a three-strategy repeated-transition path, and mutating-accountant/module withdraw and shutdown-redeem edge paths; more adversarial third-party behavior remains. |
| Vyper `String` and `DynArray` bounds | Exact counterpart surface, audit pending | Accepted and rejected lengths are covered at the semantic boundary; exact decoder timing and revert bytes are intentionally out of scope unless source behavior depends on them. |
| Function-by-function parity audit | Incomplete | The source-to-port checklist now maps every upstream function and tracks the remaining branch gaps in `docs/yearn-v3-source-port-checklist.md`. |
| Storage layout | Approximate | Full storage-layout compatibility is intentionally false for the idiomatic Solidity port. |

Immediate chips:

- Close the open Yearn branch gaps tracked in
  `docs/yearn-v3-source-port-checklist.md`.
- Add edge-case scenarios for repeated transitions across roles, queues, debt,
  reports, modules, shutdown, withdrawals, and redeems.
- Expand third-party module/strategy mock coverage and any remaining unusual
  third-party accountant paths beyond zero-return max views and refund
  balance/allowance mutation.

## `uniswap_v2_pair`

Exact now:

- The Solidity benchmark implementation is `UniswapV2PairReal.sol`, a
  latest-syntax modernization of the pinned upstream
  `contracts/UniswapV2Pair.sol`.
- The pinned upstream Solidity sources for ERC20, factory, interfaces, and
  libraries remain vendored as provenance/reference inputs.
- The Vyper port implements the matched pair API: initialization, reserves,
  mint, burn, swap, skim, sync, fee-on `kLast`, cumulative prices, LP ERC20
  accounting, and permit.
- The Vyper port uses packed reserves with the upstream bit layout, including
  the upstream uint112 reserve overflow guard, and
  `default_return_value=True` transfer handling for no-return ERC20s while
  still rejecting explicit false-return transfers.
- Exact storage layout compatibility is intentionally false for the rest of the
  Vyper port. That is acceptable for an idiomatic source comparison because the
  benchmark claims externally observable pair behavior, while preserving the
  packed reserve word where upstream behavior depends on uint112/uint32 bounds.
- Scenarios cover initial and subsequent mints, initial mint rejection below
  `MINIMUM_LIQUIDITY`, `token0`, `token1`, `factory`, `DOMAIN_SEPARATOR`,
  factory CREATE2 deployment, same-order and reverse-order duplicate factory
  guards, exact upstream token0-input and token1-input
  swap invariant checks, zero-output and insufficient-liquidity swap guards,
  one-wei over-output K rejection for each input side, no-staged-LP burn rejection,
  no-return token transfers, false-return transfer rejection across swap, burn,
  and skim, flash callback repayment through callback data above the old 4 KiB
  port bound and at the current 64 KiB bound, flash reentrancy rejection,
  fee-on/off behavior, timestamp wrapping, reserve overflow rejection, LP
  transfer/allowance failures, raw ABI-boundary rejection for representative
  pair actions, LP-token balance/allowance/nonce/spend calls, swap head, missing
  dynamic tail, short and overlapping dynamic payload calldata, and permit payloads including signature-tail truncation, and permit
  success/failure.
- The generated differential harness normalizes deployment-specific addresses
  and compares event/log hashes for the listed scenarios.

Remaining:

- Upstream `swap` accepts unbounded `bytes calldata`. Vyper requires a bounded
  byte array and the current port uses `Bytes[65536]`, with scenarios covering
  payloads above the old 4 KiB port bound and exactly at the current 64 KiB
  bound. This is an accepted idiomatic language-level semantic boundary for the
  primary port, not a reason to introduce a low-level emulation variant.
- The benchmark CREATE2 fixture now mirrors upstream token sorting,
  zero/identical/same-order and reverse-order duplicate-pair guards, bidirectional `getPair` storage,
  `allPairs` tracking and public getter bounds, `PairCreated`, `feeTo`, and
  `feeToSetter` authorization including post-transfer old-setter rejection, but
  its external deployment hook remains bytecode-injected so both language
  artifacts can share the same factory path.
- The final audit still needs to check revert reasons or decoder failures where
  they matter, and any ABI entry whose boundary behavior is not already covered
  by the representative malformed calldata scenarios, direct getters, permit,
  LP-token zero-recipient behavior plus allowance/balance rejection, reserve,
  symmetric swap receiver guards, or pair-action scenarios.

Suggested next chips:

- Decide whether the remaining bytecode-injected `createPair` deployment hook
  is acceptable as the factory boundary, or whether this benchmark needs a
  first-class full factory artifact before retiring the fixture caveat entirely.

## `curve_stableswap_2coin`

Exact now:

- The Vyper benchmark implementation is `CurveStableSwapNG.vy`, a
  latest-syntax modernization of the pinned upstream
  `contracts/main/CurveStableSwapNG.vy`.
- The pinned upstream Vyper source remains vendored as a provenance/reference
  input.
- The Solidity port implements a matched constructor-driven pool API for the
  covered paths, including add/remove liquidity, exchange, `exchange_received`,
  one-coin withdrawal, amplification and fee admin controls, LP ERC20
  accounting, permit, moving averages, stored rates, and admin-fee accounting.
- Scenarios cover standard ERC20s, no-return ERC20s, oracle-rate assets,
  donation-before-first-deposit handling, initial and imbalanced three-coin,
  four-coin endpoint `get_dy`/`get_dx`, exchange, one-coin withdrawal quote/action,
  `calc_token_amount` deposit/withdraw, and dynamic-fee quote,
  five-coin midpoint `get_dy`/`get_dx`, `calc_token_amount` deposit/withdraw, proportional withdrawal, imbalanced withdrawal, exchange, `exchange_received`, one-coin withdrawal, `calc_withdraw_one_coin`, dynamic-fee quote, and oracle-decay paths,
  seven-coin midpoint `get_dy`/`get_dx`, exchange, one-coin withdrawal quote/action, and
  eight-coin liquidity, three-coin `get_dy`/`get_dx`, `calc_token_amount` deposit/withdraw, and `dynamic_fee` quote views, eight-coin quote views including `calc_token_amount` deposit/withdraw, three-coin exchange and
  `exchange_received`, proportional three-coin and eight-coin withdrawal,
  imbalanced three-coin and eight-coin withdrawal, three-coin and eight-coin endpoint/interior one-coin withdrawal,
  eight-coin endpoint/interior exchange and endpoint/interior exchange slippage rejection,
  eight-coin imbalanced withdrawal slippage rejection,
  eight-coin endpoint/interior one-coin withdrawal slippage rejection,
  rebasing asset behavior, ERC4626 rate scaling, dynamic fees, admin controls,
  slippage and invalid coin reverts, and Vyper DynArray length edges for the
  two-coin deployment.
- Price and D oracle scenarios now advance time and exercise the upstream NG
  exponential moving-average decay path for two-coin, three-coin, and
  five-coin midpoint and eight-coin endpoint/interior price slots rather than only same-block oracle
  upkeep.
- Permit scenarios now cover both EOA EIP-712 signatures and the upstream
  ERC1271 smart-contract-wallet validation path.
- The generated differential harness normalizes deployment-specific pool and
  coin addresses and compares event/log hashes for the listed scenarios.

Remaining:

- Upstream `CurveStableSwapNG` is generic over `N_COINS` from constructor input
  up to `MAX_COINS = 8`; the Solidity port now has dynamic array state and
  three-coin initial-liquidity, imbalanced-liquidity deposit, add-liquidity
  slippage rejection, `get_dy`/`get_dx`, `calc_token_amount` deposit/withdraw, and `dynamic_fee` quotes, exchange,
  exchange slippage rejection, `exchange_received`, `exchange_received`
  slippage rejection, proportional-withdrawal, proportional-withdrawal
  slippage rejection, imbalanced-withdrawal, imbalanced-withdrawal
  slippage rejection, one-coin-withdrawal,
  oracle-update scenarios, five-coin midpoint/endpoint `get_dy`/`get_dx`, midpoint/endpoint exchange and `exchange_received`, `calc_token_amount` deposit/withdraw, proportional withdrawal, imbalanced withdrawal, one-coin-withdrawal, `calc_withdraw_one_coin`, dynamic-fee quote, and oracle-decay paths, and
  four-coin initial-liquidity, endpoint `get_dy`/`get_dx` quotes, endpoint
  exchange, endpoint one-coin-withdrawal quote, endpoint one-coin-withdrawal,
  deposit and withdraw `calc_token_amount` quotes, endpoint `dynamic_fee`, and endpoint
  `exchange_received`, six-coin endpoint `get_dy`/`get_dx`, midpoint exchange and
  `exchange_received`, deposit and withdraw `calc_token_amount` quotes, and
  midpoint one-coin-withdrawal quote/action, seven-coin midpoint `get_dy`/`get_dx`,
  midpoint exchange, proportional and imbalanced
  withdrawal plus midpoint one-coin-withdrawal quote, midpoint
  one-coin-withdrawal, and midpoint `dynamic_fee` quote, and eight-coin
  initial-liquidity,
  imbalanced-liquidity deposit, add-liquidity
  slippage rejection, `get_dy`, endpoint/interior `get_dx`, endpoint/interior `dynamic_fee`,
  `calc_token_amount` deposit/withdraw, endpoint/interior `calc_withdraw_one_coin`, endpoint/interior exchange,
  endpoint/interior exchange slippage rejection,
  endpoint/interior `exchange_received`, endpoint/interior `exchange_received` slippage rejection,
  proportional-withdrawal, proportional-withdrawal slippage rejection,
  imbalanced-withdrawal, imbalanced-withdrawal slippage rejection,
  endpoint/interior one-coin-withdrawal, and
  endpoint/interior one-coin-withdrawal slippage rejection
  coverage, plus eight-coin endpoint/interior oracle-decay coverage, but every NG action has not
  been repeated at every possible
  constructor coin count.
- The factory, admin, fee receiver, rate oracle, rebasing token, and ERC4626
  dependencies are deterministic benchmark fixtures, not full upstream
  deployments.
- Solidity now preserves the relevant constructor DynArray storage lengths and
  return shapes for covered pool behavior; revert data and decoder timing are
  still not exact.
- Exact storage layout compatibility is intentionally false for the Solidity
  port. That is acceptable for an idiomatic source comparison because the
  benchmark claims externally observable pool behavior, not slot-level upgrade
  or proxy compatibility.

Suggested next chips:

- Decide whether every NG action must be repeated at `N_COINS = 8`, or whether
  the current three-coin surface plus eight-coin boundary checks are the desired
  production-equivalence boundary for standard-token dynamic-N behavior.

## `yearn_vault_v3`

Exact now:

- The Vyper benchmark implementation is `VaultV3.vy`, a latest-syntax
  modernization of the pinned upstream `contracts/VaultV3.vy`.
- The pinned upstream Vyper source remains vendored as a provenance/reference
  input.
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
  gain-with-refund net-loss fee recalculation with and without protocol-fee
  splits, zero-return accountant reports,
  refund clipping by balance/allowance, refund balance/allowance mutation
  during zero and gain accountant reports, excessive-fee rejection, reentrant accountant
  rejection, realized and unrealized loss paths, protocol-fee splits on gain
  and loss reports, locked-profit zero reset, module acceptance/rejection,
  cross-strategy loss reporting after another strategy's partially unlocked
  profit report with accountant effects, and gain/loss reports with both
  accountant refunds and protocol-fee splits,
  high-return deposit-limit module execution through deposit and mint,
  high-return withdraw-limit execution through withdraw and redeem, capping,
  withdraw module `max_loss` and
  strategy-queue argument forwarding, long-queue bounds, no-return/false-return
  asset handling, and permit before/after initialization plus after chain-id
  changes.
- Strategy report scenarios now cover the no-profit-lock fee recalculation
  branch on both gain and loss reports with accountant refunds, and simultaneous
  strategy gain/loss reports with accountant effects.
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
- The scenario set includes six combined management sequences that mutate
  delegated roles, role-manager authority, the default/custom queue, debt,
  reporting, limit modules, shutdown state, and then withdraw or redeem from the
  post-sequence vault state. They include an active non-shutdown withdrawal path
  plus a three-strategy queue rewrite, repeated default-queue toggles, debt
  increase/decrease cycles, multiple reports, repeated module updates, and
  delegated emergency shutdown. The third-party edge paths combine
  mutating-accountant reports with receiver-specific deposit-module state,
  queue-specific or owner-specific withdraw-module state, role-manager handoff,
  and live explicit-queue withdrawal or shutdown redeem behavior.
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
  current scenarios cover important boundary paths, share-based over-limit
  rejection, receiver/owner zero-return special cases, receiver/owner
  short-circuits, gain-path allowance mutation, a partial-unlock mixed report where
  the accountant creates refund balance and allowance during the report hook,
  and a cross-strategy partial-unlock mixed report with hook-created refunds and
  protocol-fee splitting, but not exhaustive adversarial or unusual
  implementations behind those interfaces.
- The current sequence coverage proves six representative combined management
  orderings, including active non-shutdown withdrawal and repeated transitions
  across queues, debt, reports, modules, and role-manager handoff. Third-party
  edge cases now include mutating-accountant/module withdraw and shutdown
  redeem sequences, but exhaustive adversarial implementations remain out of
  scope.
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
