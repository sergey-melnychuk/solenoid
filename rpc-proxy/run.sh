#!/bin/sh
cargo build --release --quiet
killall rpc-proxy 2>/dev/null || true
RUST_LOG=warn nohup ./target/release/rpc-proxy > proxy.log 2>&1 &
sleep 2
resp=$(curl -s "http://${BIND_ADDR:-127.0.0.1:8080}/ready")
block=$(echo "$resp" | jq -r '.block // empty')
if [ -n "$block" ]; then
    echo "OK: $block"
else
    echo "FAIL: $(echo "$resp" | jq -r '.error // "unknown"')"
fi
