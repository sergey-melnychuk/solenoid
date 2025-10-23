etc/run.sh 23027350 10
etc/run.sh 23027350 14

---

cargo run --release --example runner -- 23027350

// DONE! Matched transactions 129/129!
cargo run --release --example runner -- 23624962

cargo run --release --example runner -- 23634227

cargo run --release --example runner -- 23635557

cargo run --release --example runner -- 23635581

cargo run --release --example runner -- 23640313

cargo run --release --example runner -- 23641709

cargo run --release --example runner -- 23642294

---

cargo run --release --example runner -- 23027350 > etc/23027350.txt
cargo run --release --example runner -- 23624962 > etc/23624962.txt
cargo run --release --example runner -- 23634227 > etc/23634227.txt
cargo run --release --example runner -- 23635557 > etc/23635557.txt
cargo run --release --example runner -- 23635581 > etc/23635581.txt
cargo run --release --example runner -- 23640313 > etc/23640313.txt
cargo run --release --example runner -- 23641709 > etc/23641709.txt
cargo run --release --example runner -- 23642294 > etc/23642294.txt

#cargo run --release --example runner -- latest
