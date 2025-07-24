use crate::common::{Word, address::Address, call::Call};

#[derive(Debug)]
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

#[derive(Debug)]
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

#[derive(Clone, Copy, Debug, Default)]
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

#[derive(Debug)]
pub enum EventData {
    Init(String),

    OpCode {
        pc: usize,
        op: u8,
        name: String,
        data: Option<Vec<u8>>,
    },
    GasSub {
        pc: usize,
        op: u8,
        name: String,
        gas: Word,
    },
    GasAdd {
        pc: usize,
        op: u8,
        name: String,
        gas: Word,
    },

    Keccak {
        data: Vec<u8>,
        hash: [u8; 32],
    },

    State(StateEvent),
    Account(AccountEvent),

    Call {
        call: Call, 
        r#type: CallType,
    },

    Return {
        data: Vec<u8>,
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
        data: Vec<u8>,
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

#[derive(Debug)]
pub struct Event {
    pub data: EventData,
    pub depth: usize,
    pub reverted: bool,
}

#[allow(unused_variables)] // default impl ignores all arguments
pub trait EventTracer: Default {
    fn push(&mut self, event: Event) {
        #[cfg(feature = "tracing")]
        eprintln!("TRACER: {event:?}");
    }

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
