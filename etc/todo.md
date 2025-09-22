```
$ RUST_LOG=off cargo run --release --example revm > revm.log 
...
📦 Fetched block number: 23027350
---
RET: 0000000000000000000000000000000000000000000000000000000000000001
GAS: 177185
OK: true
```

```
$ RUST_LOG=off cargo run --release --example sole > sole.log 
...
📦 Fetched block number: 23027350
---
RET: 0000000000000000000000000000000000000000000000000000000000000001
GAS: 175729         [<-- correct value is 177185 (according to revm)]
OK: true
```

```
echo "Running REVM vs SOLE:"
RUST_LOG=off cargo run --release --example revm > revm.log
RUST_LOG=off cargo run --release --example sole > sole.log

cargo run --release --example check -- revm.log sole.log '{}'
```
