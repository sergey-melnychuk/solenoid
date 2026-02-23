#!/bin/bash

# Script to run runner on a range of blocks
# Usage: etc/run.sh <end_block> [number_of_blocks=1]

set -e

# Check if arguments are provided
if [ $# -lt 1 ]; then
    echo "Usage: $0 <end_block> [number_of_blocks=1]"
    echo "Examples:"
    echo "  $0 latest          # runs runner on the latest block"
    echo "  $0 latest 10       # runs runner on the last 10 blocks ending at latest"
    echo "  $0 23000000        # runs runner on block 23000000"
    echo "  $0 23000000 5      # runs runner on 5 blocks ending at block 23000000"
    exit 1
fi

export RUST_MIN_STACK=16777216

END_BLOCK_ARG=$1
NUM_BLOCKS=${2:-1}

# Validate that number of blocks is a positive number
if ! [[ "$NUM_BLOCKS" =~ ^[0-9]+$ ]] || [ "$NUM_BLOCKS" -le 0 ]; then
    echo "Error: Number of blocks must be a positive number"
    exit 1
fi

echo "---"

if cargo build --release --example runner --quiet; then
    echo "📦 Runner binary was built successfully"
else
    echo "❌ Failed to build runner"
    exit 1
fi

# Determine the end block
if [ "$END_BLOCK_ARG" = "latest" ]; then
    echo "🔍 Fetching latest block number..."
    END_BLOCK=$(cargo run --release --example latest --quiet 2>/dev/null)
    
    if [ -z "$END_BLOCK" ]; then
        echo "Error: Failed to fetch latest block number"
        exit 1
    fi
    echo "📊 Latest block: $END_BLOCK"
else
    # Validate that end block is a number
    if ! [[ "$END_BLOCK_ARG" =~ ^[0-9]+$ ]]; then
        echo "Error: End block must be a number or 'latest'"
        exit 1
    fi
    END_BLOCK=$END_BLOCK_ARG
fi

START_BLOCK=$((END_BLOCK - NUM_BLOCKS + 1))
echo "⚙️ Running $NUM_BLOCKS blocks: $START_BLOCK..$END_BLOCK"
SUCCESS=0
FAILED=0

for ((BLOCK=$START_BLOCK; BLOCK<=END_BLOCK; BLOCK++)); do    
    if ./target/release/examples/runner "$BLOCK" 2>&1 > etc/sync/$BLOCK.txt; then
        ((SUCCESS++))
        echo "✅ Block $BLOCK completed successfully"
    else
        ((FAILED++))
        echo "❌ Block $BLOCK failed"
    fi
done
