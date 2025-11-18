#!/bin/sh
rm *.log

etc/recent.sh 23728678 10
etc/recent.sh 23820674 10

cargo run --release --example runner -- 23027350 > etc/23027350.txt
cargo run --release --example runner -- 23624962 > etc/23624962.txt
cargo run --release --example runner -- 23634227 > etc/23634227.txt
cargo run --release --example runner -- 23635557 > etc/23635557.txt
cargo run --release --example runner -- 23635581 > etc/23635581.txt
cargo run --release --example runner -- 23640313 > etc/23640313.txt
cargo run --release --example runner -- 23641709 > etc/23641709.txt
cargo run --release --example runner -- 23642294 > etc/23642294.txt
cargo run --release --example runner -- 23647631 > etc/23647631.txt
cargo run --release --example runner -- 23647653 > etc/23647653.txt
cargo run --release --example runner -- 23678686 > etc/23678686.txt
cargo run --release --example runner -- 23683764 > etc/23683764.txt
cargo run --release --example runner -- 23678721 > etc/23678721.txt
cargo run --release --example runner -- 23683035 > etc/23683035.txt
cargo run --release --example runner -- 23678007 > etc/23678007.txt
cargo run --release --example runner -- 23690323 > etc/23690323.txt
#cargo run --release --example runner --  > etc/.txt
