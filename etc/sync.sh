#!/bin/sh
rm *.log
rm *.state.json

# watch -n 10 'grep "###" etc/*.txt'

if cargo build --release --example runner --quiet; then
    echo "✅ Runner built successfully"
else
    echo "❌ Runner build failed"
    exit 1
fi

etc/run.sh 23027350
etc/run.sh 24484346
etc/run.sh 24486519
etc/run.sh 24486570
etc/run.sh 24526715
etc/run.sh 24526755
etc/run.sh 24527394

etc/todo.sh
