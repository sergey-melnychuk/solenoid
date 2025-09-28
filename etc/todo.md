RUN:
```
cargo run --release --example sole block-number skip-txs
cargo run --release --example revm block-number skip-txs
```

CHECK:
```
cargo run --release --example check -- revm.*.log sole.*.log
...
```