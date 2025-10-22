etc/run.sh 23027350 11
etc/run.sh 23027350 14

etc/run.sh 23624962 33
etc/run.sh 23624962 48

etc/run.sh 23635581 3

---

cargo run --release --example runner -- 23027350

cargo run --release --example runner -- 23624962

cargo run --release --example runner -- 23634227

cargo run --release --example runner -- 23635557

cargo run --release --example runner -- 23635581

---

cargo run --release --example runner -- 23027350 > etc/23027350.txt
cargo run --release --example runner -- 23624962 > etc/23624962.txt
cargo run --release --example runner -- 23634227 > etc/23634227.txt
cargo run --release --example runner -- 23635557 > etc/23635557.txt
cargo run --release --example runner -- 23635581 > etc/23635581.txt

#cargo run --release --example runner -- latest
