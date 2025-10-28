cargo run --release --example runner -- 23641709
cargo run --release --example check -- 23641709 8

cargo run --release --example runner -- 23640313
cargo run --release --example check -- 23640313 83

cargo run --release --example runner -- 23642294
cargo run --release --example check -- 23642294 23

---

cargo run --release --example runner -- 23676566
cargo run --release --example check -- 23676566 101

cargo run --release --example runner -- 23676595
cargo run --release --example check -- 23676595 148

cargo run --release --example runner -- 23676648
cargo run --release --example check -- 23676648 27

cargo run --release --example runner -- 23676766
cargo run --release --example check -- 23676766 19

cargo run --release --example runner -- 23677645
cargo run --release --example check -- 23677645 124

cargo run --release --example runner -- 23678121
cargo run --release --example check -- 23678121 106

etc/ab.sh 23678156 19
etc/ab.sh 23678156 53
etc/ab.sh 23678588 197
etc/ab.sh 23678620 4
etc/ab.sh 23678620 31
etc/ab.sh 23678620 83
etc/ab.sh 23678686 0
etc/ab.sh 23678721 91
etc/ab.sh 23678721 137
etc/ab.sh 23678747 290

---

cargo run --release --example runner -- 23676997

cargo run --release --example check -- 23676997 3
etc/ab.sh 23676997 3

etc/ab.sh 23676997 2 // SELFDESTRUCT

---

23676831 64

etc/ab.sh 23676766 64
cargo run --release --example runner -- 23676766
...
64
runner(93662,0x16e907000) malloc: Failed to allocate segment from range group - out of space

-

etc/ab.sh 23678007 0
cargo run --release --example runner -- 23678007
cargo run --release --example check -- 23678007 0

---

cargo run --release --example runner -- latest
