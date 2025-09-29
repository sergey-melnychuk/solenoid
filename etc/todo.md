export BLOCK=23027350
export SKIP=7
cargo run --release --example revm -- $BLOCK $SKIP
cargo run --release --example sole -- $BLOCK $SKIP
cargo run --release --example check -- $BLOCK $SKIP
