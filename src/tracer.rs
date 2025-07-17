use crate::common::{Word, address::Address, call::Call};

pub enum StackEvent {
    Push(Word),
    Pop(Word),
}

pub enum StateEvent {
    R(Address, Word, Word),
    W(Address, Word, Word, Word),
}

pub enum MemoryEvent {
    R(usize, Vec<u8>),
    W(usize, Vec<u8>),
}

#[derive(Clone, Debug, Default)]
pub enum CallType {
    #[default]
    Call,
    Code,
    Static,
    Delegate,
}

pub enum EventData {
    Opcode {
        pc: usize,
        op: u8,
        name: String,
        data: Option<Vec<u8>>,
        gas: Word,
    },
    Keccak {
        data: Vec<u8>,
        hash: [u8; 32],
    },
    Stack(StackEvent),
    State(StateEvent),
    Memory(MemoryEvent),
    Create(Address, Word, Vec<u8>),
    Call(Call, CallType),
    Return(Vec<u8>),
    Revert(Vec<u8>),
    Value(Address, Word, Word),
    Nonce(Address, u64),
}

pub struct Event {
    pub data: EventData,
    pub depth: usize,
    pub reverted: bool,
}

#[allow(unused_variables)] // default impl ignores all arguments
pub trait EventTracer: Default {
    fn get(&self) -> Vec<Event> {
        vec![]
    }
    fn add(&mut self, event: Event) {}
    fn fork(&self) -> Self {
        Self::default()
    }
    fn join(&mut self, other: Self, reverted: bool) {
        for mut event in other.get() {
            event.reverted = reverted;
            self.add(event);
        }
    }
}

#[derive(Default)]
pub struct NoopTracer;

impl EventTracer for NoopTracer {}
