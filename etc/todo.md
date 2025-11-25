DONE! 10 selected blocks etc/sync.sh are 100% gas match!
TODO: Check post-tx state & logs in runner.

Blocks that fail with "stack overflow":
## Set min stack size to 16 Mb
RUST_MIN_STACK=16777216

---

COINBASE BALANCE:

23820674 30 hash=0xaf7e4a7912075617a168c3b44a49c7a951e9ad8e69ee2d52d45d055be32705e4
23820674 58 hash=0xc0cd09dc4fd684a9557f6d866b23a001e9f303c2ab00fe6aa993cdc01def9b7c
23820674 59 hash=0x902c1edb3786cc89e08214aba55efaf4951cdca8e449c408d5eb9e55088c7d70
23820674 77 hash=0xc3601cfb15cce98f4a716b062dd260f54a2f817b79eea197a793daa1874f8949

23820674 30 pc=14657,op=BALANCE,revm.stack[0]=0xdad39f04d866e061,sole.stack[0]=0xdad4da0b4aedc849
23820674 58 pc=14713,op=BALANCE,revm.stack[0]=0xdae5ca2364862a,sole.stack[0]=0xdae7c752069e12
23820674 59 pc=8473,op=BALANCE,revm.stack[0]=0xdae6177cfd9fce,sole.stack[0]=0xdae814ac03cb55e8
23820674 77 pc=16216,op=BALANCE,revm.stack[0]=0xdae9ce52e69062b9,sole.stack[0]=0xdaeb0c52985b16a1

etc/run.sh 23820674
...
TX:  21/163
[REVM] COINBASE BALANCE: 0xdac0af446b73c188
[SOLE] COINBASE (GAS)  : 0xdac1ea4addfaa970 *0x13b067286e7e8
[SOLE] COINBASE BALANCE: 0xdac1ea4addfaa970
...
TX:  66/163
[REVM] COINBASE BALANCE: 0xdae76db113f6a8d5
[SOLE] COINBASE (GAS)  : 0xdae76db113f6a8d5 *0x17e66ab280
[SOLE] COINBASE BALANCE: 0xdae76db113f6a8d5
TX:  67/163
[REVM] COINBASE BALANCE: 0xdae82ce068577cd5
[SOLE] COINBASE (BLOB) : 0xdae76db10a12a8d5 +0x9e40000
[SOLE] COINBASE BALANCE: 0xdae76db113f6a8d5 -0xbf2f5460d400

etc/ab.sh 23820674 67
TX hash=0x61f0f1c5e71ba28ee18b74fefd96226033cd11a1e1751debd244bb5a221982af index=67
---
RET:
GAS: 21000
OK: true

---

ACCESS-LIST transactions:

23828643 11
GAS=-7375
0xdc3e84df00ff8ff2dbb5dfb8a5c6bb4e04ef9fb2e74b22885ae2380e0a0631d8

23678721 137
GAS=-192478
0xc90b93f50ccbc3f1238b8a7d4ea8fe40c09cbb43d958e88974a11a84eea7b41f

---
