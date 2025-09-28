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
        let exe = Executor::<LoggingTracer>::with_tracer(LoggingTracer::default());
        let exe = exe.with_header(self.header);

        let code = if self.call.to.is_zero() {
            Decoder::decode(self.call.data.clone())
        } else {
            let (code, _) = ext.code(&self.call.to).await?;
            Decoder::decode(code)
        };

        let mut evm = Evm::new();
        evm.gas = Gas::new(self.call.gas.as_i64());

        ext.pull(&self.call.from).await?;
        let nonce = ext.acc_mut(&self.call.from).nonce;
        let address: Address = self.call.from.of_smart_contract(nonce);

        if !self.call.to.is_zero() {
            let (tracer, ret) = exe.execute(&code, &self.call, &mut evm, ext).await?;
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
            ..Default::default()
        };
        let (tracer, ret) = exe
            .execute_with_context(&code, &self.call, &mut evm, ext, ctx)
            .await;

        let hash = Word::from_bytes(&keccak256(&ret));
        *ext.code_mut(&address) = (ret.clone(), hash);
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
pub struct CallResult<T: EventTracer> {
    pub evm: Evm,
    pub ret: Vec<u8>,
    pub tracer: T,
    pub created: Option<Address>,
}
