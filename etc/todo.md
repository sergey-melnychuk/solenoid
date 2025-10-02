```
export BLOCK=23027350
export SKIP=14
time cargo run --release --example revm -- $BLOCK $SKIP
time cargo run --release --example sole -- $BLOCK $SKIP
time cargo run --release --example check -- $BLOCK $SKIP
```
===
```
# SKIP=9 - SELFBALANCE inconsistency for 0x34976e84a6b6febb8800118dedd708ce2be2d95f

# SKIP=11 - not enough balance for transfer revert (see ./src/executor.rs:1831:17)
# SKIP=11 - CALL at pc=11456: gas_cost lacks 39 gas (2500 / 64 = 39), cold/warm?
```
===
```
# SKIP=14 (extra 465 gas reported as used @ pc=23538; total gas: lacking 4865)
📦 Fetched block number: 23027350
TX hash=0x7c73933ddf6aa7cc0016e956314a19b3332075131d031cea4e584dff48c59612 index=14
DEBUG revm: gas.spent=183140 gas.refund=0 refund.cap=0 gas.final=183140
---
RET: 0000000000000000000000000000000000000000000000000000000000000001
GAS: 183140
OK: true
TRACES: 10078 in revm.23027350.14.log
cargo run --release --example revm -- $BLOCK $SKIP  0,24s user 0,11s system 57% cpu 0,609 total
===
📦 Fetched block number: 23027350
PATCH: 0x9e4ee5137a738d218e85bb2fd0f29174f87afdfe balance 0 -> 90a4a345dbae6ead
PATCH: 0x042523db4f3effc33d2742022b2490258494f8b3 balance 0 -> 7af6c7f2729115eee
PATCH: 0x0fc7cb62247151faf5e7a948471308145f020d2e balance 0 -> 7af6c7f2728a1bef0
PATCH: 0x8a14ce0fecbefdcc612f340be3324655718ce1c1 balance 0 -> 7af6c7f2728a0e4f0
PATCH: 0x8778f133d11e81a05f5210b317fb56115b95c7bc balance 0 -> 7af6c7f27291f2ff0
PATCH: 0xbb318a1ab8e46dfd93b3b0bca3d0ebf7d00187b9 balance 0 -> 0
PATCH: 0xdf7c26aaa9903f91ad1a719af2231edc33e131ed balance 0 -> 0
PATCH: 0x34976e84a6b6febb8800118dedd708ce2be2d95f balance 0 -> 8bc93020944b6ead
PATCH: 0x881d40237659c251811cec9c364ef91dc08d300c balance 0 -> 2f40478f834000
TX hash=0x7c73933ddf6aa7cc0016e956314a19b3332075131d031cea4e584dff48c59612 index=14
---
RET: 0000000000000000000000000000000000000000000000000000000000000001
DEBUG: gas.used=145007 gas.refund=0 refund.cap=0 gas.final=178275
GAS: 178275
OK: true
TRACES: 10012 in sole.23027350.14.log
cargo run --release --example sole -- $BLOCK $SKIP  0,21s user 0,09s system 49% cpu 0,618 total
===
WARN: len mismatch: sole=10012 revm=10078

thread 'main' panicked at examples/check.rs:64:13:
assertion failed: `(left == right)`

Diff < left / right > :
 OpcodeTrace {
     pc: 23538,
     op: 21,
     name: "ISZERO",
<    gas_used: 19133,
<    gas_left: 226240,
>    gas_used: 18668,
>    gas_left: 226705,
     gas_cost: 3,
     gas_back: 0,
     stack: ...,
     memory: ...,
     depth: 1,
     extra: ...
 }

LINE: 2960
```
