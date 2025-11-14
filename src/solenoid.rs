use crate::{
    common::{address::Address, block::Header, call::Call, hash::keccak256, word::Word},
    decoder::Decoder,
    executor::{Context, Evm, Executor, Gas},
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

        let exe = Executor::<LoggingTracer>::with_tracer(LoggingTracer::default());
        let exe = exe.with_header(self.header);

        // EIP-3651 (Shanghai): Pre-warm coinbase address
        if !coinbase.is_zero() {
            ext.warm_address(&coinbase);
        }

        let code = if self.call.to.is_zero() {
            self.call.data.clone()
        } else {
            let (code, _) = ext.code(&self.call.to).await?;
            code
        };

        // Check and resolve delegation: CODE = <0xef0100> + <20 bytes address>
        let code = if code.len() == 23 && code.starts_with(&[0xef, 0x01, 0x00]) {
            let target = Address::try_from(&code[3..]).expect("address");
            // eprintln!("DEBUG: delegation {} -> {}", self.call.to, target);
            let (code, _) = ext.code(&target).await?;
            code
        } else {
            code
        };

        let code = Decoder::decode(code);
        let mut evm = Evm::new();

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

        evm.gas = Gas::new(self.call.gas.as_i64() - upfront_gas_reduction);

        ext.pull(&self.call.from).await?;
        let nonce = ext.account_mut(&self.call.from).nonce;
        let address: Address = self.call.from.create(nonce);

        if !self.call.to.is_zero() {
            let (tracer, ret) = exe.execute(&code, &self.call, &mut evm, ext).await?;
            if evm.reverted {
                evm.revert(ext).await?;
                // Re-increment nonce (nonce is never reverted for valid transactions)
                ext.account_mut(&self.call.from).nonce += Word::one();
            }
            return Ok(CallResult {
                evm,
                ret,
                tracer,
                created: Some(address),
            });
        };

        let ctx = Context {
            created: address,
            call_type: CallType::Create,
            depth: 1,
            ..Default::default()
        };
        let (tracer, ret) = exe
            .execute_with_context(&code, &self.call, &mut evm, ext, ctx)
            .await;

        if evm.reverted {
            evm.revert(ext).await?;
            // Re-increment nonce (nonce is never reverted for valid transactions)
            ext.account_mut(&self.call.from).nonce += Word::one();
        } else {
            ext.pull(&address).await?;
            ext.pull(&self.call.from).await?;

            let hash = Word::from_bytes(&keccak256(&ret));
            *ext.code_mut(&address) = (ret.clone(), hash);
            ext.account_mut(&self.call.from).nonce += Word::one();
        }

        Ok(CallResult {
            evm,
            ret,
            tracer,
            created: Some(address),
        })
    }
}

#[derive(Debug, Default)]
pub struct CallResult<T: EventTracer> {
    pub evm: Evm,
    pub ret: Vec<u8>,
    pub tracer: T,
    pub created: Option<Address>,
}
