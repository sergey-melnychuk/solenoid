DONE! 10 selected blocks etc/sync.sh are 100% gas match!
TODO: Check post-tx state & logs in runner.

Blocks that fail with "stack overflow":
## Set min stack size to 16 Mb
RUST_MIN_STACK=16777216

---

COINBASE BALANCE:

etc/ck.sh 23728671 36
   value: 0x166605a78ad81c (sent to miner in a tx)

NO FEES:
expected: 0xb033a129cea32f7e0
 but got: 0xafff3303ff6a69b5a // initial balance at block start

WITH FEES:
expected: 0xb033a129cea32f7e0
 but got: 0xb03c479b79f6cbb06 // cumulative fees (too big)
    diff:    0x8a671ab539c326

---

23828643 11 // GAS=-7375

23678721 137 // GAS=-192478

---
