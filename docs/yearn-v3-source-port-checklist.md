# Yearn V3 Source-to-Port Checklist

Last refreshed: 2026-05-24 on `dev`.

This tracks the pinned upstream Vyper `VaultV3.vy` against the idiomatic
Solidity port `YearnVaultV3Real.sol`. It is a working checklist for removing
the `production_equivalence: false` guard; it is not yet a claim of full
production equivalence.

Source pair:

- Upstream source:
  `benches/implementations/yearn_vault_v3/vyper/upstream/contracts/VaultV3.vy`
  at pinned blob `97f4a60dd2485c6400f994e46b442d0424716eed`.
- Counterpart port:
  `benches/implementations/yearn_vault_v3/solidity/YearnVaultV3Real.sol`.
- Differential scenarios:
  `benches/scenarios/yearn_vault_v3.yaml`.

Status meanings:

- `mapped`: source function has a counterpart implementation and the broad
  success/revert shape matches the upstream source on audited branches.
- `covered`: at least one differential scenario exercises the function or
  branch.
- `open`: more scenario or adversarial mock coverage is still needed before
  this can support `production_equivalence: true`.
- `approximate`: the Solidity port matches success/failure intent but cannot
  exactly reproduce Vyper ABI decoder timing, revert data, or bounded type
  mechanics. Exact decoder timing and revert bytes are not production
  equivalence blockers when the semantic boundary is covered and the source
  behavior does not expose or depend on those lower-level details.

## External Surface

| Upstream function | Port counterpart | Status | Remaining work |
| --- | --- | --- | --- |
| `__init__` | `constructor` plus harness minimal proxy deployment | mapped, covered | Keep deployment explicitly clone-based; direct implementation initialization is intentionally not the measured path. |
| `initialize` | `initialize` | mapped, covered | String success/failure bounds are covered under the semantic-boundary policy. |
| `setName` | `setName` | mapped, covered | String success/failure bounds are covered under the semantic-boundary policy. |
| `setSymbol` | `setSymbol` | mapped, covered | String success/failure bounds are covered under the semantic-boundary policy. |
| `set_accountant` | `set_accountant` | mapped, covered | Third-party accountant behavior remains harness-scoped. |
| `set_default_queue` | `set_default_queue` | mapped, covered | Duplicate active strategy entries are covered. |
| `set_use_default_queue` | `set_use_default_queue` | mapped, covered | Covered directly and inside combined sequences. |
| `set_auto_allocate` | `set_auto_allocate` | mapped, covered | Covered with a default-queue strategy deposit and a minimum-idle-not-met deposit. |
| `set_deposit_limit` | overloaded `set_deposit_limit` | mapped, covered | Default direct-limit update, active-module rejection, active-module override clearing, post-shutdown rejection, and the direct deposit-limit equality boundary are covered. |
| `set_deposit_limit_module` | overloaded `set_deposit_limit_module` | mapped, covered | Default module update, finite-direct-limit rejection, direct-limit override reset, post-shutdown rejection, reverting module calls through `maxDeposit` and `maxMint`, zero-return `maxDeposit`/`maxMint`, finite, exact-limit, active-module maxDeposit after existing vault assets, post-gain and non-1:1 partial-unlock `maxMint` conversion, and max-uint deposit/mint/max-view behavior, receiver-specific module returns through `maxDeposit`, `maxMint`, deposit execution, mint execution, deposit/mint over-limit rejection, and zero/vault-receiver short-circuiting before module calls are covered; more unusual module return values remain open. |
| `set_withdraw_limit_module` | `set_withdraw_limit_module` | mapped, covered | Limit capping above and below balance, zero-return `maxWithdraw`/`maxRedeem`, finite, exact-limit, non-1:1 partial-unlock `maxRedeem` conversion, and max-uint withdraw/redeem/max-view behavior, owner-specific, max-loss-specific, and queue-specific module returns through `maxWithdraw`, `maxRedeem`, withdraw execution, redeem execution, withdraw/redeem rejection, and reverting module calls through `maxWithdraw` and `maxRedeem` including zero-balance owners are covered; queue-specific module argument forwarding is covered in max-view, withdraw execution, and redeem execution paths; more unusual module return values remain open. |
| `set_minimum_total_idle` | `set_minimum_total_idle` | mapped, covered | Debt increase clipping, decrease-side reserve restoration, and no-available-idle early return preserve the configured idle reserve. |
| `setProfitMaxUnlockTime` | `setProfitMaxUnlockTime` | mapped, covered | Zero-reset branch with locked shares is now covered by `reset_profit_unlock_after_report`. |
| `set_role` | `set_role` | mapped, covered | Solidity enforces role bit bounds explicitly because Vyper enum decoding does it before function body. |
| `add_role` | `add_role` | mapped, covered | Same enum-bound approximation as `set_role`. |
| `remove_role` | `remove_role` | mapped, covered | Same enum-bound approximation as `set_role`. |
| `transfer_role_manager` | `transfer_role_manager` | mapped, covered | Covered in pending and accepted paths. |
| `accept_role_manager` | `accept_role_manager` | mapped, covered | Covered directly and inside combined sequences. |
| `isShutdown` | `isShutdown` | mapped, covered | Covered as observer. |
| `unlockedShares` | `unlockedShares` | mapped, covered | Covered before and after reports, including partial-unlock loss reports. |
| `pricePerShare` | `pricePerShare` | mapped, covered | Covered as observer before and after reports. |
| `get_default_queue` | `get_default_queue` | mapped, covered | Covered through normalized queue-id observer. |
| `process_report` | `process_report` | mapped, covered | Inactive-strategy rejection, strategy and self zero reports, strategy zero reports with accountant fees/refunds and protocol-fee splits, self-report refunds including zero-effective clipping, self-report idle gain/loss with accountant fees/refunds and protocol-fee splits, gain plus clipped refunds, gain that moves current debt above max debt, protocol-fee splits on gain and loss reports including loss reports with refunds, gain/fee/refund exact offset, gain-with-refund net-loss fee recalculation, no-lock refund reports, zero-effective clipped refunds, third-party accountant refund state mutation, same-strategy partial-unlock profit/loss reports with accountant effects, and cross-strategy loss reporting after another strategy's partially unlocked profit report are covered; more strategy reporting variants remain open. |
| `buy_debt` | `buy_debt` | mapped, covered | Inactive-strategy rejection, zero-current-debt rejection, zero-amount rejection, over-current-debt clipping, and zero-share rejection are covered. |
| `add_strategy` | overloaded `add_strategy` | mapped, covered | Queue-full append-skip, zero-address rejection, active-strategy rejection, and default-queue append behavior are covered. |
| `revoke_strategy` | `revoke_strategy` | mapped, covered | Covered for normal removal, inactive-strategy rejection, active-debt rejection, and re-add after revoke. |
| `force_revoke_strategy` | `force_revoke_strategy` | mapped, covered | Covered for force removal, inactive-strategy rejection, and re-add after forced debt accounting. |
| `update_max_debt_for_strategy` | `update_max_debt_for_strategy` | mapped, covered | Covered for active-strategy update and inactive-strategy rejection. |
| `update_debt` | overloaded `update_debt` | mapped, covered | Equal-current-debt rejection, strategy `maxDeposit`/`maxRedeem` limits, minimum-idle clipping and no-available-idle early return, max-debt-below-current, shutdown pull-only, and actual-withdrawal loss/over-return branches are covered. |
| `shutdown_vault` | `shutdown_vault` | mapped, covered | Covered with and without deposit-limit module, including post-shutdown debt pull. |
| `deposit` | `deposit` | mapped, covered | `max_value(uint256)` deposit-all branch is covered. |
| `mint` | `mint` | mapped, covered | Covered directly and through preview observers. |
| `withdraw` | overloaded `withdraw` | mapped, covered | Default max-loss overload, max-loss upper-bound rejection, long-queue, custom/default selection, limited strategy redeem, zero-redeem queue fallthrough, and partial/over strategy redeem returns are covered. |
| `redeem` | overloaded `redeem` | mapped, covered | Default max-loss overload, max-loss upper-bound rejection, long-queue and custom/default selection covered; limited and partial/over strategy redeem behavior is covered through withdraw. |
| `approve` | `approve` | mapped, covered | Covered directly. |
| `transfer` | `transfer` | mapped, covered | Share movement plus insufficient-balance, receiver zero, and vault-self rejection are covered. |
| `transferFrom` | `transferFrom` | mapped, covered | Finite and infinite allowance paths, insufficient allowance, insufficient owner balance, and receiver guards are covered. |
| `permit` | `permit` | mapped, covered | Valid, expired, invalid, pre-initialization, and post-chain-id-change permit paths are covered. |
| `balanceOf` | `balanceOf` | mapped, covered | Vault-self locked-share branch covered through observers after reports. |
| `totalSupply` | `totalSupply` | mapped, covered | Covered as observer. |
| `totalAssets` | `totalAssets` | mapped, covered | Covered as observer. |
| `totalIdle` | `totalIdle` | mapped, covered | Covered as observer. |
| `totalDebt` | `totalDebt` | mapped, covered | Covered as observer. |
| `convertToShares` | `convertToShares` | mapped, covered | Zero-assets and max-uint special cases are covered. |
| `previewDeposit` | `previewDeposit` | mapped, covered | Zero-assets and max-uint special cases are covered through the shared conversion path. |
| `previewMint` | `previewMint` | mapped, covered | Zero-shares and max-uint special cases are covered through the shared conversion path. |
| `convertToAssets` | `convertToAssets` | mapped, covered | Zero-shares and max-uint special cases are covered. |
| `maxDeposit` | `maxDeposit` | mapped, covered | Zero receiver and vault receiver branches are covered. |
| `maxMint` | `maxMint` | mapped, covered | Zero receiver, vault receiver, unlimited deposit limit, and module-return paths are covered. |
| `maxWithdraw` | overloaded `maxWithdraw` | mapped, covered | Limited strategy redeem and unrealized-loss queue-break behavior are covered. |
| `maxRedeem` | overloaded `maxRedeem` | mapped, covered | Limited strategy redeem and unrealized-loss queue-break behavior are covered through the shared max-withdraw path. |
| `previewWithdraw` | `previewWithdraw` | mapped, covered | Rounded-up conversion plus zero and max-uint branches are covered. |
| `previewRedeem` | `previewRedeem` | mapped, covered | Rounded-down conversion plus zero and max-uint branches are covered. |
| `FACTORY` | `FACTORY` | mapped, covered | Factory is harness-provided; full factory behavior is out of scope. |
| `apiVersion` | `apiVersion` | mapped, covered | Covered as observer. |
| `assess_share_of_unrealised_losses` | `assess_share_of_unrealised_losses` | mapped, covered | Current-debt `< assets_needed` revert is covered. |
| `profitMaxUnlockTime` | `profitMaxUnlockTime` | mapped, covered | Covered as observer. |
| `fullProfitUnlockDate` | `fullProfitUnlockDate` | mapped, covered | Covered as observer. |
| `profitUnlockingRate` | `profitUnlockingRate` | mapped, covered | Covered as observer. |
| `lastProfitUpdate` | `lastProfitUpdate` | mapped, covered | Covered as observer. |
| `DOMAIN_SEPARATOR` | `DOMAIN_SEPARATOR` | mapped, covered | Live chain-id behavior is covered through `permit` after a chain-id change. |

## Internal Logic

| Upstream helper | Port counterpart | Status | Remaining work |
| --- | --- | --- | --- |
| `_spend_allowance` | `_spendAllowance` | mapped, covered | Finite, infinite, and insufficient allowance branches are covered. |
| `_transfer` | `_transfer` | mapped, covered | Receiver checks happen in external wrappers, with zero and vault receiver rejection covered. |
| `_transfer_from` | external `transferFrom` plus `_spendAllowance` and `_transfer` | mapped, covered | Finite and infinite allowance branches, insufficient allowance, insufficient owner balance, plus zero/vault receiver guards are covered. |
| `_approve` | `_approve` | mapped, covered | Covered by approve and permit paths. |
| `_permit` | `permit` plus `domain_separator` | mapped, covered | Post-chain-id-change signatures are covered. |
| `_burn_shares` | `_burnShares` | mapped, covered | Locked-share zero reset now covered. |
| `_unlocked_shares` | `_unlockedShares` | mapped, covered | Partial-unlock plus subsequent profit and loss/fee reports are covered. |
| `_total_supply` | `_effectiveSupply` | mapped, covered | Covered as observer. |
| `_total_assets` | `totalAssets` | mapped, covered | Covered as observer. |
| `_convert_to_assets` | `_convertToAssets` | mapped, covered | Max-uint and zero-value special cases are covered. |
| `_convert_to_shares` | `_convertToShares` | mapped, covered | Max-uint and zero-value special cases are covered. |
| `_erc20_safe_approve` | `_safeApproveToken` | mapped, covered | No-return and false-return approve paths are covered. |
| `_erc20_safe_transfer_from` | `_safeTransferFromToken` | mapped, covered | No-return and false-return transferFrom paths are covered. |
| `_erc20_safe_transfer` | `_safeTransferToken` | mapped, covered | No-return and false-return transfer paths are covered. |
| `_issue_shares` | `_issueShares` | mapped, covered | Covered by deposit, reports, and fees. |
| `_max_deposit` | `_maxDeposit` | mapped, covered | Module and receiver boundary cases are covered through both asset and share max views. |
| `_max_withdraw` | `_maxWithdraw` | mapped, covered | Limited strategy redeem, module owner boundary cases through both asset and share max views, and nonzero unrealized-loss queue break are covered. |
| `_deposit` | `_deposit` | mapped, covered | Auto-allocate and deposit-all entry paths are covered. |
| `_assess_share_of_unrealised_losses` | `_assessShareOfUnrealisedLosses` | mapped, covered | Current-debt boundary branches are covered. |
| `_withdraw_from_strategy` | `_withdrawFromStrategy` | mapped, covered | Partial and over-returning strategy redeems are covered. |
| `_redeem` | `_redeem`, `_withdrawFromQueue`, `_withdrawFromQueueStrategy` | mapped, covered | Strategy maxRedeem limit, zero-redeem fallthrough, zero-redeem after full unrealized loss, queue-specific withdraw-limit module forwarding during withdraw and redeem, and partial/over strategy redeem returns are covered. |
| `_add_strategy` | `_addStrategy` | mapped, covered | Queue-full append-skip branch is covered. |
| `_revoke_strategy` | `_revokeStrategy` | mapped, covered | Non-forced debt revert and re-add after normal or forced revoke are covered. |
| `_update_debt` | `_updateDebt` | mapped, covered | Equal-current-debt rejection, strategy max-deposit zero, limited-deposit, minimum-idle clipping and no-available-idle early return, max-redeem, max-debt-below-current, shutdown pull-only, and actual-withdrawal loss/over-return branches are covered. |
| `_process_report` | `_processReport` plus report-state helpers | mapped, covered | Inactive-strategy rejection, strategy and self zero reports, zero-report protocol-fee splits, plain strategy loss, self-report gain/loss/refunds including zero-effective clipping, self-report idle gain/loss with accountant fees/refunds and protocol-fee splits, third-party accountant refund state mutation, fee/refund clipping including zero-effective refunds, gain plus clipped refund locking, gain that moves current debt above max debt, protocol-fee splits on gain and loss reports including loss reports with refunds, gain/fee equality, gain/fee/refund exact offset, gain-with-refund net-positive and net-loss fee/refund paths, net-positive and net-negative mixed loss/fee/refund reports, loss/no-lock/net-loss fee recalculation, gain and loss no-lock refund reports, same-strategy partial-unlock profit/loss reports with accountant effects, cross-strategy loss reporting after another strategy's partially unlocked profit report, and repeated profit-lock weighting are covered; remaining strategy reporting variants remain open. |
| `_enforce_role` | `_enforceRole` | mapped, covered | Vyper enum decoding is approximated by explicit Solidity role bounds at external role-mutator entry points. |
| `domain_separator` | `domain_separator` | mapped, covered | Live chain-id behavior is covered through `permit` after a chain-id change. |

## Open Checklist

These items should be closed before flipping `yearn_vault_v3` to
`production_equivalence: true`:

- Add any remaining adversarial strategy report value combinations beyond the
  covered current-debt-above-max-debt, net-positive/exact-offset/net-negative mixed loss/fee/refund,
  gain/fee equality, gain/fee/refund exact offset, gain-with-refund net-positive, and gain-with-refund fee-recalculation,
  net-loss with refund/protocol-fee splits, gain and loss no-profit-lock refund reports, self-report, same-strategy and cross-strategy partial-unlock with accountant effects, and accountant-mutation paths.
