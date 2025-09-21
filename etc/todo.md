```
echo "Running REVM vs SOLE:"
RUST_LOG=off cargo run --release --example revm > revm.log
RUST_LOG=off cargo run --release --example sole > sole.log

cargo run --release --example check -- revm.log sole.log '{}'
```

```
NOTE: revm path: revm.log
NOTE: sole path: sole.log
WARN: len mismatch: block=8898 trace=8698

thread 'main' panicked at examples/check.rs:56:13:
assertion failed: `(left == right)`

Diff < left / right > :
 OpcodeTrace {
     pc: 8468,
     op: 241,
     name: "CALL",
<    gas_used: 167480,
>    gas_used: 172280,
     gas_cost: 161505,
     gas_back: 0,
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
         0,
         0,
         87449498630317259039493649861936745254872503236,
         1097077688018008265106216665536940668749033598146,
         2256,
         87449498630317259039493649861936745254872503236,
         8158604411017647857969462287098456729060654638,
         10278278526063167437064631507323,
         0,
         96,
         87449498630317259039493649861936745254872503236,
         360,
     ],
     memory: [
         0,
         0,
         292,
         0,
         25,
         52670383448186445861553817759887498218675746408080920759387454194053457903616,
         68,
         76450787359836037641860180984291677749982818650173156833227181650216873998108,
         97820150042821517726933934340677095888753881422686686000670458140509581997372,
         48829203071513819780905531147139022235485713091346447759706217384245060984993,
         15579554367386512484115185589700858880130709892634341999162169957297731189288,
         50916924422539399718869858322060785231731346453131874365649259200814996520960,
         0,
     ],
     depth: 3,
     extra: Extra {
         value: Object {
<            "SRC": String("CALL"),
<            "gas_left": Number(163966),
<            "gas_cost": Number(161505),
<            "evm.gas.used": Number(10775),
<            "evm.gas.refund": Number(4800),
>            "gas_left": Number(2561),
         },
     },
 }


note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
LINE: 5164
```
