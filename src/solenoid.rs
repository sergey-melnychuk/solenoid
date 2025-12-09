use serde::{Deserialize, Serialize};

use crate::{
    common::{address::Address, block::Header, call::Call, hash::keccak256, word::Word},
    decoder::Decoder,
    executor::{AccountTouch, Context, Evm, Executor, Gas},
    ext::Ext,
    tracer::{CallType, EventTracer, LoggingTracer},
};

#[derive(Default)]
pub struct Solenoid {}

impl Solenoid {
    pub fn new() -> Self {
        Self {}
    }

    pub fn create(&self, code: Vec<u8>) -> CreateBuilder {
        CreateBuilder {
            code,
            ..Default::default()
        }
    }

    pub fn execute(&self, to: Address, method: &str, args: &[u8]) -> ExecuteBuilder {
        let mut data = Vec::with_capacity(args.len() + 4);
        if !method.is_empty() {
            let hash = keccak256(method.as_bytes());
            data.extend_from_slice(&hash[..4]);
        }
        data.extend_from_slice(args);
        ExecuteBuilder {
            to,
            data,
            ..Default::default()
        }
    }

    pub fn transfer(&self, to: Address, value: Word) -> TransferBuilder {
        TransferBuilder {
            to,
            value,
            ..Default::default()
        }
    }
}

pub trait Builder {
    fn with_header(self, header: Header) -> Self;
    fn with_sender(self, sender: Address) -> Self;
    fn with_value(self, amount: Word) -> Self;
    fn with_gas(self, gas: Word) -> Self;
    fn ready(self) -> Runner;
}

#[derive(Default)]
pub struct CreateBuilder {
    header: Header,
    from: Address,
    value: Word,
    gas: Word,
    code: Vec<u8>,
}

impl Builder for CreateBuilder {
    fn with_header(mut self, header: Header) -> Self {
        self.header = header;
        self
    }

    fn with_sender(mut self, sender: Address) -> Self {
        self.from = sender;
        self
    }

    fn with_value(mut self, value: Word) -> Self {
        self.value = value;
        self
    }

    fn with_gas(mut self, gas: Word) -> Self {
        self.gas = gas;
        self
    }

    fn ready(self) -> Runner {
        Runner {
            header: self.header,
            call: Call {
                from: self.from,
                to: Address::zero(),
                value: self.value,
                gas: self.gas,
                ..Default::default()
            },
            code: self.code,
        }
    }
}

#[derive(Default)]
pub struct ExecuteBuilder {
    header: Header,
    from: Address,
    to: Address,
    value: Word,
    gas: Word,
    data: Vec<u8>,
}

impl Builder for ExecuteBuilder {
    fn with_header(mut self, header: Header) -> Self {
        self.header = header;
        self
    }

    fn with_sender(mut self, sender: Address) -> Self {
        self.from = sender;
        self
    }

    fn with_value(mut self, value: Word) -> Self {
        self.value = value;
        self
    }

    fn with_gas(mut self, gas: Word) -> Self {
        self.gas = gas;
        self
    }

    fn ready(self) -> Runner {
        Runner {
            header: self.header,
            call: Call {
                from: self.from,
                to: self.to,
                value: self.value,
                gas: self.gas,
                data: self.data,
            },
            code: vec![],
        }
    }
}

#[derive(Default)]
pub struct TransferBuilder {
    header: Header,
    from: Address,
    to: Address,
    value: Word,
    gas: Word,
}

impl Builder for TransferBuilder {
    fn with_header(mut self, header: Header) -> Self {
        self.header = header;
        self
    }

    fn with_sender(mut self, sender: Address) -> Self {
        self.from = sender;
        self
    }

    fn with_value(mut self, value: Word) -> Self {
        self.value = value;
        self
    }

    fn with_gas(mut self, gas: Word) -> Self {
        self.gas = gas;
        self
    }

    fn ready(self) -> Runner {
        Runner {
            header: self.header,
            call: Call {
                from: self.from,
                to: self.to,
                value: self.value,
                gas: self.gas,
                data: vec![],
            },
            code: vec![],
        }
    }
}

pub struct Runner {
    header: Header,
    call: Call,
    #[allow(dead_code)] // TODO: sort this out
    code: Vec<u8>,
}

impl Runner {
    pub async fn apply(self, ext: &mut Ext) -> eyre::Result<CallResult<LoggingTracer>> {
        let coinbase = self.header.miner;
        let base_fee = self.header.base_fee;

        let exe = Executor::<LoggingTracer>::with_tracer(LoggingTracer::default());
        let exe = exe.with_header(self.header);

        // EIP-3651 (Shanghai): Pre-warm coinbase address
        if !coinbase.is_zero() {
            ext.warm_address(&coinbase);
        }

        let mut evm = Evm::new();

        let code = if self.call.to.is_zero() {
            self.call.data.clone()
        } else {
            let (code, codehash) = ext.code(&self.call.to).await?;
            evm.touches.push(AccountTouch::GetCode(self.call.to, codehash, code.clone()));
            code
        };

        // Check and resolve delegation: CODE = <0xef0100> + <20 bytes address>
        let code = if code.len() == 23 && code.starts_with(&[0xef, 0x01, 0x00]) {
            let target = Address::try_from(&code[3..]).expect("address");
            // eprintln!("DEBUG: delegation {} -> {}", self.call.to, target);
            let (code, codehash) = ext.code(&target).await?;
            evm.touches.push(AccountTouch::GetCode(target, codehash, code.clone()));
            code
        } else {
            code
        };

        let code = Decoder::decode(code);

        let call_cost = 21000i64;
        let data_cost = {
            let total_calldata_len = self.call.data.len();
            let nonzero_bytes_count = self.call.data.iter().filter(|byte| *byte != &0).count();
            nonzero_bytes_count * 16 + (total_calldata_len - nonzero_bytes_count) * 4
        } as i64;
        let upfront_gas_reduction = if self.call.to.is_zero() {
            let create_cost = 32000i64;
            let init_code_cost = 2 * self.call.data.len().div_ceil(32) as i64;
            data_cost + create_cost + call_cost + init_code_cost
        } else {
            call_cost + data_cost
        };

        let upfront_gas_reduction = upfront_gas_reduction + ext.tx_ctx.access_list_cost();
        ext.apply_access_list();

        evm.gas = Gas::new(self.call.gas.as_i64() - upfront_gas_reduction);

        ext.pull(&self.call.from).await?;
        let nonce = ext.account_mut(&self.call.from).nonce;
        let created = self.call.from.create(nonce);
        ext.created_accounts.push(created);
        ext.warm_address(&created);
        evm.touches.push(AccountTouch::WarmUp(created));

        if !self.call.to.is_zero() {
            let (tracer, ret) = exe.execute(&code, &self.call, &mut evm, ext).await?;
            if evm.reverted {
                // evm.revert(ext).await?; // do not re-revert
                // Re-increment nonce (nonce is never reverted for valid transactions)
                ext.account_mut(&self.call.from).nonce += Word::one();
            }

            let gas_final = evm.gas
                .finalized(upfront_gas_reduction, evm.reverted);

            return Ok(CallResult {
                evm,
                ret,
                tracer,
                gas: GasResult {
                    gas_max: self.call.gas.as_i64(),
                    gas_use: gas_final,
                    gas_fee: Word::from(gas_final) * ext.tx_ctx.gas_price,
                },    
            });
        };

        let ctx = Context {
            created,
            call_type: CallType::Create,
            depth: 1,
            ..Default::default()
        };
        let (tracer, mut ret) = exe
            .execute_with_context(&code, &self.call, &mut evm, ext, ctx)
            .await;

        let deployed_code_cost = 200 * ret.len() as i64;
        let gas_final = if !evm.reverted && evm.gas.remaining() < deployed_code_cost {
            // Not enough gas to cover deployed code cost
            ret.clear();
            evm.reverted = true;
            let gas_limit = self.call.gas.as_i64();
            evm.gas(gas_limit).ok();
            gas_limit
        } else {
            evm.gas
                .finalized(upfront_gas_reduction + deployed_code_cost, evm.reverted)
        };

        // Transfer priority fee to coinbase (same logic as in executor.rs)
        // TODO: Avoid duplication of fee calculation code (see executor.rs)
        if !coinbase.is_zero() {
            let coinbase_gas_price = if ext.tx_ctx.gas_max_priority_fee.is_zero() {
                // Legacy transaction
                ext.tx_ctx.gas_price.saturating_sub(base_fee)
            } else {
                // EIP-1559 transaction
                let effective_gas_price = {
                    let base_plus_priority = base_fee + ext.tx_ctx.gas_max_priority_fee;
                    Word::min(ext.tx_ctx.gas_max_fee, base_plus_priority)
                };
                effective_gas_price.saturating_sub(base_fee)
            };

            let priority_fee_total = Word::from(gas_final) * coinbase_gas_price;

            if !priority_fee_total.is_zero() {
                // TODO charge the fee from tx.sender account
                ext.pull(&coinbase).await?;
                let current_coinbase_balance = ext.account_mut(&coinbase).value;
                let new_coinbase_balance = current_coinbase_balance + priority_fee_total;
                ext.account_mut(&coinbase).value = new_coinbase_balance;
                evm.touches.push(AccountTouch::FeePay(coinbase, current_coinbase_balance, new_coinbase_balance));
                // println!("[SOLE] COINBASE (GAS-C): {new_coinbase_balance:#x} *{priority_fee_total:#x}");
            }
        }

        if evm.reverted {
            evm.revert(ext).await?;
            // Re-increment nonce (nonce is never reverted even for failed tx)
            let nonce = ext.account_mut(&self.call.from).nonce;
            ext.account_mut(&self.call.from).nonce = nonce + Word::one();
            evm.touches.push(AccountTouch::SetNonce(self.call.from, nonce.as_u64(), nonce.as_u64() + 1));
        } else {
            ext.pull(&created).await?;
            ext.pull(&self.call.from).await?;

            let nonce = ext.account_mut(&self.call.from).nonce;
            ext.account_mut(&self.call.from).nonce = nonce + Word::one();
            evm.touches.push(AccountTouch::SetNonce(self.call.from, nonce.as_u64(), nonce.as_u64() + 1));

            let hash = Word::from_bytes(&keccak256(&ret));
            *ext.code_mut(&created) = (ret.clone(), hash);
            ext.account_mut(&created).nonce = Word::one();
            // TODO: check for transferred balance into newly created contract
            evm.touches.push(AccountTouch::Create(created, Word::zero(), Word::one(), ret.clone(), hash));
        }

        // Deduct gas fee from sender
        let gas_fee = Word::from(gas_final) * ext.tx_ctx.gas_price;
        let sender_balance = ext.balance(&self.call.from).await?;
        let new_sender_balance = sender_balance.saturating_sub(gas_fee);
        ext.account_mut(&self.call.from).value = new_sender_balance;
        evm.touches.push(AccountTouch::FeePay(self.call.from, sender_balance, new_sender_balance));

        Ok(CallResult {
            evm,
            ret,
            tracer,
            gas: GasResult {
                gas_max: self.call.gas.as_i64(),
                gas_use: gas_final,
                gas_fee: Word::from(gas_final) * ext.tx_ctx.gas_price,
            },
        })
    }
}

pub struct CallResult<T: EventTracer> {
    pub evm: Evm,
    pub ret: Vec<u8>,
    pub tracer: T,
    pub gas: GasResult,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct GasResult {
    pub gas_max: i64,
    pub gas_use: i64,
    pub gas_fee: Word,
}
