solenoid
========

Learning EVM by doing.

```
export CODE=0x
export DATA=0x

NO_COLOR=1 cargo run --release -- $CODE $DATA > dump.log
```

```
NO_COLOR=1 cargo run --release -- 0x$(cat etc/counter/Counter.bin) "0x"
```

```
cargo check --target wasm32-unknown-unknown
```
