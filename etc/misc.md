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

curl -s -X POST -H "Content-Type: application/json" --data '{
    "jsonrpc": "2.0",
    "method": "eth_getCode",
    "params": ["0xac612bd99ba27f51c612b0c5eaf798cfee6a0e0f","latest"],
    "id":1
}' http://127.0.0.1:8080

curl -s -X POST -H "Content-Type: application/json" --data '{
    "jsonrpc": "2.0",
    "method": "eth_getBalance",
    "params": ["0x4838b106fce9647bdf1e7877bf73ce8b0bad5f97","0x168c0b6"],
    "id":1
}' http://127.0.0.1:8080
# result: 0x10ed3718e8fb9b189 (COINBASE BALANCE)

---

export HASH=0x61f0f1c5e71ba28ee18b74fefd96226033cd11a1e1751debd244bb5a221982af

curl -s -X POST \
  -H "Content-Type: application/json" \
  -d "{
    \"jsonrpc\": \"2.0\",
    \"method\": \"eth_getTransactionByHash\",
    \"params\": [\"$HASH\"],
    \"id\": 1
  }" http://127.0.0.1:8080 | jq .result > etc/tx/$HASH.tx.json

curl -s -X POST \
  -H "Content-Type: application/json" \
  -d "{
    \"jsonrpc\": \"2.0\",
    \"method\": \"eth_getTransactionReceipt\",
    \"params\": [\"$HASH\"],
    \"id\": 1
  }" http://127.0.0.1:8080 | jq .result > etc/tx/$HASH.txr.json

curl -s -X POST \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "eth_getTransactionCount",
    "params": ["0x93F730AB81f4B72f778d25dA321dda2e4570597f","0x168a664"],
    "id": 1
  }' http://127.0.0.1:8080 | jq

---

curl "https://www.4byte.directory/api/v1/signatures/?hex_signature=0x3850c7bd" | jq '.results[0].text_signature'
"slot0()"

curl "https://www.4byte.directory/api/v1/signatures/?hex_signature=0x3850c7bd" | jq
{
  "count": 1,
  "next": null,
  "previous": null,
  "results": [
    {
      "id": 178654,
      "created_at": "2021-04-22T20:07:09.415160Z",
      "text_signature": "slot0()",
      "hex_signature": "0x3850c7bd",
      "bytes_signature": "8PÇ½"
    }
  ]
}

curl "https://www.4byte.directory/api/v1/signatures/?page=11496" | jq
{
  "count": 1149612,
  "next": "http://www.4byte.directory/api/v1/signatures/?page=11497",
  "previous": "http://www.4byte.directory/api/v1/signatures/?page=11495",
  "results": [
    {
      "id": 408,
      "created_at": "2016-07-09T04:00:44.274050Z",
      "text_signature": "scheduleCall(bytes,bytes)",
      "hex_signature": "0xf4bbfd6a",
      "bytes_signature": "ô»ýj"
    },
    ...
  ]
}

curl "https://www.4byte.directory/api/v1/signatures/?page=11497" | jq
{
  "count": 1149612,
  "next": null,
  "previous": "http://www.4byte.directory/api/v1/signatures/?page=11496",
  "results": [
    ...
  ]
}

curl -s -X POST https://eth.merkle.io \
-H "Content-Type: application/json" \
-d '{"jsonrpc":"2.0","method":"eth_getBlockByNumber","params":["0x16b7982",true],"id":1}' | jq > etc/b/23820674.json
