```
echo "Running REVM vs SOLE:"
RUST_LOG=off cargo run --release --example revm > revm.log
RUST_LOG=off cargo run --release --example sole > sole.log

cargo run --release --example check -- revm.log sole.log '{}'
```

```
NOTE: revm path: revm.log
NOTE: sole path: sole.log
WARN: len mismatch: sole=8898 revm=8698

thread 'main' panicked at examples/check.rs:56:13:
assertion failed: `(left == right)`

Diff < left / right > :
 OpcodeTrace {
     pc: 3404,
     op: 85,
     name: "SSTORE",
     gas_used: 87549,
     gas_cost: 100,
<    gas_back: 2800,
>    gas_back: 0,
     stack: [
         36441503,
         599,
         10278278526063167437064631507323,
         0,
         8158604411017647857969462287098456729060654638,
         164,
         0,
         179097484329444323534965117274265,
         5674137658222917002,
         168819205803381156097900485766942,
         6020637658222917002,
     ],
     memory: [
         0,
         0,
         356,
         0,
         25,
         52670383448186445861553817759887498218675746408080920759387454194053457903616,
         68,
         76450787359836037641860180984291677749982818650173156833227181650216873998108,
         97820150042821517726933934340677095888753881422686686000670458140509581997372,
         48829203053713733752069786167845894462088185980121892663542533619322818920448,
         862718293348820473429344482784628181556388621521298319395315527974912,
         26959946667150639794667015087019630673637144422540572481103610249216,
         80675818,
         3018216498407675031010667955644090554662299366837776087604331998235151430972,
         48829203053713733752069786167845894462088185980121892663542533619322818920448,
         0,
     ],
     depth: 3,
     extra: Extra {
         value: Object {
             "gas_left": Number(87292),
<            "sstore": Object {
<                "is_warm": Bool(true),
<                "original": String("0000000000000000000000000000000000000000000000000000000000000001"),
<                "key": String("000000000000000000000000000000000000000000000000000000000000000c"),
<                "val": String("0000000000000000000000000000000000000000000000000000000000000000"),
<                "new": String("0000000000000000000000000000000000000000000000000000000000000001"),
<                "gas_cost": Number(100),
<                "gas_back": Number(2800),
<            },
<            "gas_cost": Number(100),
<            "evm.gas.back": Number(7600),
<            "evm.gas.used": Number(87449),
<            "SRC": String("not CALL"),
         },
     },
 }


note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
LINE: 7241
```
