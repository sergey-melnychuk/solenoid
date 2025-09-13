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

---

Next discrepancy: CALLER result of a nested CALL.

LINE 1804:

BLOCK:
{"pc":1094,"op":51,"name":"CALLER","gas_used":308,"gas_cost":2,"gas_refunded":0,"stack":["0xd0e30db0","0x3d2","0x4cf03ea06ac4000","0x3","0x0","0x16dd82f7d46de18cfc860a12271b71cd8443e2e"],"memory":["0x0","0x0","0x60"],"depth":3}

TRACE:
{"pc":1094,"op":51,"name":"CALLER","gas_used":308,"gas_cost":2,"gas_refunded":0,"stack":["0xd0e30db0","0x3d2","0x4cf03ea06ac4000","0x3","0x0","0x3a10dc1a145da500d5fba38b9ec49c8ff11a981f"],"memory":["0x0","0x0","0x60"],"depth":3}
