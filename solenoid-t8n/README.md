git clone https://github.com/ethereum/execution-spec-tests
cd execution-spec-tests

uv python install 3.12
uv sync --all-extras

---

uv run fill tests/shanghai/eip3855_push0/ \
  --evm-bin=../target/release/solenoid-t8n \
  --fork=Shanghai \
  --clean \
  -vvv

uv run fill tests/ \
  --clean \
  --evm-bin=../target/release/solenoid-t8n \
  --fork=Cancun
