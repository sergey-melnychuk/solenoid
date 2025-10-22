etc/run.sh 23027350 11
etc/run.sh 23624962 33

$ cargo run --release --example runner -- 23624962
...
📦 Fetched block number: 23624962 [with 129 txs]
📦 Fetched block number: 23624962 [with 129 txs]
---
### block=23624962 index=33 hash=0xf13031bdf77a313bea32a6b3cfed412c8ecd8af37eaf2d9e1c7a3aa87a3b5f88
REVM 	OK=true 	RET=empty 	GAS=212254 	TRACES=1696 	ms=5
sole 	OK=true 	RET=true 	GAS=-99997 	TRACES=1696 	ms=6
---
### block=23624962 index=48 hash=0x0f7dd42a218e80fae64d4701ae374f006345c891a01a411d4f75297c9f9934bc
REVM 	OK=true 	RET=empty 	GAS=145956 	TRACES=9516 	ms=43
sole 	OK=false 	RET=false 	GAS=+24805 	TRACES=9479 	ms=41
---
### block=23624962 index=51 hash=0x84680d05925132314d3b8d3d46d7d5a9be16e15c3f5e0bb5be59f930fc526277
REVM 	OK=true 	RET=<32> 	GAS=134528 	TRACES=7687 	ms=38
sole 	OK=true 	RET=true 	GAS=-2500 	TRACES=7687 	ms=37
---
### block=23624962 index=70 hash=0x0867ed14c15beaf15501198ee9deb9286289e376f4e040456ce9a44b4f03101b
REVM 	OK=true 	RET=<96> 	GAS=154436 	TRACES=5778 	ms=20
sole 	OK=true 	RET=true 	GAS=-2500 	TRACES=5778 	ms=20
---
### block=23624962 index=90 hash=0x34525dbfcd324961e0fe867fb4d677964020400e55a2235b09aa8f25d01abb3b
REVM 	OK=true 	RET=<32> 	GAS=46625 	TRACES=740 	ms=1
sole 	OK=true 	RET=true 	GAS=-2800 	TRACES=740 	ms=2
---
### block=23624962 index=124 hash=0x65ab14a56629bfdf65d532c90e2879dc32268625176bcc462c18b07f556259d3
REVM 	OK=true 	RET=empty 	GAS=25054 	TRACES=62 	ms=0
sole 	OK=true 	RET=true 	GAS=-2500 	TRACES=62 	ms=0

(total: 129, matched: 123, invalid: 6)

===

cargo run --release --example runner -- 23027350
    Finished `release` profile [optimized] target(s) in 0.11s
     Running `target/release/examples/runner 23027350`
📦 Fetched block number: 23027350 [with 209 txs]
📦 Fetched block number: 23027350 [with 209 txs]
---
### block=23027350 index=11 hash=0xf4ba30862cbab5fbb0daadd28b8818c5f11489bccdc5b85b0de5ebb84a704dd1
REVM 	OK=true 	RET=empty 	GAS=202853 	TRACES=10523 	ms=52
sole 	OK=true 	RET=true 	GAS=-7500 	TRACES=10523 	ms=51
---
### block=23027350 index=14 hash=0x7c73933ddf6aa7cc0016e956314a19b3332075131d031cea4e584dff48c59612
REVM 	OK=true 	RET=<32> 	GAS=183140 	TRACES=10078 	ms=84
sole 	OK=true 	RET=true 	GAS=-7861 	TRACES=10078 	ms=78
---
### block=23027350 index=18 hash=0x1ee2cd45b6365a8c1d5eb9d49c1700f2489a0b0eb24a0fe2e4adb0949af27573
REVM 	OK=false 	RET=<100> 	GAS=34910 	TRACES=425 	ms=1
sole 	OK=false 	RET=true 	GAS=-2182 	TRACES=425 	ms=1
---
### block=23027350 index=21 hash=0x3dc02add78fb958d4460a2b853971468e696744a49013b2ff3aea2caa12245b9
REVM 	OK=true 	RET=empty 	GAS=105729 	TRACES=2514 	ms=12
sole 	OK=false 	RET=true 	GAS=+87565 	TRACES=2474 	ms=11
---
### block=23027350 index=22 hash=0x41c17144882af6ae28478ea930a9f171514bb04aa14c5df49d2c660a09c553de
REVM 	OK=true 	RET=empty 	GAS=448318 	TRACES=3824 	ms=12
sole 	OK=true 	RET=true 	GAS=-104997 	TRACES=3824 	ms=15
---
### block=23027350 index=24 hash=0x7b751b7f39d0ec4ad28f7d75d8313520f1fbc26bc38df48fd658f72cb1d56604
REVM 	OK=true 	RET=<32> 	GAS=231724 	TRACES=10706 	ms=112
sole 	OK=true 	RET=true 	GAS=-2500 	TRACES=10706 	ms=101
---
### block=23027350 index=26 hash=0x9c3312fc1035e77ce2ee0193faaeea0ea2dc8b2e9fc7bef11962ced7213f22ff
REVM 	OK=true 	RET=empty 	GAS=408554 	TRACES=21450 	ms=56
sole 	OK=true 	RET=true 	GAS=-2497 	TRACES=21450 	ms=65
---
### block=23027350 index=28 hash=0x1514d34268a6525225c82bebe18b10a1da84f20c699a423232a336b226d2ddef
REVM 	OK=true 	RET=empty 	GAS=215265 	TRACES=13181 	ms=56
sole 	OK=false 	RET=true 	GAS=-145284 	TRACES=1822 	ms=6
---
### block=23027350 index=30 hash=0xaf17e609a877c61afe2873f2dda78c50535f7d634b6bf030897ac273d0f349a1
REVM 	OK=true 	RET=<32> 	GAS=174704 	TRACES=5915 	ms=21
sole 	OK=false 	RET=false 	GAS=-133110 	TRACES=1133 	ms=4
---
### block=23027350 index=55 hash=0xb10b001ffb62d7273accd1c94829f8cc16e8f98597e5c5df4d39af5ff0422bfc
REVM 	OK=true 	RET=empty 	GAS=351107 	TRACES=48620 	ms=740
sole 	OK=true 	RET=true 	GAS= +372 	TRACES=48620 	ms=649
---
### block=23027350 index=60 hash=0xe30bacb372ab39e3cfc57c2b939ed1962833852e884d60fcbca6f82d2c2a6507
REVM 	OK=true 	RET=empty 	GAS=269137 	TRACES=14469 	ms=157
sole 	OK=false 	RET=true 	GAS=-188314 	TRACES=3259 	ms=37
---
### block=23027350 index=64 hash=0x450251aef90494156f3456696e5f1a58d67c0ba18ffe4fe8f88d528267bf92e9
REVM 	OK=true 	RET=empty 	GAS=33157 	TRACES=146 	ms=0
sole 	OK=false 	RET=true 	GAS= -356 	TRACES=133 	ms=0
---
### block=23027350 index=70 hash=0x747f8bfe7eeceeaebe070b32138198c57c91086213643c29c5eda54a6e16ae97
REVM 	OK=true 	RET=<64> 	GAS=159752 	TRACES=5717 	ms=17
sole 	OK=true 	RET=true 	GAS=-4000 	TRACES=5717 	ms=18
---
### block=23027350 index=71 hash=0xafd98234ce73d9f058b135323ba29e36d0d23d432d4d83c2d028478d90b4fbbc
REVM 	OK=true 	RET=empty 	GAS=29240 	TRACES=0 	ms=0
sole 	OK=true 	RET=true 	GAS=-4944 	TRACES=0 	ms=0
---
### block=23027350 index=79 hash=0x42714d5dd4dd51cbbb777a3190afd71efd936447ca932f903b0c6aed2b6bebfa
REVM 	OK=true 	RET=empty 	GAS=82538 	TRACES=3236 	ms=18
sole 	OK=true 	RET=true 	GAS=-2500 	TRACES=3236 	ms=17
---
### block=23027350 index=83 hash=0x6677280b19ee666d9c29d5e16a62000626b26f5aad3d8ecffb6571745dcadd57
REVM 	OK=true 	RET=<96> 	GAS=502640 	TRACES=34359 	ms=2364
sole 	OK=true 	RET=true 	GAS=-135653 	TRACES=34359 	ms=2421
---
### block=23027350 index=97 hash=0x82359c55437c8923f2e7df58aa2ae5ab0705c8e91660ab241ae32e70b0c6555d
REVM 	OK=true 	RET=empty 	GAS=22280 	TRACES=0 	ms=0
sole 	OK=true 	RET=true 	GAS= -768 	TRACES=0 	ms=0
---
### block=23027350 index=98 hash=0xa5a80f7648e069e60f5547d1a6077d9fda7ce10a1ab29680edbdec55ae424385
REVM 	OK=true 	RET=empty 	GAS=22280 	TRACES=0 	ms=0
sole 	OK=true 	RET=true 	GAS= -768 	TRACES=0 	ms=0
---
### block=23027350 index=99 hash=0x816c41af36663d61c47bbe5afa19e2421efe01c8873e1e14725e90a891223fc7
REVM 	OK=true 	RET=empty 	GAS=22280 	TRACES=0 	ms=0
sole 	OK=true 	RET=true 	GAS= -768 	TRACES=0 	ms=0
---
### block=23027350 index=100 hash=0xa0cd5ca48739d18d07fdda9bd379cf148d7e3321e36bf0cd121bdaa583c51a9b
REVM 	OK=true 	RET=empty 	GAS=22250 	TRACES=0 	ms=0
sole 	OK=true 	RET=true 	GAS= -750 	TRACES=0 	ms=0
---
### block=23027350 index=101 hash=0xaf9bc110613cd93023b42756665d659c363a16823587642bc97555780df0a894
REVM 	OK=true 	RET=empty 	GAS=22280 	TRACES=0 	ms=0
sole 	OK=true 	RET=true 	GAS= -768 	TRACES=0 	ms=0
---
### block=23027350 index=111 hash=0x5fbdcf34537d36ac52cbdc271a59e35f75a66255e0956994d13ea85ceb2f25e1
REVM 	OK=true 	RET=<64> 	GAS=175164 	TRACES=6954 	ms=21
sole 	OK=true 	RET=true 	GAS=-2500 	TRACES=6954 	ms=21
---
### block=23027350 index=147 hash=0x47b94bf20a643e4a724ac604db2ca5b4667fd30ea1fffbdf5d3dff03f9291711
REVM 	OK=true 	RET=empty 	GAS=393460 	TRACES=20218 	ms=132
sole 	OK=false 	RET=true 	GAS=-276022 	TRACES=3539 	ms=14
---
### block=23027350 index=149 hash=0x92e2d89fd012ceaba693e51891feb5e1b930df8a55d65b0c98a2a55962534d10
REVM 	OK=true 	RET=empty 	GAS=453850 	TRACES=21538 	ms=130
sole 	OK=false 	RET=true 	GAS=-330769 	TRACES=2952 	ms=12
---
### block=23027350 index=175 hash=0x752ed5bcb4807e95a4aee90bc5a7bfbf61e7816257f9798c6731cd4971d8f0bd
REVM 	OK=true 	RET=<32> 	GAS=139986 	TRACES=2233 	ms=10
sole 	OK=true 	RET=true 	GAS=-5000 	TRACES=2233 	ms=10
---
### block=23027350 index=186 hash=0xa81607f1c623ce53b1ac298bf973e6a79e1c8974e17438a84ca37d923264c69c
REVM 	OK=true 	RET=empty 	GAS=172870 	TRACES=9686 	ms=48
sole 	OK=true 	RET=true 	GAS=-2500 	TRACES=9686 	ms=45
---
### block=23027350 index=197 hash=0x9b312d7abad8a54cca5735b21304097b700142cea90aeba3740f6a470e734fa6
REVM 	OK=true 	RET=empty 	GAS=39464 	TRACES=166 	ms=0
sole 	OK=true 	RET=true 	GAS=-2497 	TRACES=166 	ms=0

(total: 209, matched: 182, invalid: 27)

===

curl -X POST \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "eth_getBlockByNumber",
    "params": ["0x15f5e96", true],
    "id": 1
  }' \
  https://eth.llamarpc.com | jq .result > 23027350.json

curl -X POST \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "eth_getBlockByNumber",
    "params": ["0x15f5e96", true],
    "id": 1
  }' \
  http://127.0.0.1:8080

curl -s -X POST -H "Content-Type: application/json" --data '{
    "jsonrpc": "2.0",
    "method": "eth_getStorageAt",
    "params": ["0x8ad599c3a0ff1de082011efddc58f1908eb6e6d8","0x0","0x165c93d"],
    "id":1
}' http://127.0.0.1:8080

export HASH=0x073c6e8b5b748dff4d58bdb59fa2705f7ce9e32682678ca0aa541ace3b7eee52 && curl -s -X POST -H "Content-Type: application/json" --data '{
    "jsonrpc": "2.0",
    "method": "eth_getTransactionByHash",
    "params": ["$HASH"],
    "id": 1
}' http://127.0.0.1:8080 | jq > $HASH.json
