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
etc/run.sh 23624962
etc/run.sh 23634227
etc/run.sh 23635557
etc/run.sh 23635581
etc/run.sh 23640313
etc/run.sh 23641709
etc/run.sh 23642294
etc/run.sh 23647631
etc/run.sh 23647653
etc/run.sh 23678007
etc/run.sh 23678686
etc/run.sh 23678721
etc/run.sh 23683035
etc/run.sh 23683764
etc/run.sh 23690323
etc/run.sh 23828643
etc/run.sh 23890624
etc/run.sh 23890628
etc/run.sh 23890632

etc/run.sh 23728678 10
etc/run.sh 23820674 10
etc/run.sh 23882432 10
etc/run.sh 23884145 10
etc/run.sh 23884838 10

etc/run.sh 23891590 30

etc/todo.sh
