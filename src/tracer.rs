use serde::{Deserialize, Serialize};

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
        gas_refund: Word,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum AccountEvent {
    Deploy {
        address: Address,
        code_hash: Word,
        byte_code: Vec<u8>,
    },
    Nonce {
        address: Address,
        new: u64,
    },
    Value {
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
pub enum EventData {
    Init(String),

    /* TODO: opcode-level traces are pretty much useless */
    // OpCode {
    //     pc: usize,
    //     op: u8,
    //     name: String,
    //     data: Hex,
    //     gas: Word,
    // },
    Keccak {
        data: Hex,
        hash: Hex,
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
        data: Hex,
        gas_used: Word,
    },

    SelfDestruct {
        address: Address,
        beneficiary: Address,
        balance: Word,
    },

    Log {
        address: Address,
        topics: Vec<Word>,
        data: Hex,
    },

    // BlockHead {
    //     number: u64,
    //     hash: Word,
    //     // extra: gas_cost, etc
    // },
    // BlockDone {
    //     number: u64,
    //     hash: Word,
    //     execution_millis: u64,
    // },
    // TxHead {
    //     index: u64,
    //     hash: Word,
    //     call: Call,
    //     gas_limit: Word,
    //     // extra?
    // },
    // TxDone {
    //     index: u64,
    //     hash: Word,
    //     status: Word,
    //     gas_used: Word,
    //     execution_millis: u64,
    //     // more?
    // },
    // Init {
    //     chain_id: u64,
    //     spec: u64,
    //     // extra?
    // },
    // Reorg(/* TODO */),
    // Fork(/* TODO */),
    Error(String),
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
pub struct LogingTracer(Vec<Event>);

impl EventTracer for LogingTracer {
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
