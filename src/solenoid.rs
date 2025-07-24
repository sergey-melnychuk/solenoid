use crate::{
    common::{Word, address::Address, call::Call, hash::keccak256},
    decoder::Decoder,
    executor::{Context, Evm, Executor, Gas},
    ext::Ext,
    tracer::{CallType, EventTracer, NoopTracer},
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
        let hash = keccak256(method.as_bytes());
        data.extend_from_slice(&hash[..4]);
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
    fn with_sender(self, sender: Address) -> Self;
    fn with_value(self, amount: Word) -> Self;
    fn with_gas(self, gas: Word) -> Self;
    fn ready(self) -> Runner;
}

#[derive(Default)]
pub struct CreateBuilder {
    from: Address,
    value: Word,
    gas: Word,
    code: Vec<u8>,
}

impl Builder for CreateBuilder {
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
    from: Address,
    to: Address,
    value: Word,
    gas: Word,
    data: Vec<u8>,
}

impl Builder for ExecuteBuilder {
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
    from: Address,
    to: Address,
    value: Word,
    gas: Word,
}

impl Builder for TransferBuilder {
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
    call: Call,
    code: Vec<u8>,
}

impl Runner {
    pub async fn apply(self, ext: &mut Ext) -> eyre::Result<CallResult> {
        let exe = Executor::<NoopTracer>::new();

        let code = if self.call.to.is_zero() {
            Decoder::decode(self.code)?
        } else {
            let code = ext.code(&self.call.to).await?;
            Decoder::decode(code)?
        };

        let mut evm = Evm::new();

        if !self.call.to.is_zero() {
            let (tracer, ret) = exe.execute(&code, &self.call, &mut evm, ext).await?;
            return Ok(CallResult {
                evm,
                ret,
                tracer,
                created: None,
            });
        };

        let nonce = ext.acc_mut(&self.call.from).nonce;
        let address: Address = self.call.from.of_smart_contract(nonce);

        let ctx = Context {
            created: address,
            call_type: CallType::Create,
            ..Default::default()
        };
        evm.gas = Gas::new(self.call.gas);
        let (tracer, ret) = exe
            .execute_with_context(&code, &self.call, &mut evm, ext, ctx)
            .await?;

        *ext.code_mut(&address) = ret.clone();
        ext.acc_mut(&address).code = Word::from_big_endian(&keccak256(&ret));
        ext.acc_mut(&self.call.from).nonce += Word::one();

        Ok(CallResult {
            evm,
            ret,
            tracer,
            created: Some(address),
        })
    }
}

#[derive(Debug, Default)]
pub struct CallResult<T: EventTracer = NoopTracer> {
    pub evm: Evm,
    pub ret: Vec<u8>,
    pub tracer: T,
    pub created: Option<Address>,
}
