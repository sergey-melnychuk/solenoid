DONE! 10 selected blocks etc/sync.sh are 100% gas match!

---

23676566 101    // balance mismatch
23678156 53     // probably out-of-gas
23678686 0      // -38 gas_left on delegatecall
23678721 137    // "the len is 11219 but the index is 15616"
23678747 290    // probably out-of-gas
23683035 0      // ADDMOD mismatch
23683035 1      // same
23676997 2      // SELFDESTRUCT
23676997 3      // funds?
23683264 37     // SELFDESTRUCT

---

etc/ab.sh 23678007 0
// revm(<pid>,0x1f4cb0800) malloc: Failed to allocate segment from range group - out of space
// TODO: create PR in revm with allocation sanity checks

---

cargo run --release --example runner -- .
cargo run --release --example check -- .

---

cargo run --release --example runner -- latest
