# Compiler compatibility adapters

The benchmark compiles generated variants of the checked-in sources. Adapters
are selected by the resolved compiler version; their output paths and hashes are
recorded in the run manifest. They preserve the benchmark's observable behavior
while translating syntax and APIs supported by the target compiler.

The Vyper prerelease profile follows PyPI using PEP 440 version ordering. The
September 2026 migration resolves 0.5.0b1 in place of the pinned 0.5.0a1 profiles.

| Source / compiler | Adapter | Behavioral constraint |
| --- | --- | --- |
| Uniswap pair / Vyper 0.5 | Import `math` and call `math.isqrt` | Preserve floor square root over the full uint256 domain. |
| Yearn v2 / Vyper 0.5 | Give each token-symbol call an explicit `String[1]` local | Preserve the old interface's one-byte limit, both conditional calls, and both override branches. |
| Yearn v2 / Vyper 0.3 | Translate `StrategyParams(field=value)` to dictionary-style construction | Preserve fields, their order and expressions, nested calls, and comments. |
| Curve / Vyper 0.3.7 | Use `shift`, a static loop bound with an early break, and an explicit ERC-4626 interface | Preserve signed/unsigned shifts and coefficients; iterate to the immutable coin count, bounded by the constructor's `DynArray`; preserve external selectors. |
| Uniswap pair / solc 0.4.26 | Hash literals directly and adapt low-level token calls | Hash identical bytes; accept empty returns or a canonical 32-byte true value, including trailing data; reject short returns, false, and noncanonical booleans. |
| Yearn v3 / solc 0.4.26 | Disambiguate function-scoped locals, use array length decrement, opt into ABIEncoderV2, and adapt low-level token calls | Preserve branch-local evaluation order, removal of the final array element, and optional-return checks. |

The one-byte Yearn limit is an existing benchmark-source behavior, not a newly
chosen token-symbol policy. Vyper 0.4.3's bundled `IERC20Detailed` returns
`String[1]`, and using that call directly in `concat` retains the runtime limit.
Using the destination capacities (57 or 30 bytes) for the new locals would
silently broaden accepted inputs. The adapter deliberately retains the observed
0/1-byte success and 2-byte rejection, without truncation.

Run the full matrix and then the focused compatibility checks:

```sh
cargo run --release -- run
cargo run --release -- validate
uv run scripts/verify_vyper_compat.py
```

The focused script extracts the actual materialized vault initialization
branches and Curve exponential function, verifies recorded compiler binary
hashes, and compiles standalone probes. It compares all six beta modes against
stable Vyper, plus both Vyper 0.3.7 modes for Curve arithmetic. Tests cover
square-root invariants and edge values, symbol bounds and override branches,
and exponential range boundaries and fuzz inputs. Inputs, bytecode hashes, and
Foundry output are retained in `target/vyper-compat/`. Full-contract scenario
checks remain part of the main matrix; these probes supplement them.
The script also explicitly compares recovered historical rows against
`vyper-latest-gas`: call outcomes and return, observer, and log hashes. Historical
variants do not automatically receive those differential checks in the main
report, so this comparison is recorded separately in the compatibility audit.

Run `20260909T233233Z` contains 7,843 compiled artifacts and 29,775 scenario
measurements. The adapters recover 19 artifacts, reducing compilation failures
from 299 to 280. All six beta profiles compile all 64 benchmarks, and their
1,482 scenario rows pass the applicable report checks. The recovered historical
profiles add 135 scenarios; all match stable Vyper's recorded outcomes and
hashes. The same 20 pre-existing runtime failures remain. The standalone probes
pass five tests, including 256 square-root and 256 exponential fuzz cases.

## Failures retained

Successful syntax adaptation does not imply successful compilation or execution.
After the solc 0.4.26 syntax fixes, the pair reaches a stack-depth failure and
Yearn v3 reaches the compiler's unimplemented calldata-array encoding path.
Those results remain compile errors. Stack-depth stress cases, Fe operand
limits, missing historical `raw_create`, and broader Vyper 0.2 feature gaps are
also retained rather than simplified out of the corpus.

The solc 0.4.26 factory runtime failure is an opcode-version incompatibility.
That compiler emits the draft CREATE2 opcode `0xfb`, as defined in its
[tagged instruction table](https://github.com/ethereum/solidity/blob/v0.4.26/libevmasm/Instruction.h#L196).
The measured factory runtime contains that opcode and the Prague harness reports
`OpcodeNotFound`. Changing the compiler's EVM target to `constantinople` does not
update its opcode definition. Bytecode is not patched; the raw failure remains.

The time-advanced Curve oracle failures under Vyper 0.4.0 Venom remain runtime
failures. Matching legacy and newer compiler profiles pass those scenarios, but
the exact compiler defect has not been isolated. Fixtures and expected outcomes
remain unchanged.
