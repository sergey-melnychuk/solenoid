DONE! 10 selected blocks etc/sync.sh are 100% gas match!

---

23678686 0      // -38 gas_left on delegatecall
23683764 137    // depth 1->3 after CREATE (not OOG)

23678721 137    // JUMP(I) dest beyond bytecode

23683035 0      // ADDMOD mismatch
23683035 1      // same
23683035 2      // same

---

etc/ab.sh 23678007 0
// revm(<pid>,0x1f4cb0800) malloc: Failed to allocate segment from range group - out of space
// TODO: create PR in revm with allocation sanity checks

---

cargo run --release --example runner -- .
cargo run --release --example check -- .

---

cargo run --release --example runner -- latest
