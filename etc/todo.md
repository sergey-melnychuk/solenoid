export BLOCK=23027351
export SKIP=42
cargo run --release --example revm -- $BLOCK $SKIP
cargo run --release --example sole -- $BLOCK $SKIP
cargo run --release --example check -- $BLOCK $SKIP
