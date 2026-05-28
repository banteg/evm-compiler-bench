# Real-Derived Two-Language Equivalence Inventory

Last audited: 2026-05-25 on `dev`.

This is the scope inventory for the real-derived two-language equivalence
corpus. For `latest_syntax_original` benchmarks, the checked-in implementation
path is the modernized source-language original.
Normalized result rows expose the materialized `source_path` and `source_hash`
that were compiled for each profile-specific source variant. The run manifest
records per-benchmark `source_variants` with profile, source variant, path,
hash, and compile status. Compiled variant paths must live under
`target/bench-source-variants/<profile_id>/...`; provenance fields
`source_path`, `source_reference_path`, and `source_blob` identify the vendored
upstream reference under the implementation's `upstream/` directory.
`cargo run --release -- validate` enforces the reference blob without treating
that historical source as the compiled comparison artifact.
Normalized row provenance keeps `comparison_lane` as the benchmark-level lane
and uses `implementation_lane` for the compiled artifact's source or
counterpart side.

The target corpus is an idiomatic cross-language comparison, not an exhaustive
audit harness. A counterpart-language port is considered in-scope when these
conditions hold:

- The port is an idiomatic implementation of the upstream contract behavior
  selected for the benchmark, with explicit scope boundaries.
- The spec, normalized output provenance, and run manifest list any
  `excluded_features` that are outside the benchmark scope.
- ABI shape, access control, accounting, external-call behavior, token return
  handling, events, and intended success/revert behavior have been audited.
- Language-level ABI decoder timing and exact revert bytes are not required to
  match for idiomatic cross-language ports unless the upstream contract exposes
  or depends on them. The semantic boundary, such as accepted lengths, rejected
  lengths, and post-state, still must be covered.
- Scenario coverage is broad enough to catch representative cross-feature
  interactions, not every branch, adversarial hook, or ordering permutation.
- A benchmark-scoped no-cache run and `validate` pass. The full matrix and
  report build are still required before release.

Storage slot layout is tracked separately. It does not have to match when the
comparison is intentionally idiomatic, unless upstream behavior depends on that
layout.

Every real-derived spec also declares lane metadata:

- `upstream_exact_historical`: exact pinned protocol source, compiled with its
  historical compiler lane. This remains available as a lane label, but it is
  provenance/reference material rather than the intended production-conformance
  benchmark source.
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

`source_lane` is the checked-in source-language original side,
`counterpart_lane` is the cross-language port side, and `comparison_lane` is the
benchmark-level lane used by legacy report consumers. Current real-derived
specs use `comparison_lane: production_conformance` and
`counterpart_lane: latest_idiomatic`. The target source side is
`source_lane: latest_syntax_original`, with pinned upstream files retained only
as provenance/reference inputs.

## Focused Compatibility Checks

The latest-syntax originals are also checked against representative historical
compiler profiles through generated source variants. On 2026-05-25, these
no-cache compile probes passed with zero failures:

Those historical profiles are compatibility coverage for the modernized
checked-in source. They do not switch the benchmark back to the vendored
upstream historical file, which remains provenance-only for this lane.

| Benchmark | Profiles |
| --- | --- |
| `uniswap_v2_pair` | `solc-0.5.16-noopt`, `solc-0.5.16-legacy-runs200`, `vyper-0.3.10-none`, `vyper-0.3.10-gas` |
| `curve_stableswap_2coin` | `vyper-0.3.10-none`, `vyper-0.3.10-gas`, `solc-0.8.20-noopt`, `solc-0.8.20-legacy-runs200` |
| `yearn_vault_v2` | `solc-0.4.26-noopt`, `solc-0.4.26-legacy-runs200`, `solc-0.8.20-noopt`, `solc-0.8.20-legacy-runs200`, `vyper-latest-none`, `vyper-latest-gas` |
| `yearn_vault_v3` | `vyper-0.3.7-default`, `vyper-0.3.7-none`, `solc-0.8.20-noopt`, `solc-0.8.20-legacy-runs200` |

These are compile-surface checks only. They prove the current source-variant
rewrites are sufficient for those older profiles, not that the counterpart
ports cover additional benchmark behavior.

## Summary

| Benchmark | Comparison lane | Source lane | Counterpart lane | Exact source-language side | Counterpart status | Main blockers |
| --- | --- | --- | --- | --- | --- | --- |
| `uniswap_v2_pair` | `production_conformance` | `latest_syntax_original` | `latest_idiomatic` | Latest-syntax Solidity modernization of upstream `UniswapV2Pair.sol`, with pinned upstream retained for provenance. | Vyper port covers the pair hot path, LP-token surface, and covered factory-management branches. | Intentional boundaries remain for factory fixture artifact shape, Vyper's bounded flash callback payload, exhaustive decoder permutations, exact revert bytes, and storage layout. |
| `curve_stableswap_2coin` | `production_conformance` | `latest_syntax_original` | `latest_idiomatic` | Latest-syntax Vyper modernization of upstream `CurveStableSwapNG.vy`, with pinned upstream retained for provenance. | Solidity port covers two-coin standard, oracle, rebasing, and ERC4626 harness tokens, plus representative three-coin and MAX_COINS dynamic-N canaries. | Dynamic-N coverage is representative rather than exhaustive; factory/views topology is kept as a single delegated quote-view canary; revert-data and decoder-timing details remain approximate. |
| `yearn_vault_v2` | `production_conformance` | `latest_syntax_original` | `latest_idiomatic` | Latest-syntax Vyper modernization of upstream `contracts/Vault.vy`, with pinned upstream retained for provenance. | Solidity port covers the monolithic V2 vault API, ERC20 share accounting and permit, deposits/withdrawals, governance/management controls, strategy queue/debt/report accounting, locked profit, and optional-return ERC20 transfers. | Intentional boundaries remain for deterministic asset/strategy fixtures, representative common workflows, exact Vyper bounded-string/bytes decoder behavior, revert data, and storage layout. |
| `yearn_vault_v3` | `production_conformance` | `latest_syntax_original` | `latest_idiomatic` | Latest-syntax Vyper modernization of upstream `VaultV3.vy`, with pinned upstream retained for provenance. | Solidity port covers common vault API, management, strategy accounting, module, queue, and permit usage with idiomatic Solidity dynamic strings and queue storage. | Intentional boundaries remain for representative third-party fixtures, representative common workflow sequences, exact Vyper bounded-string decoder behavior, and storage layout; the `MAX_QUEUE` cap remains modeled as vault behavior. |

## Status Legend

- Exact source: the checked-in source-language implementation is the
  source-language original for the active lane. For `latest_syntax_original`,
  `validate` enforces the vendored upstream reference hash separately from the
  compiled modernized implementation.
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
- Incomplete: still blocks the stated benchmark scope.

## Granular Inventory

### `uniswap_v2_pair`

| Surface | Status | Notes |
| --- | --- | --- |
| Pair source-language original | Latest-syntax original | `UniswapV2PairReal.sol` is a latest-syntax Solidity modernization of the pinned upstream pair source; the pinned upstream file remains provenance/reference input only. |
| Factory ownership of `initialize` | Fixture-exact | The pair-side factory gate, CREATE2 deployment path, pair mappings, `allPairs` tracking and public getter bounds, `feeTo`/`feeToSetter` authorization including old-setter rejection after authority transfer, same-order and reverse-order duplicate-pair guards, and invalid-pair guards are covered; the full upstream factory contract is not the benchmark target. |
| LP ERC20 metadata, balances, allowances, transfers | Exact counterpart surface | Metadata, balances, `approve`, `transfer`, upstream zero-recipient transfer behavior, insufficient-balance transfer rejection, finite/infinite-allowance `transferFrom`, upstream zero-recipient `transferFrom` behavior, and insufficient-allowance rejection are implemented and scenario-covered. |
| Pair identity and EIP-712 getters | Exact counterpart surface | `factory`, `PERMIT_TYPEHASH`, and `MINIMUM_LIQUIDITY` getters are directly scenario-covered; deployment-specific `token0`, `token1`, and `DOMAIN_SEPARATOR` values are covered through normalized observers. |
| EIP-2612 permit | Exact counterpart surface | Valid signatures, invalid signatures, expired deadlines, and acceptance after post-deploy chain-id drift through the constructor-time domain separator are covered. |
| Reserve accounting and `getReserves` | Exact counterpart surface | Vyper uses native `uint112` reserve fields plus a native `uint32` timestamp field; reserve overflow rejection and timestamp wrap behavior are covered. |
| Mint, burn, swap, skim, sync | Exact counterpart surface | Initial/subsequent mint, upstream zero-recipient LP mint behavior, initial mint below `MINIMUM_LIQUIDITY` rejection, burn including zero-recipient output transfers and no-staged-LP burn rejection, invariant and dual-output swaps, zero-output and insufficient-liquidity swap guards, drift, skim including zero-recipient transfers, sync, and timestamp wrap paths are covered. |
| Protocol-fee `kLast` behavior | Exact counterpart surface | Fee-on minting and fee-off reset are covered. |
| Optional-return token handling | Exact counterpart surface | No-return transfer-out paths are covered for swap, burn, and skim. |
| Flash-swap callback | Exact counterpart surface under idiomatic-scope policy | Ordinary non-empty callback repayment and reentrancy rejection are covered. The Vyper port keeps an idiomatic `Bytes[4096]` ABI bound instead of emulating Solidity's unbounded `bytes calldata`; oversized callback payload conformance is tracked as a language-level semantic boundary rather than a port implementation gap. |
| Revert data and ABI boundary behavior | Representative boundary | Success/failure is covered for important paths plus unknown selectors, truncated initializer, pair-action, LP-token approve/transfer/transferFrom, malformed swap head, missing/short/overlapping dynamic calldata tails, and truncated permit payloads including signature-tail truncation. Exhaustive decoder-boundary permutations and exact revert bytes are intentionally out of scope for the headline benchmark. |
| Storage layout | Tracked separately | The Vyper port uses idiomatic storage rather than matching the upstream packed reserve word. Externally observable uint112/uint32 reserve semantics are preserved, while slot layout remains outside the claimed behavioral equivalence surface unless a slot-dependent behavior is added. |

Immediate chips:

- Exact revert bytes and exhaustive ABI decoder permutations remain out of scope
  beyond the representative malformed-calldata classes already covered.

### `curve_stableswap_2coin`

| Surface | Status | Notes |
| --- | --- | --- |
| Pool source-language original | Latest-syntax original | `CurveStableSwapNG.vy` is a latest-syntax Vyper modernization of the pinned upstream pool source; the pinned upstream file remains provenance/reference input only. |
| Two-coin constructor setup | Exact counterpart surface for `N_COINS = 2` | The harness deploys matched standard, oracle-rate, rebasing, and ERC4626 two-coin pools. |
| Dynamic `N_COINS` generality | Representative canaries | Upstream supports constructor-driven coin counts up to `MAX_COINS = 8`; the headline benchmark keeps one three-coin setup canary and MAX_COINS canaries for imbalanced add-liquidity, interior exchange, and imbalanced removal. |
| Add/remove liquidity and exchange paths | Exact counterpart surface for two coins | Balanced, imbalanced, one-coin, standard exchange, and `exchange_received` paths are covered. |
| NG stored-rate, oracle, rebasing, ERC4626 behavior | Exact counterpart surface for fixtures | Constructor-provided multipliers, oracles, rebasing flags, and ERC4626 rates are covered through deterministic fixtures. |
| Moving-average oracle decay | Exact counterpart surface | Price and D oracle scenarios advance time and cover exponential decay. |
| Dynamic/off-peg fees and admin fees | Exact counterpart surface | Fee quotes, exchange accounting, and admin-fee withdrawal are covered. |
| Admin controls | Exact counterpart surface | Ramp, stop-ramp, fee updates, moving-average windows, public admin-fee withdrawal, and non-admin rejection for factory-admin-gated setters are covered. |
| LP token and permit | Exact counterpart surface | EOA and ERC1271 permit success plus invalid permit failure are covered. |
| Factory and views dependencies | Fixture canary | Both implementations call the benchmark-provided factory/views fixture for one delegated `get_dy` quote-view row. Additional delegated quote rows are demoted because they mostly measure topology rather than pool math. |
| `StableSwapViews` call topology in Solidity | Exact counterpart surface, lightly sampled | The Solidity port mirrors upstream by routing quote views through `factory.views_implementation()`, but the headline scenario set keeps only one delegated topology canary. |
| Vyper `DynArray[MAX_COINS]` ABI bounds | Idiomatic counterpart divergence | Ignored extra amount entries are covered where both implementations accept them. Too-long Vyper decoder rejections are out of scope for the idiomatic Solidity counterpart. |
| Storage layout | Tracked separately | The Solidity port is idiomatic and not storage-layout-compatible; this is outside the claimed behavioral equivalence surface unless a slot-dependent behavior is added. |

Immediate chips:

- Decide whether every NG action must be repeated at each constructor coin
  count through `MAX_COINS`, or whether the current constructor-driven port
  plus representative three-, five-, and eight-coin coverage is the intended
  production-equivalence boundary.

### `yearn_vault_v2`

| Surface | Status | Notes |
| --- | --- | --- |
| Vault source-language original | Latest-syntax original | `Vault.vy` is a latest-syntax Vyper modernization of the pinned upstream Yearn V2 vault source; the pinned upstream file remains provenance/reference input only. |
| Initialization and mutable metadata | Exact counterpart surface | All three upstream initialization overloads are modeled, including default guardian/management behavior, name/symbol overrides, decimals import from the asset token, and governance-managed metadata setters. |
| ERC20 shares and permit | Exact counterpart surface | Share balances, approvals, finite/infinite allowance spends, increase/decrease allowance, EIP-712 packed-signature permit, nonces, and transfer receiver guards are implemented. |
| Deposits and withdrawals | Representative common-path surface | Default, max-amount, receiver-specific, and deposit-limit paths are covered, along with vault share/accounting observations and withdrawal through the strategy queue. |
| Governance, management, guardian controls | Exact counterpart surface for covered API | Governance handoff, management/rewards/guardian updates, fee/deposit-limit updates, shutdown toggling, sweep, withdrawal queue updates, and strategy parameter updates are implemented. |
| Strategy lifecycle and accounting | Representative common-path surface | Add/revoke/migrate/queue operations, credit/debt/expected-return views, report gain/loss/debt-payment flows, locked-profit decay, and withdrawal-from-strategy accounting are implemented against deterministic strategy fixtures. Exhaustive strategy implementation behavior is intentionally out of scope. |
| Optional-return token handling | Exact counterpart surface | The Vyper source uses `default_return_value=True`; the Solidity port accepts no-return and true-return token transfers in the same helper boundary. |
| Vyper bounds and revert data | Approximate | The source Vyper original retains bounded `String` and `Bytes` ABI behavior. The Solidity counterpart uses idiomatic dynamic types where Solidity naturally does so; exact decoder timing and revert bytes are outside the headline comparison. |
| Storage layout | Approximate | Full storage-layout compatibility is intentionally false for the idiomatic Solidity port. |

Immediate chips:

- Additional Yearn V2 scenarios should stay focused on common vault workflows,
  not exhaustive strategy, decoder, or revert-data edge cases.

### `yearn_vault_v3`

| Surface | Status | Notes |
| --- | --- | --- |
| Vault source-language original | Latest-syntax original | `VaultV3.vy` is a latest-syntax Vyper modernization of the pinned upstream vault source; the pinned upstream file remains provenance/reference input only. |
| Blueprint/minimal-proxy deployment | Exact source path | The upstream Vyper benchmark is deployed through the blueprint/clone path. |
| ERC4626 deposit, mint, withdraw, redeem | Exact counterpart surface | Direct/default-argument overloads, deposit-all, no-return/false-return asset transfers, zero/max-uint conversion boundaries, and direct deposit-limit equality are scenario-covered. |
| ERC20 share accounting and permit | Exact counterpart surface | Transfers, receiver rejection, approvals, finite/infinite allowance spends, EIP-712 permit before/after initialization, permit after chain-id changes, expired permits, and invalid permits are covered. |
| Role bitmasks and role-manager handoff | Exact counterpart surface | Set/add/remove role, delegated execution, bounds, pending transfer, and acceptance are covered. |
| Metadata setters | Exact counterpart surface | Name and symbol initialization/setters use idiomatic Solidity strings on the counterpart side; exact Vyper bounded-string decoder behavior is out of scope. |
| Strategy registry and debt management | Exact counterpart surface | Add, revoke, force revoke, inactive-management rejection, re-add after revoke/force-revoke, max debt, debt increase/decrease, minimum-idle clipping and no-available-idle return, report gain moving current debt above max debt, unrealized-loss assessment boundaries, max-loss defaults, strategy maxDeposit/maxRedeem limits, unrealized-loss queue breaks, shutdown pull-only, and buy-debt inactive/current-debt/amount/clipping/rejection paths are covered. |
| Report accounting and locked profit | Representative common-path surface | Strategy zero, profit, loss, accountant/protocol-fee report, unlock-over-time, strategy-loss withdrawal, and unrealized-loss assessment paths are covered. Exhaustive report-value combinatorics are intentionally out of scope for the idiomatic benchmark. |
| Default/custom withdrawal queues | Exact counterpart surface | Default queue, custom queue, queue order, representative strategy maxRedeem behavior, strategy-debt withdrawal, and the `MAX_QUEUE` semantic cap are modeled; exact Vyper decoder timing and revert bytes remain out of scope. |
| Limit modules and accountant dependencies | Fixture-exact representative surface | Deterministic and refund-mutating accountant mocks cover representative fee/refund paths; deterministic module mocks cover accept/reject and exact-limit paths, zero and high-return deposit/withdraw/redeem execution and capping, high-return deposit-limit module execution through both deposit and mint, active-module maxDeposit after existing vault assets, receiver/owner-specific asset/share max-view returns including zero-return special cases, max-loss-specific and queue-specific withdraw module argument forwarding through max views and execution paths, receiver-specific deposit and mint execution/rejection, owner-specific withdraw execution and rejection, post-gain and non-1:1 partial-unlock `maxMint`/`maxRedeem` conversion, exact-limit deposit/mint/withdraw/redeem execution, deposit/mint/withdraw/redeem over-limit rejection, zero/vault-receiver short-circuiting before deposit-module calls, and reverting deposit/withdraw-module calls across asset and share max views including zero-balance owners. Arbitrary custom accountant/module behavior is intentionally out of scope. Direct deposit-limit equality is covered outside the module path. |
| Cross-feature sequence behavior | Representative common-path surface | Withdrawal and redeem sequences cover representative management orderings, including active non-shutdown withdrawal, a three-strategy repeated-transition path, and mutating-accountant/module withdraw and shutdown-redeem paths. Exhaustive ordering permutations are out of scope for the idiomatic benchmark. |
| Vyper `String` and `DynArray` bounds | Mixed | The source Vyper original remains bounded. The Solidity counterpart uses native dynamic strings for metadata, but retains the meaningful `MAX_QUEUE` cap for queue arrays. Exact decoder timing and revert bytes are intentionally out of scope for the headline comparison. |
| Function-by-function map | Scope tracker | The source-to-port checklist maps upstream functions to counterpart surfaces and records any explicit benchmark-scope notes in `docs/yearn-v3-source-port-checklist.md`. |
| Storage layout | Approximate | Full storage-layout compatibility is intentionally false for the idiomatic Solidity port. |

Immediate chips:

- No additional Yearn matrix-expansion work is tracked for the idiomatic benchmark.

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
- The Vyper port uses native uint112 reserve fields and a native uint32
  timestamp field, preserving the upstream reserve overflow guard, and
  `default_return_value=True` transfer handling for no-return ERC20s while
  still rejecting explicit false-return transfers.
- Exact storage layout compatibility is intentionally false for the rest of the
  Vyper port. That is acceptable for an idiomatic source comparison because the
  benchmark claims externally observable pair behavior, while preserving
  uint112/uint32 reserve bounds in idiomatic storage.
- Scenarios cover initial and subsequent mints, initial mint rejection below
  `MINIMUM_LIQUIDITY`, `token0`, `token1`, `factory`, `DOMAIN_SEPARATOR`,
  factory CREATE2 deployment, same-order and reverse-order duplicate factory
  guards, exact upstream token0-input and token1-input
  swap invariant checks, zero-output and insufficient-liquidity swap guards,
  one-wei over-output K rejection for each input side, no-staged-LP burn rejection,
  no-return token transfers, false-return transfer rejection across swap, burn,
  and skim, ordinary flash callback repayment, flash reentrancy rejection,
  fee-on/off behavior, timestamp wrapping, reserve overflow rejection, LP
  transfer/allowance failures, raw ABI-boundary rejection for representative
  pair actions, LP-token balance/allowance/nonce/spend calls, swap head, missing
  dynamic tail, short and overlapping dynamic payload calldata, and permit payloads including signature-tail truncation, and permit
  success/failure.
- The generated differential harness normalizes deployment-specific addresses
  and compares event/log hashes for the listed scenarios.

Remaining:

- Upstream `swap` accepts unbounded `bytes calldata`. Vyper requires a bounded
  byte array for the same ABI `bytes` shape, and the current port uses
  `Bytes[4096]` as a realistic callback-payload ceiling. Oversized payload
  conformance is an accepted idiomatic language-level semantic boundary for the
  primary port, not a reason to introduce a low-level emulation variant.
- The benchmark CREATE2 fixture now exposes the upstream `createPair(address,address)`
  ABI and mirrors upstream token sorting, zero/identical/same-order and
  reverse-order duplicate-pair guards, bidirectional `getPair` storage,
  `allPairs` tracking and public getter bounds, `PairCreated`, `feeTo`, and
  `feeToSetter` authorization including post-transfer old-setter rejection.
  The factory helper is intentionally constructed with the active pair bytecode
  so both language artifacts can share the same factory path while pair runtime
  behavior and bytecode size remain owned by the separate pair benchmark.
- Exact revert bytes and exhaustive decoder-boundary permutations are
  intentionally out of scope beyond the representative malformed-calldata
  scenarios, direct getters, permit, LP-token zero-recipient behavior plus
  allowance/balance rejection, reserve, symmetric swap receiver guards, and
  pair-action scenarios already covered.

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
  donation-before-first-deposit handling, two-coin add/remove liquidity,
  exchange, `exchange_received`, one-coin withdrawal, admin controls, permit,
  dynamic-fee and admin-fee accounting, slippage and invalid-coin reverts, plus
  ignored extra amount entries where both implementations accept them.
- Dynamic-N coverage is intentionally represented by a three-coin
  initial-liquidity canary and three MAX_COINS canaries: imbalanced
  add-liquidity, interior exchange, and imbalanced removal.
- The delegated views topology is intentionally represented by a single
  `get_dy` quote-view canary, because additional `get_dx`,
  `calc_token_amount`, and `dynamic_fee` quote rows mostly exercise the
  benchmark views fixture rather than compiler optimization of pool math.
- Price and D oracle scenarios advance time and exercise the upstream NG
  exponential moving-average decay path for the two-coin headline pool rather
  than only same-block oracle upkeep.
- Permit scenarios now cover both EOA EIP-712 signatures and the upstream
  ERC1271 smart-contract-wallet validation path.
- The generated differential harness normalizes deployment-specific pool and
  coin addresses and compares event/log hashes for the listed scenarios.

Remaining:

- Upstream `CurveStableSwapNG` is generic over `N_COINS` from constructor input
  up to `MAX_COINS = 8`; the Solidity port has dynamic array state, but the
  headline scenario set deliberately keeps only representative dynamic-N
  canaries instead of repeating every NG action at every possible constructor
  coin count.
- The pool exposes multiple delegated quote views through `StableSwapViews`;
  only `get_dy` remains in the headline set as a topology canary.
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
- Scenarios now cover representative common strategy report paths: zero report,
  profit report, loss report, accountant/protocol-fee report,
  unlock-over-time observation, loss withdrawal, and unrealized-loss
  assessment. Exhaustive report-value combinations are intentionally out of
  scope for this idiomatic benchmark. Additional scenarios cover
  receiver/owner-specific limit-module asset/share max-view returns,
  post-gain and non-1:1 partial-unlock `maxMint`/`maxRedeem` conversion,
  module acceptance/rejection, high-return deposit-limit module execution
  through deposit and mint,
  high-return withdraw-limit execution through withdraw and redeem, capping,
  withdraw module `max_loss` and
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

- The source-to-port checklist tracks benchmark scope and function mapping
  without requiring exhaustive report-value combinatorics before release.
- Third-party accountant behavior is represented by deterministic and
  refund-mutating harness mocks; deposit-limit module, withdraw-limit module,
  and strategy behavior is represented by deterministic harness mocks. The
  current scenarios cover representative common and boundary paths, including
  share-based over-limit rejection, receiver/owner zero-return special cases,
  receiver/owner short-circuits, and refund balance/allowance mutation.
  Arbitrary custom accountant/module implementations are intentionally out of
  scope.
- The current sequence coverage proves six representative combined management
  orderings, including active non-shutdown withdrawal and repeated transitions
  across queues, debt, reports, modules, and role-manager handoff. Third-party
  edge cases now include mutating-accountant/module withdraw and shutdown
  redeem sequences, but exhaustive unusual implementation behavior remains out
  of scope.
- Vyper bounded `String[64]` and `String[32]` decoder bounds are not mirrored
  in the Solidity port. `DynArray[address, MAX_QUEUE]` remains modeled as a
  queue-length cap because that bound affects vault behavior; exact decoder
  timing and revert bytes are still out of scope.
- Exact storage layout compatibility is intentionally false for the Solidity
  port. That is acceptable for an idiomatic source comparison because the
  benchmark claims externally observable vault behavior, not storage-layout
  compatibility.
