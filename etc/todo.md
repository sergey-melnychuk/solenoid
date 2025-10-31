DONE! 10 selected blocks etc/sync.sh are 100% gas match!

---

23678686 0      // -38 gas_left on delegatecall

23683764 137    // depth 1->3 after CREATE (not OOG)
// STOP after CREATE at depth 3 is missing from traces

23678721 137    // JUMP(I) dest beyond bytecode

23683035 0      // ADDMOD mismatch
23683035 1      // same
23683035 2      // same

---

etc/ab.sh 23678007 0
// revm(<pid>,0x1f4cb0800) malloc: Failed to allocate segment from range group - out of space
// TODO: create PR in revm with allocation sanity checks

---

23690323 25     // (block-only) SSTORE: -17100 gas
TRACE:
    pc: 11216,
    op: 85,
    name: "SSTORE",
BLOCK: {
  "sstore": {
    "is_warm": false,
    "address": "0xab02bf85a7a851b6a379ea3d5bd3b9b4f5dd8461",
    "original": "0x",
    "key": "0x36b6384b5eca791c62761152d0c79bb0604c104a5fb6f4eb0703f3154bc0cc9",
    "val": "0x",
    "new": "0x751a4f99d7987eaef0cde8a5b0d333fd00000000000000000000000000000000",
    "gas_cost": 22100,
    "gas_back": 0,
    "refund": []
  }
}
A/B: {
  "sstore": {
    "is_warm": false,
    "address": "0xab02bf85a7a851b6a379ea3d5bd3b9b4f5dd8461",
    "original": "0x224bfef74c0cd86e87a3ed17ba961d2900000000000000000000000000000000", <-- value was SSTORE'd by previous tx
    "key": "0x36b6384b5eca791c62761152d0c79bb0604c104a5fb6f4eb0703f3154bc0cc9",
    "val": "0x224bfef74c0cd86e87a3ed17ba961d2900000000000000000000000000000000",
    "new": "0x751a4f99d7987eaef0cde8a5b0d333fd00000000000000000000000000000000",
    "gas_cost": 5000,
    "gas_back": 0,
    "refund": [],
  }
}

---

cargo run --release --example runner -- .
cargo run --release --example check -- .

---

cargo run --release --example runner -- latest
