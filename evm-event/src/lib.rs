use evm_common::{Hex, address::Address, word::Word};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AccountEvent {
    GetCode {
        address: Address,
        codehash: Word,
        bytecode: Hex,
    },
    Create {
        address: Address,
        creator: Address,
        nonce: Word,
        value: Word,
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
    // Destroy {
    //     address: Address,
    //     beneficiary: Address,
    //     balance: Word,
    // },
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum EventTag {
    Block(u64, Word),
    Tx(Word),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum HashAlg {
    Keccak256,
}

fn is_zero_i64(x: &i64) -> bool {
    x == &0
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum HaltReason {
    InvalidOpcode,
    OutOfGas,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OpCode {
    pub pc: usize,
    pub op: u8,
    pub name: String,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Hex>,
    pub gas_cost: i64,
    pub gas_used: i64,
    pub gas_left: i64,
    #[serde(default)]
    #[serde(skip_serializing_if = "is_zero_i64")]
    pub gas_back: i64,
    pub stack: Vec<Word>,
    pub memory: Vec<Word>,
    pub debug: Value,
}

impl Eq for OpCode {}

impl PartialEq for OpCode {
    fn eq(&self, that: &Self) -> bool {
        self.pc == that.pc
            && self.op == that.op
            && self.name == that.name
        // && self.data == that.data
            && self.gas_cost == that.gas_cost
            && self.gas_used == that.gas_used
            && self.gas_left == that.gas_left
            && self.gas_back == that.gas_back
            && self.stack == that.stack
            && self.memory == that.memory
        // && self.debug == that.debug
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum EventData {
    Tag(EventTag),

    Halt(HaltReason),

    OpCode(OpCode),

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

    Created {
        address: Address,
        codehash: Hex,
        balance: Word,
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

    Fee {
        gas: Word,
        price: Word,
        total: Word,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub data: EventData,
    pub depth: usize,
    pub reverted: bool,
}

impl PartialEq for Event {
    fn eq(&self, that: &Self) -> bool {
        self.data == that.data && self.depth == that.depth
    }
}

impl Eq for Event {}
