RUN:
```
cargo run --release --example sole > sole.log
cargo run --release --example revm > revm.log
```

CHECK:
```
cargo run --release --example check -- revm.log sole.log
...
NOTE: revm path: revm.log
NOTE: sole path: sole.log
WARN: len mismatch: sole=10221 revm=10302

thread 'main' panicked at examples/check.rs:64:13:
assertion failed: `(left == right)`

Diff < left / right > :
 OpcodeTrace {
     pc: 1600,
     op: 90,
     name: "GAS",
     gas_used: 1200,
     gas_cost: 2,
     gas_back: 0,
     stack: [
         2028016118,
         839,
         342990334663321248477731078999614427343916395509,
         128,
         96,
         0,
         0,
         164,
         160,
         342990334663321248477731078999614427343916395509,
<        336160,
>        293260,
     ],
     memory: [
         0,
         0,
         352,
         0,
         164,
         57775691130104417178790371596940099459186599011812097720222120992330034970633,
         77530103913664693058848504660978568257861239713291787686145448540476763090575,
         71218982517604891911263497047719380103333944461646775454990984214900692268723,
         75712930419883325291303464501835544240239339948914023593427068937791342641152,
         47282974651409267059370568986281131061167005020962504592988592406898038276096,
         2126250113798169508686003478867977212337740669172507329867198429524918272,
         0,
     ],
     depth: 1,
     extra: Extra {
         value: Object {
<            "gas_left": Number(336160),
<            "evm.gas.used": Number(1198),
<            "evm.gas.back": Number(0),
<            "gas_cost": Number(2),
<            "SRC": String("not CALL"),
>            "gas_left": Number(293260),
         },
     },
 }


note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
LINE: 332
```