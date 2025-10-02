#[cfg(not(target_arch = "wasm32"))]
use evm_tracer::OpcodeTrace;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::common::{Hex, address::Address, word::Word};

#[derive(Debug, Serialize, Deserialize)]
pub enum StateEvent {
    Get {
        address: Address,
        key: Word,
        val: Word,
    },
    Put {
        address: Address,
        key: Word,
        val: Word,
        new: Word,
        gas_refund: i64,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum AccountEvent {
    GetCode {
        address: Address,
        codehash: Word,
        bytecode: Hex,
    },
    // TODO: add necessary events in the executor
    // GetNonce {
    //     address: Address,
    //     val: u64,
    // },
    // GetValue {
    //     address: Address,
    //     val: Word,
    // },
    SetCode {
        address: Address,
        codehash: Word,
        bytecode: Hex,
    },
    SetNonce {
        address: Address,
        val: u64,
        new: u64,
    },
    SetValue {
        address: Address,
        val: Word,
        new: Word,
    },
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub enum CallType {
    #[default]
    Call,
    Create,
    Create2,
    Static,
    Delegate,
    Callcode,
    Precompile(Address),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum EventTag {
    Block(u64, Word),
    Tx(Word),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum HashAlg {
    Keccak256,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum EventData {
    Tag(EventTag),

    OpCode {
        pc: usize,
        op: u8,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<Hex>,
        gas_cost: i64,
        gas_used: i64,
        gas_left: i64,
        // #[serde(skip_serializing_if = "Word::is_zero")]
        gas_back: i64,
        stack: Vec<Word>,
        memory: Vec<Word>,
        extra: Value,
    },

    Hash {
        data: Hex,
        hash: Hex,
        alg: HashAlg,
    },

    State(StateEvent),

    Account(AccountEvent),

    Call {
        data: Hex,
        value: Word,
        from: Address,
        to: Address,
        gas: Word,
        r#type: CallType,
    },

    Return {
        ok: bool,
        data: Hex,
        gas_used: i64,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },

    // SELFDESTRUCT
    Remove {
        address: Address,
        beneficiary: Address,
        balance: Word,
    },

    Log {
        address: Address,
        topics: Vec<Word>,
        data: Hex,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Event {
    pub data: EventData,
    pub depth: usize,
    pub reverted: bool,
}

pub trait EventTracer: Default {
    fn push(&mut self, _event: Event) {}

    fn peek(&self) -> &[Event] {
        &[]
    }

    fn take(&mut self) -> Vec<Event> {
        vec![]
    }

    fn fork(&self) -> Self {
        Self::default()
    }

    fn join(&mut self, mut other: Self, reverted: bool) {
        for mut event in other.take() {
            event.reverted = reverted;
            self.push(event);
        }
    }
}

#[derive(Default)]
pub struct NoopTracer;

impl EventTracer for NoopTracer {}

#[derive(Default)]
pub struct LoggingTracer(Vec<Event>);

impl EventTracer for LoggingTracer {
    fn push(&mut self, event: Event) {
        self.0.push(event);
    }

    fn peek(&self) -> &[Event] {
        &self.0
    }

    fn take(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.0)
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg(feature = "testkit")]
impl TryFrom<Event> for OpcodeTrace {
    type Error = eyre::Error;

    fn try_from(value: Event) -> Result<Self, Self::Error> {
        use evm_tracer::Extra;

        let depth = value.depth;
        match value.data {
            EventData::OpCode {
                pc,
                op,
                name,
                data: _,
                gas_cost,
                gas_used,
                gas_left,
                stack,
                memory,
                gas_back,
                extra,
            } => Ok(OpcodeTrace {
                pc: pc as u64,
                op,
                name,
                gas_used,
                gas_left,
                gas_cost,
                gas_back,
                stack: stack
                    .into_iter()
                    .map(|x| hex::encode(x.into_bytes()))
                    .collect(),
                memory: memory
                    .into_iter()
                    .map(|x| hex::encode(x.into_bytes()))
                    .collect(),
                depth,
                extra: Extra::new(extra),
            }),
            _ => eyre::bail!("Not an opcode"),
        }
    }
}
