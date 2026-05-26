I checked the packaged benchmark data and source, not a fresh full rerun. My overall take: **the benchmark is directionally useful and mostly fair for “idiomatic high-level source under compiler optimizer profiles,” but the current report still needs framing clarity before I’d call the headline comparison publish-ready.**

## Verdict

The **fixed + scale headline comparison is the right idea** because it avoids real-contract porting ambiguity and excludes manual/assembly parity tricks. I also like that metadata is stripped, compiler profiles are pinned, compile failures are retained, and real-derived contracts are separated from the headline lane.

But I would change the public framing from something like “definitive compiler benchmark” to **“controlled idiomatic-source benchmark.”** It is not yet a clean compiler-optimization championship because:

1. **Scenario weighting changes conclusions.** Scenario-weighted, fixed-only, scale-family-equal, and artifact-deduped numbers can differ meaningfully.
2. **Source/profile coverage is uneven.** Some historical and Venom profiles compile fewer artifacts, so public comparisons should keep comparable-row counts and pass rates visible.

## Headline result sanity check

The headline numbers are plausible.

For **fixed + scale**, comparing `vyper-latest-gas` against `solc-latest-legacy-runs200`:

| Metric                         |                Result |
| ------------------------------ | --------------------: |
| Runtime gas geomean            | **Vyper 10.4% lower** |
| Runtime gas median             |  **Vyper 7.4% lower** |
| Win / tie / loss, ±0.5% band   |      **102 / 16 / 8** |
| Fixed-only runtime gas geomean |  **Vyper 9.4% lower** |
| Scale runtime gas geomean      | **Vyper 10.9% lower** |

So the broad claim that Vyper latest gas profile is often cheaper than solc legacy on this suite is supported.

For **`vyper-latest-gas-venom` vs `solc-latest-viair-runs200`**:

| Metric              |                                 Result |
| ------------------- | -------------------------------------: |
| Runtime gas geomean |            **Vyper Venom 12.4% lower** |
| Median              |             **Vyper Venom 4.7% lower** |
| Win / tie / loss    |                        **97 / 20 / 3** |
| Comparable rows     | **120**, not full fixed+scale coverage |

The missing rows matter: Vyper Venom fails some storage-slot scale artifacts, so the comparison is on an intersection rather than the full suite. That is fine if disclosed, but the card should show comparable-row count and pass-rate.

For **solc viaIR vs solc legacy**:

| Metric                          |                      Result |
| ------------------------------- | --------------------------: |
| Runtime gas geomean             |        **viaIR 3.7% lower** |
| Runtime gas median              |        **viaIR 1.1% lower** |
| Compile time, scenario-weighted | **viaIR about 128% slower** |
| Compile time, artifact-deduped  | **viaIR about 100% slower** |

For **solc no optimizer vs solc viaIR**, the no-optimizer baseline is unsurprisingly much worse:

| Metric                   |           Result |
| ------------------------ | ---------------: |
| Runtime gas geomean      | **35.1% higher** |
| Runtime bytecode geomean |  **109% larger** |
| Win / tie / loss         |  **0 / 0 / 126** |

That comparison is useful as a sanity check, not as a competitive compiler comparison.

## Biggest outliers

### Vyper gas vs solc legacy, fixed + scale

Largest Vyper gas wins:

| Scenario                                   |      Delta |
| ------------------------------------------ | ---------: |
| `scale_abi_args_8 · sum_args`              | **−38.1%** |
| `scale_abi_args_4 · sum_args`              | **−32.1%** |
| `scale_external_calls_64`                  | **−30.5%** |
| `scale_external_calls_32`                  | **−30.2%** |
| `scale_external_calls_16`                  | **−29.6%** |
| `scale_external_calls_8`                   | **−28.6%** |
| `create2_address_hashing · init_code_hash` | **−26.9%** |

Largest Vyper losses are much smaller:

| Scenario                                          |     Delta |
| ------------------------------------------------- | --------: |
| `scale_mapping_depth_64 · read_after_write`       | **+4.0%** |
| `scale_mapping_depth_32 · read_after_write`       | **+1.7%** |
| `erc20_permit_hashing · hash_current_after_nonce` | **+1.7%** |
| `erc20_permit_hashing · hash_static`              | **+1.2%** |

These look like valid optimizer/codegen differences, though `scale_abi_args_*` and `scale_external_calls_*` should be labeled as stress tests rather than representative application mix.

### Vyper Venom vs solc viaIR, fixed + scale

Largest Venom wins:

| Scenario                                    |      Delta |
| ------------------------------------------- | ---------: |
| `scale_dispatch_64 · first_selector`        | **−69.6%** |
| `scale_dispatch_64 · last_selector`         | **−56.5%** |
| `scale_dispatch_32 · first_selector`        | **−53.7%** |
| `scale_dispatch_32 · last_selector`         | **−52.4%** |
| fixed `scaling_dispatch_N · first_selector` | **−48.3%** |
| `scale_abi_args_64`                         | **−42.2%** |

This exposes a likely **local minimum/regression in solc viaIR dispatch codegen**, not a generic production result. I would keep these scenarios, but put them in a “dispatch stress / local minima” section.

Largest Venom losses are modest:

| Scenario                                   |     Delta |
| ------------------------------------------ | --------: |
| `merkle_verifier · verify_empty_false`     | **+3.0%** |
| `vault_deposit_withdraw · withdraw_revert` | **+2.2%** |

### solc viaIR vs solc legacy

This comparison has the most interesting shape. viaIR wins many loop/external-call cases but loses badly on dispatch.

viaIR wins:

| Scenario                  |      Delta |
| ------------------------- | ---------: |
| `scale_loop_bound_64`     | **−31.9%** |
| `scale_loop_bound_32`     | **−29.9%** |
| `scale_external_calls_64` | **−28.4%** |
| `scale_external_calls_32` | **−28.2%** |
| `scale_external_calls_16` | **−27.6%** |
| `scale_loop_bound_16`     | **−26.8%** |

viaIR losses / local minima:

| Scenario                                    |       Delta |
| ------------------------------------------- | ----------: |
| `scale_dispatch_64 · first_selector`        | **+152.7%** |
| `scale_dispatch_64 · last_selector`         | **+100.3%** |
| `scale_dispatch_32 · first_selector`        |  **+81.8%** |
| `scale_dispatch_32 · last_selector`         |  **+64.7%** |
| fixed `scaling_dispatch_N · first_selector` |  **+52.9%** |
| `scale_dispatch_16 · first_selector`        |  **+31.8%** |

This should be called out explicitly. It is valuable data, but it should not silently drive a general “viaIR is slower/faster” conclusion.

## Compile failures

There are **91 compile failures**. The biggest clusters:

| Area                           | Failures | Interpretation                                            |
| ------------------------------ | -------: | --------------------------------------------------------- |
| `scale_abi_args_16/32/64`      | 61 total | Mostly Solidity stack-too-deep / compiler capacity stress |
| `scale_storage_slots_16/32/64` |  9 total | Vyper Venom stack-depth/internal compiler limitation      |
| `uniswap_v2_factory`           |       12 | Older Vyper/source-compat issues                          |
| `curve_stableswap_2coin`       |        2 | Accepted old-solc stack/capacity limit                    |
| `yearn_vault_v3`               |        2 | Accepted old-solc stack/capacity limit                    |
| `merkle_verifier`              |        1 | Older Vyper feature gap                                   |
| `uniswap_v2_pair`              |        1 | Older profile/source compatibility                        |

I would **not rewrite away** the stack-too-deep ABI-arity cases if your goal is fair compiler capability measurement. Keep them, but show pass rates and geomeans on the intersection.

## Real-derived contract rewrite issues and caveats

I did not find an obvious core accounting bug in the main Uniswap Pair / Yearn / Curve scenarios under the documented fixtures. The remaining old-solc real-derived failures are accepted compatibility limits: Curve hits old-solc stack depth, and Yearn only gets past syntax backports by reshaping large vault logic too much for this benchmark's latest-source policy.

## Stress tests, not representative production mix

* `scale_dispatch_N`: important local-minimum detector, especially for solc viaIR.
* `scale_abi_args_N`: compiler-capacity / ABI lowering stress.
* `scale_external_calls_N`: call-overhead stress.
* low-N `abi_args` / `loop_bound`: partly dispatch/base-overhead dominated.

## Weighting recommendation

Right now the headline is scenario-weighted. That is defensible, but not sufficient.

I would report four separate aggregates:

1. **Scenario-weighted runtime gas**
   Useful for “what did the measured scenario set show?”

2. **Benchmark-equal or family-equal runtime gas**
   Prevents `mapping_depth`, `storage_slots`, or `dispatch` from dominating because they have more branches per N.

3. **Artifact-level bytecode size**
   Deduped by compiled artifact.

4. **Artifact-level compile time**
   Deduped by compiled artifact.

This matters. For example:

| Comparison                | Scenario-weighted runtime gas | Family/benchmark-equal runtime gas |
| ------------------------- | ----------------------------: | ---------------------------------: |
| Vyper gas vs solc legacy  |                    **−10.4%** |                   **about −11.7%** |
| Vyper Venom vs solc viaIR |                    **−12.4%** |                   **about −11.5%** |
| solc viaIR vs solc legacy |                     **−3.7%** |                    **about −6.5%** |

Not disastrous, but enough that the report should not rely on one master number.

## Fairness of compiler-optimization comparison

The comparison is mostly fair **for the specific claim**:

> Given idiomatic Solidity and idiomatic Vyper implementations of similar benchmark tasks, using pinned compiler profiles, how do compiler/codegen outputs compare?

It is less fair for this stronger claim:

> Which compiler has better optimizations overall?

To support the stronger claim, add optimizer-frontier comparisons. `vyper-latest-gas` vs `solc runs=200` is not necessarily comparing two Pareto-optimal settings. I would add a solc runs sweep, for example:

* runs 1
* runs 20
* runs 200
* runs 800
* runs 10,000
* runs 1,000,000

Then compare gas/size Pareto frontiers against:

* Vyper gas
* Vyper codesize
* Vyper Venom gas
* Vyper Venom codesize, if supported

That avoids the criticism that one compiler was compared against an arbitrary optimizer setting.

## Concrete pre-publication fixes

I would prioritize these:

1. **Soften “definitive” language.**
   The benchmark is strong, but the current form is better described as a controlled idiomatic-source benchmark with separate stress and real-derived lanes.
