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
}' http://127.0.0.1:8080 | jq > $HASH.tx.json

curl -s -X POST -H "Content-Type: application/json" --data '{
    "jsonrpc": "2.0",
    "method": "eth_getTransactionByHash",
    "params": ["0xaf9bc110613cd93023b42756665d659c363a16823587642bc97555780df0a894"],
    "id": 1
}' http://127.0.0.1:8080 | jq

curl -s -X POST -H "Content-Type: application/json" --data '{
    "jsonrpc": "2.0",
    "method": "eth_getCode",
    "params": ["0xac612bd99ba27f51c612b0c5eaf798cfee6a0e0f","latest"],
    "id":1
}' http://127.0.0.1:8080

export HASH=0x073c6e8b5b748dff4d58bdb59fa2705f7ce9e32682678ca0aa541ace3b7eee52 && curl -s -X POST \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "eth_getTransactionReceipt",
    "params": ["$HASH"],
    "id": 1
  }' http://127.0.0.1:8080 | jq > $HASH.r.json

curl -s -X POST \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "eth_getTransactionReceipt",
    "params": ["0xaf9bc110613cd93023b42756665d659c363a16823587642bc97555780df0a894"],
    "id": 1
  }' http://127.0.0.1:8080 | jq
