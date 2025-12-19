DONE! 10 selected blocks etc/sync.sh are 100% gas match!
TODO: Check post-tx state & logs in runner.

---

State alignment focus (see todo.txt):
23891564: 605-621: failed txs, pattern is one extra touched account (or one extra key)
23891588: 400-408: failed txs, pattern is one extra key touched

---

23891571 38 LINE=24833,pc=17711,op=SSTORE
### 23891571 38 hash=0x8792f21d230131a3f5c57add8950e966041f2c746b9ba9669ebca0a90c206d1f
REVM 	OK=false 	RET=<4> 	GAS=534715	TRACES=24966
sole 	OK=false 	RET=match	GAS=match	TRACES=24994

---

Heavy transactions: 400k (57GB) traces
All to "Aztec: Ignition Chain L2 Rollup"

23891562 280 0x60a36465ae2da6d0a05c52b3105002a53d83d7e2b81e80d571863f272fa2869b
23891565 116 0x7a737b66c684f5754247ea0b2dd19720c653433fa2860e2f2fe0dfaee2654f1f 
23891581 201 0x4f82a8896a76491dc8f1b5506547976569ee02608ae84d55f1642daf1906a975
23891583 220 0xd41c774a51b575db212f9dcda23e6e550ca84587522620f02e40fef6b60aae75

Returning large number of traces in memory seems like unnecessary memory pressure:
it is unlikely that user will ever need ALL opcode-based logs (until the they do),
so providing a channel for the user to pick traces from might be a more scalable
and a reasonable thing to do, which at the same time leaves user in full control.

Will need to check how exactly will it work as WebAssembly binary in a browser.
Will also need a way to run it in A/B mode against revm, maybe zip trace streams.

Per-tx A/B check:

etc/ab.sh 23891562 280: OK
etc/ab.sh 23891565 116: OK
etc/ab.sh 23891581 201: OK
etc/ab.sh 23891583 220: OK

---

Txn Type: 4 (EIP-7702) PROCESSED ALL WRONG by both REVM & SOLE!

https://netbasal.medium.com/eip-7702-delegated-execution-and-sponsored-transactions-ad7f5ef80257

TX with authorizationList is sent to address 0x0

### 23891565 57 hash=0xd111228966a056e7bfe35654fc62c2defec2b3402a1707b7d00ade36cc029dcc
REVM 	OK=true 	RET=empty	GAS=21000	TRACES=0
sole 	OK=false 	RET=match	GAS=match	TRACES=0
Ethrerscan:	
 EIP-7702: 0x5567a5f4...bC0909A2c Delegate to 0xB144e6f0...2A3de95Df
 46,382 | 36,800 (79.34%) | Base: 0.055669204 Gwei
 Delegated Address: 0xAC629747e42c9789D47B82C9d03cAF0a69932e3b

### 23891505 227 hash=0x0c4eda0c27fac8f9235b383f09178672e2f51a792529fd8714810704574df855
REVM 	OK=true 	RET=empty	GAS=21010	TRACES=0
sole 	OK=true 	RET=match	GAS=match	TRACES=1
Etherscan:
 EIP-7702: 0x4DE23f3f...61714b2f3 Delegate to Null: 0x000...000
 300,003 | 36,804 (12.27%) | Base: 0.05073424 Gwei
 Delegated Address: 0xAC629747e42c9789D47B82C9d03cAF0a69932e3b

---

Blocks that fail with "stack overflow":
## Set min stack size to 16 Mb
RUST_MIN_STACK=16777216
