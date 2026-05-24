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
  mechanics.

## External Surface

| Upstream function | Port counterpart | Status | Remaining work |
| --- | --- | --- | --- |
| `__init__` | `constructor` plus harness minimal proxy deployment | mapped, covered | Keep deployment explicitly clone-based; direct implementation initialization is intentionally not the measured path. |
| `initialize` | `initialize` | mapped, covered | String bounds are runtime checks in Solidity, Vyper decoder bounds upstream. |
| `setName` | `setName` | mapped, covered | Revert data and decoder timing remain approximate. |
| `setSymbol` | `setSymbol` | mapped, covered | Revert data and decoder timing remain approximate. |
| `set_accountant` | `set_accountant` | mapped, covered | Third-party accountant behavior remains harness-scoped. |
| `set_default_queue` | `set_default_queue` | mapped, covered | Duplicate active strategy entries are covered. |
| `set_use_default_queue` | `set_use_default_queue` | mapped, covered | Covered directly and inside combined sequences. |
| `set_auto_allocate` | `set_auto_allocate` | mapped, covered | Covered with a default-queue strategy deposit. |
| `set_deposit_limit` | overloaded `set_deposit_limit` | mapped, covered | Shutdown/module override branches covered only partially. |
| `set_deposit_limit_module` | overloaded `set_deposit_limit_module` | mapped, covered | Direct-limit override branch is covered; unusual module behavior remains open. |
| `set_withdraw_limit_module` | `set_withdraw_limit_module` | mapped, covered | Unusual module behavior remains open. |
| `set_minimum_total_idle` | `set_minimum_total_idle` | mapped, covered | More debt update interactions remain useful. |
| `setProfitMaxUnlockTime` | `setProfitMaxUnlockTime` | mapped, covered | Zero-reset branch with locked shares is now covered by `reset_profit_unlock_after_report`. |
| `set_role` | `set_role` | mapped, covered | Solidity enforces role bit bounds explicitly because Vyper enum decoding does it before function body. |
| `add_role` | `add_role` | mapped, covered | Same enum-bound approximation as `set_role`. |
| `remove_role` | `remove_role` | mapped, covered | Same enum-bound approximation as `set_role`. |
| `transfer_role_manager` | `transfer_role_manager` | mapped, covered | Covered in pending and accepted paths. |
| `accept_role_manager` | `accept_role_manager` | mapped, covered | Covered directly and inside combined sequences. |
| `isShutdown` | `isShutdown` | mapped, covered | Covered as observer. |
| `unlockedShares` | `unlockedShares` | mapped, covered | Needs more edge coverage around partially unlocked shares plus loss/fee reports. |
| `pricePerShare` | `pricePerShare` | mapped, covered | Covered as observer before and after reports. |
| `get_default_queue` | `get_default_queue` | mapped, covered | Covered through normalized queue-id observer. |
| `process_report` | `process_report` | mapped, covered | More third-party accountant and strategy reporting variants remain open. |
| `buy_debt` | `buy_debt` | mapped, covered | Over-current-debt clipping and zero-share rejection are covered. |
| `add_strategy` | overloaded `add_strategy` | mapped, covered | Queue-full append-skip behavior is covered. |
| `revoke_strategy` | `revoke_strategy` | mapped, covered | Covered for normal removal, active-debt rejection, and re-add after revoke. |
| `force_revoke_strategy` | `force_revoke_strategy` | mapped, covered | Covered for force removal and re-add after forced debt accounting. |
| `update_max_debt_for_strategy` | `update_max_debt_for_strategy` | mapped, covered | Covered for active-strategy update and inactive-strategy rejection. |
| `update_debt` | overloaded `update_debt` | mapped, covered | Strategy `maxDeposit`/`maxRedeem` limits, max-debt-below-current, shutdown pull-only, and actual-withdrawal loss/over-return branches are covered. |
| `shutdown_vault` | `shutdown_vault` | mapped, covered | Covered with and without deposit-limit module, including post-shutdown debt pull. |
| `deposit` | `deposit` | mapped, covered | `max_value(uint256)` deposit-all branch is covered. |
| `mint` | `mint` | mapped, covered | Covered directly and through preview observers. |
| `withdraw` | overloaded `withdraw` | mapped, covered | Long-queue, custom/default selection, limited strategy redeem, zero-redeem queue fallthrough, and partial/over strategy redeem returns are covered. |
| `redeem` | overloaded `redeem` | mapped, covered | Long-queue and custom/default selection covered; limited and partial/over strategy redeem behavior is covered through withdraw. |
| `approve` | `approve` | mapped, covered | Covered directly. |
| `transfer` | `transfer` | mapped, covered | Receiver zero/self rejection is mapped but not fully covered. |
| `transferFrom` | `transferFrom` | mapped, covered | Finite and infinite allowance paths are covered. |
| `permit` | `permit` | mapped, covered | Valid, invalid, and pre-initialization permit paths covered; chain-id drift remains open. |
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
| `maxMint` | `maxMint` | mapped, covered | Zero receiver, unlimited deposit limit, and module-return paths are covered. |
| `maxWithdraw` | overloaded `maxWithdraw` | mapped, covered | Limited strategy redeem behavior is covered. |
| `maxRedeem` | overloaded `maxRedeem` | mapped, covered | Limited strategy redeem behavior is covered through the shared max-withdraw path. |
| `previewWithdraw` | `previewWithdraw` | mapped, covered | Rounded-up conversion plus zero and max-uint branches are covered. |
| `previewRedeem` | `previewRedeem` | mapped, covered | Rounded-down conversion plus zero and max-uint branches are covered. |
| `FACTORY` | `FACTORY` | mapped, covered | Factory is harness-provided; full factory behavior is out of scope. |
| `apiVersion` | `apiVersion` | mapped, covered | Covered as observer. |
| `assess_share_of_unrealised_losses` | `assess_share_of_unrealised_losses` | mapped, covered | Current-debt `< assets_needed` revert is covered. |
| `profitMaxUnlockTime` | `profitMaxUnlockTime` | mapped, covered | Covered as observer. |
| `fullProfitUnlockDate` | `fullProfitUnlockDate` | mapped, covered | Covered as observer. |
| `profitUnlockingRate` | `profitUnlockingRate` | mapped, covered | Covered as observer. |
| `lastProfitUpdate` | `lastProfitUpdate` | mapped, covered | Covered as observer. |
| `DOMAIN_SEPARATOR` | `DOMAIN_SEPARATOR` | mapped, covered | Chain-id drift remains open. |

## Internal Logic

| Upstream helper | Port counterpart | Status | Remaining work |
| --- | --- | --- | --- |
| `_spend_allowance` | `_spendAllowance` | mapped, covered | Infinite allowance branch is covered. |
| `_transfer` | `_transfer` | mapped, covered | Receiver checks happen in external wrappers, matching upstream external surface. |
| `_transfer_from` | external `transferFrom` plus `_spendAllowance` and `_transfer` | mapped, covered | Finite and infinite allowance branches are covered. |
| `_approve` | `_approve` | mapped, covered | Covered by approve and permit paths. |
| `_permit` | `permit` plus `domain_separator` | mapped, covered | Chain-id drift remains open. |
| `_burn_shares` | `_burnShares` | mapped, covered | Locked-share zero reset now covered. |
| `_unlocked_shares` | `_unlockedShares` | mapped, covered | Partial-unlock plus subsequent loss/fee report edge remains open. |
| `_total_supply` | `_effectiveSupply` | mapped, covered | Covered as observer. |
| `_total_assets` | `totalAssets` | mapped, covered | Covered as observer. |
| `_convert_to_assets` | `_convertToAssets` | mapped, covered | Max-uint and zero-value special cases are covered. |
| `_convert_to_shares` | `_convertToShares` | mapped, covered | Max-uint and zero-value special cases are covered. |
| `_erc20_safe_approve` | `_safeApproveToken` | mapped, covered | Optional-return false-return path remains open. |
| `_erc20_safe_transfer_from` | `_safeTransferFromToken` | mapped, covered | No-return token path covered; false-return path remains open. |
| `_erc20_safe_transfer` | `_safeTransferToken` | mapped, covered | No-return token path covered; false-return path remains open. |
| `_issue_shares` | `_issueShares` | mapped, covered | Covered by deposit, reports, and fees. |
| `_max_deposit` | `_maxDeposit` | mapped, covered | Module and receiver boundary cases are covered. |
| `_max_withdraw` | `_maxWithdraw` | mapped, covered | Limited strategy redeem is covered; nonzero unrealized loss queue break remains open. |
| `_deposit` | `_deposit` | mapped, covered | Auto-allocate and deposit-all entry paths are covered. |
| `_assess_share_of_unrealised_losses` | `_assessShareOfUnrealisedLosses` | mapped, covered | Current-debt boundary branches are covered. |
| `_withdraw_from_strategy` | `_withdrawFromStrategy` | mapped, covered | Partial and over-returning strategy redeems are covered. |
| `_redeem` | `_redeem`, `_withdrawFromQueue`, `_withdrawFromQueueStrategy` | mapped, covered | Strategy maxRedeem limit, zero-redeem fallthrough, and partial/over strategy redeem returns are covered; nonzero unrealized-loss queue-break variants remain open. |
| `_add_strategy` | `_addStrategy` | mapped, covered | Queue-full append-skip branch is covered. |
| `_revoke_strategy` | `_revokeStrategy` | mapped, covered | Non-forced debt revert and re-add after normal or forced revoke are covered. |
| `_update_debt` | `_updateDebt` | mapped, covered | Strategy max-deposit zero, limited-deposit, max-redeem, max-debt-below-current, shutdown pull-only, and actual-withdrawal loss/over-return branches are covered. |
| `_process_report` | `_processReport` plus report-state helpers | mapped, covered | Third-party accountant, fee/refund clipping, and post-unlock loss/fee variants remain open. |
| `_enforce_role` | `_enforceRole` | mapped, covered | Vyper enum decoding is approximated by explicit Solidity role bounds at external role-mutator entry points. |
| `domain_separator` | `domain_separator` | mapped, covered | Chain-id drift remains open. |

## Open Checklist

These items should be closed before flipping `yearn_vault_v3` to
`production_equivalence: true`:

- Add adversarial but ABI-valid accountant mocks for fee/refund clipping,
  zero-return fees, excessive fees, and state changes outside the current
  deterministic accountant.
- Add more adversarial strategy report value combinations beyond the covered
  current-debt-above-max-debt update path.
- Cover ERC20 false-return asset behavior in addition to no-return behavior.
- Decide whether Vyper `String` and `DynArray` decoder timing/revert data are
  acceptable language-level approximations or need explicit non-equivalence
  callouts.
- Decide whether EIP-712 chain-id/domain-separator drift belongs in the
  benchmark scenario set.
