#!/bin/sh
rm *.log

# watch -n 10 'grep "###" etc/*.txt'

if cargo build --release --example runner --quiet; then
    echo "✅ Runner built successfully"
else
    echo "❌ Runner build failed"
    exit 1
fi

etc/run.sh 23027350 1
etc/run.sh 23624962 1
etc/run.sh 23634227 1
etc/run.sh 23635557 1
etc/run.sh 23635581 1
etc/run.sh 23640313 1
etc/run.sh 23641709 1
etc/run.sh 23642294 1
etc/run.sh 23647631 1
etc/run.sh 23647653 1
etc/run.sh 23678007 1
etc/run.sh 23678686 1
etc/run.sh 23678721 1
etc/run.sh 23683035 1
etc/run.sh 23683764 1
etc/run.sh 23690323 1
etc/run.sh 23828643 1

etc/run.sh 23728678 10
etc/run.sh 23820674 10
etc/run.sh 23882432 10
etc/run.sh 23884145 10
etc/run.sh 23884838 10

etc/todo.sh
