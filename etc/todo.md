DONE! 10 selected blocks etc/sync.sh are 100% gas match!
TODO: Check post-tx state & logs in runner.

Blocks that fail with "stack overflow":
## Set min stack size to 16 Mb
RUST_MIN_STACK=16777216

---

BLOCK killers:

23891562 280
23891565 116
23891581 201
23891583 220

---

Txn Type: 4 (EIP-7702) PROCESSED ALL WRONG by both REVM & SOLE!

Ethrerscan:	
 EIP-7702: 0x5567a5f4...bC0909A2c Delegate to 0xB144e6f0...2A3de95Df
 46,382 | 36,800 (79.34%) | Base: 0.055669204 Gwei
 Delegated Address 0xAC629747e42c9789D47B82C9d03cAF0a69932e3b


### 23891565 57 hash=0xd111228966a056e7bfe35654fc62c2defec2b3402a1707b7d00ade36cc029dcc
REVM 	OK=true 	RET=empty	GAS=21000	TRACES=0
sole 	OK=false 	RET=match	GAS=match	TRACES=0

---

SSTORE: OOG (exact event format)

### 23891512 252 hash=0x04d0784e6e24204fd34092a0584a991c8686fb7d23fe57807e438a48c2365251
REVM 	OK=false 	RET=empty	GAS=276370	TRACES=3454
sole 	OK=false 	RET=match	GAS=match	TRACES=3454

{
     pc: 9214,
     op: 85,
     name: "SSTORE",
SOLE:
<    gas_used: 214496,
<    gas_left: 0,
<    gas_cost: 1773,
REVM:
>    gas_used: 212723,
>    gas_left: 1773,
>    gas_cost: 0,
     gas_back: 0,
     stack: [],
     memory: [],
     depth: 2,
}

---

REVM doesn't add extra STOP opcode if program run out of opcodes to execute

### 23891505 227 hash=0x0c4eda0c27fac8f9235b383f09178672e2f51a792529fd8714810704574df855
REVM 	OK=true 	RET=empty	GAS=21010	TRACES=0
sole 	OK=true 	RET=match	GAS=match	TRACES=1

---
!
Reason: SSTORE from static-call (LINE: 154) at pc=4815 does not revert

### 23891493 124 hash=0xcea665449c32af65386c23befc1bca8818125aba1720ce848771e0d2140fe60d
REVM 	OK=false 	RET=empty	GAS=97871	TRACES=836
sole 	OK=true 	RET=<32>	GAS=-29943	TRACES=965

### 23891497 104 hash=0xbf5cee932c029be4932ba4cbaf8657a06ef6810979ec44361da02ae126d19647
REVM 	OK=false 	RET=empty	GAS=97871	TRACES=836
sole 	OK=true 	RET=<32>	GAS=-29943	TRACES=965

### 23891503 137 hash=0x83c2eaf70828e29b8532c4a27231672f084572667322813461afc58fb3a5981d
REVM 	OK=false 	RET=empty	GAS=97871	TRACES=836
sole 	OK=true 	RET=<32>	GAS=-29943	TRACES=965

### 23891507 255 hash=0x82b52dbd83efc272d47040ad2bb29709899c6667aecccd85933dcb588f75c6c6
REVM 	OK=false 	RET=empty	GAS=97871	TRACES=836
sole 	OK=true 	RET=<32>	GAS=-29943	TRACES=965

### 23891512 250 hash=0x6b53ae8e3bafb1c515e3f23e6d9149d1787b69eef0159a5d29f91f749c871d15
REVM 	OK=false 	RET=empty	GAS=97871	TRACES=836
sole 	OK=true 	RET=<32>	GAS=-29943	TRACES=965

### 23891562 101 hash=0xe227bf6d1058d2ef1ad6233ace48710d7f89e758889d1616b978f6ce653d4fb3
REVM 	OK=false 	RET=empty	GAS=97871	TRACES=836
sole 	OK=true 	RET=<32>	GAS=-29943	TRACES=965

---
!
Reason: wrong `this` address from CREATE;

REVM: 2 contracts created: 
0xe3f121577b394c4051de55d6cf3a9a31d49c88bb
0x67baac3dcd713f875f6cdc557cf7d1ffb86e6718
SOLE: 1 contrac created:
0xe29800246a9412828a47f74dc07e13cf360c6cf3

### 23891492 223 hash=0x9da982cf07f1f6f82fba618f7cae64f7aa68fb790fd230ba107d13ae4c79c8dc
REVM 	OK=true 	RET=<987>	GAS=1772514	TRACES=602
sole 	OK=true 	RET=<987>	GAS=match	TRACES=602

$ etc/ck.sh 23891492 223
NOTE: len match: 602
...
 OpcodeTrace {
     pc: 243,
     op: 48,
     name: "ADDRESS",
     gas_used: 1147,
     gas_left: 1532715,
     gas_cost: 2,
     gas_back: 0,
     stack: [
SOLE:
<        "0xe29800246a9412828a47f74dc07e13cf360c6cf3",
REVM:
>        "0x67baac3dcd713f875f6cdc557cf7d1ffb86e6718",
         "0000000000000000000000000000000000000000000000000000000000000220",
         "0000000000000000000000000000000000000000000000000000000000000240",
         "00000000000000000000000036948856c512a76eb9a70e1facd9ad4a7e806131",
     ],
     memory: [],
     depth: 2,
     debug: ...
 }
LINE: 436

{
  "pc": 143,
  "op": 240,
  "name": "CREATE",
  "gas_used": 1568488,
  "gas_left": 24347,
  "gas_cost": 1566366,
  "gas_back": 0,
  "stack": [],
  "memory": [],
  "depth": 1,
  "debug": {
    "revm": {
      "gas_left": 24347,
      "evm.gas.back": 0
    },
    "sole": {
      "is_call": true,
      "evm.gas.used": 2122,
      "evm.gas.refund": 0,
      "created": {
        "opcode": "Create",
        "address": "0xe29800246a9412828a47f74dc07e13cf360c6cf3",
        "creator": "0xe3f121577b394c4051de55d6cf3a9a31d49c88bb",
        "nonce": "0x"
      },
      "inner_evm.reverted": false,
      "inner_call": {
        "data": [],
        "value": "0x",
        "from": "0xe3f121577b394c4051de55d6cf3a9a31d49c88bb",
        "to": "0x0000000000000000000000000000000000000000",
        "gas": "0x1767a6"
      }
    }
  }
}
LINE: 133 [explore]

---
