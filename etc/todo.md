`cargo run --release --bin analyser -- trace.log block.log '{}'` :

```
NOTE: trace path: trace.log
NOTE: block path: block.log
WARN: len mismatch: block=8898 trace=8698

thread 'main' panicked at src/bin/analyser.rs:56:13:
assertion failed: `(left == right)`

Diff < left / right > :
 OpcodeTrace {
     pc: 1785,
     op: 85,
     name: "SSTORE",
<    gas_used: 2575,
<    gas_cost: 0,
>    gas_used: 5475,
>    gas_cost: 2900,
     gas_back: 4800,
     stack: [
         36441503,
         599,
         10278278526063167437064631507323,
         0,
         8158604411017647857969462287098456729060654638,
         164,
         0,
     ],
     memory: [
         0,
         0,
         128,
     ],
     depth: 3,
     extra: Extra {
         value: Object {
<            "gas_left": String("0x2aaf9"),
<            "gas_cost": Number(0),
<            "evm.gas.used": Number(2575),
<            "evm.gas.refund": Number(4800),
>            "gas_left": Number(169366),
         },
     },
 }


note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
LINE: 4875
```
