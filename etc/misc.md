```bash
curl -X POST \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "eth_getBlockByNumber",
    "params": ["0x15f5e96", true],
    "id": 1
  }' \
  https://eth.llamarpc.com | jq .result > 23027350.json
```
