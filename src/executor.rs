use std::{collections::HashMap, time::Instant};

use i256::I256;
use primitive_types::U256;
use thiserror::Error;

use crate::{
    common::{account::Account, address::Address, hash::keccak256},
    decoder::{Bytecode, Decoder, DecoderError, Instruction},
    eth::EthClient,
};

#[derive(Error, Debug)]
pub enum ExecutorError {
    #[error("Stack overflow")]
    StackOverflow,
    #[error("Stack underflow")]
    StackUnderflow,
    #[error("Call depth limit reached")]
    CallDepthLimitReached,
    #[error("Out of memory: {0} bytes requested")]
    OutOfMemory(usize),
    #[error("Invalid jump")]
    InvalidJump,
    #[error("Missing data")]
    MissingData,
    #[error("Invalid opcode: {0:#02x}")]
    InvalidOpcode(u8),
    #[error("Unknown opcode: {0:#02x}")]
    UnknownOpcode(u8),
    #[error("Wrong returned data size: expected {exp} but got {got}")]
    WrongCallRetDataSize { exp: usize, got: usize },
    #[error("Bytcode decoding error: {0}")]
    DecoderError(#[from] DecoderError),

    #[error("{0}")]
    Eyre(#[from] eyre::ErrReport),
}

const STACK_LIMIT: usize = 1024;

const CALL_DEPTH_LIMIT: usize = 1024;

#[derive(Debug, Default)]
pub struct Evm {
    pub stack: Vec<U256>,
    pub memory: Vec<u8>,
    pub pc: usize,
    pub stopped: bool,
    pub reverted: bool,
    pub state: Vec<(Address, U256, U256, Option<U256>)>,
}

impl Evm {
    pub fn push(&mut self, value: U256) -> Result<(), ExecutorError> {
        if self.stack.len() >= STACK_LIMIT {
            return Err(ExecutorError::StackOverflow);
        }
        self.stack.push(value);
        Ok(())
    }

    pub fn pop(&mut self) -> Result<U256, ExecutorError> {
        self.stack.pop().ok_or(ExecutorError::StackUnderflow)
    }
}

#[derive(Clone, Debug)]
pub struct Call {
    pub calldata: Vec<u8>,
    pub value: U256,
    pub from: Address,
    pub to: Address,
}

#[derive(Default)]
pub struct State {
    account: Account,
    data: HashMap<U256, U256>,
    code: Vec<u8>,
}

pub struct Ext {
    block_hash: String,
    state: HashMap<Address, State>,
    eth: EthClient,
}

impl Ext {
    pub fn new(block_hash: String, eth: EthClient) -> Self {
        Self {
            block_hash,
            state: Default::default(),
            eth,
        }
    }

    pub async fn get(&mut self, addr: &Address, key: &U256) -> eyre::Result<U256> {
        let val = if let Some(val) = self.state.get(addr).and_then(|s| s.data.get(key)).copied() {
            val
        } else {
            let now = Instant::now();
            let hex = format!("0x{key:064x}");
            let address = format!("0x{}", hex::encode(addr.0));
            let val = self
                .eth
                .get_storage_at(&self.block_hash, &address, &hex)
                .await?;
            let ms = now.elapsed().as_millis();
            let addr = hex::encode(addr.0);
            tracing::info!("SLOAD: [{ms} ms] 0x{addr}[{key:#x}]={val:#x}");
            val
        };
        Ok(val)
    }

    pub async fn put(&mut self, addr: &Address, key: U256, val: U256) -> eyre::Result<()> {
        let state = self.state.entry(*addr).or_default();
        state.data.insert(key, val);
        Ok(())
    }

    pub async fn acc(&mut self, addr: &Address) -> eyre::Result<Account> {
        if let Some(acc) = self.state.get(addr).map(|s| s.account.clone()) {
            Ok(acc)
        } else {
            let address = format!("0x{}", hex::encode(addr.0));
            let account = self.eth.get_account(&self.block_hash, &address).await?;

            let state = self.state.entry(*addr).or_default();
            state.account = account.clone();
            Ok(account)
        }
    }

    pub async fn code(&mut self, addr: &Address) -> eyre::Result<Vec<u8>> {
        if let Some(code) = self.state.get(addr).map(|s| s.code.clone()) {
            Ok(code)
        } else {
            let address = format!("0x{}", hex::encode(addr.0));
            let code = self.eth.get_code(&self.block_hash, &address).await?;

            let state = self.state.entry(*addr).or_default();
            state.code = code.clone();
            Ok(code)
        }
    }

    pub fn acc_mut(&mut self, addr: &Address) -> Option<&mut Account> {
        self.state.get_mut(addr).map(|s| &mut s.account)
    }

    pub fn code_mut(&mut self, addr: &Address) -> Option<&mut Vec<u8>> {
        self.state.get_mut(addr).map(|s| &mut s.code)
    }
}

pub enum StackEvent {
    Push(U256),
    Pop(U256),
}

pub enum StateEvent {
    R(Address, U256, U256),
    W(Address, U256, U256, U256),
}

pub enum MemoryEvent {
    R(usize, Vec<u8>),
    W(usize, Vec<u8>),
}

pub enum EventData {
    Opcode {
        pc: usize,
        op: u8,
        name: String,
        data: Option<Vec<u8>>,
    },
    Keccak {
        data: Vec<u8>,
        hash: [u8; 32],
    },
    // TODO: Gas
    // TODO: Created { code, addr }
    Stack(StackEvent),
    State(StateEvent),
    Memory(MemoryEvent),
    Call(Call, Option<Context>),
    Return(Vec<u8>),
    // Balance, Nonce (, Tx, Block): these happen on higher level then execution of call
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

#[derive(Clone)]
pub struct Context {
    // TODO: gas
    pub depth: usize,
    pub from: Address,
    pub origin: Address,
    pub is_static_call: bool,
    pub is_delegate_call: bool,
}

#[derive(Default)]
pub struct Executor<T: EventTracer> {
    tracer: T,
    evm: Evm,
    ret: Vec<u8>,
    log: bool,
}

impl<T: EventTracer> Executor<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_log(self) -> Self {
        Self { log: true, ..self }
    }

    pub fn with_tracer<G: EventTracer>(tracer: G) -> Executor<G> {
        Executor {
            tracer,
            ..Default::default()
        }
    }

    pub async fn execute(
        mut self,
        code: &Bytecode,
        call: &Call,
        ext: &mut Ext,
    ) -> Result<(T, Evm, Vec<u8>), ExecutorError> {
        self.tracer.add(Event {
            reverted: false,
            depth: 0,
            data: EventData::Call(call.clone(), None),
        });
        let mut ctx = Context {
            depth: 0,
            from: call.from,
            origin: call.from,
            is_delegate_call: false,
            is_static_call: false,
        };
        self.execute_with_context(code, call, &mut ctx, ext).await
    }

    async fn execute_with_context(
        mut self,
        code: &Bytecode,
        call: &Call,
        ctx: &mut Context,
        ext: &mut Ext,
    ) -> Result<(T, Evm, Vec<u8>), ExecutorError> {
        if ctx.depth > CALL_DEPTH_LIMIT {
            return Err(ExecutorError::CallDepthLimitReached);
        }

        while !self.evm.stopped && self.evm.pc < code.instructions.len() {
            let instruction = &code.instructions[self.evm.pc];
            self.tracer.add(Event {
                depth: ctx.depth,
                reverted: false,
                data: EventData::Opcode {
                    pc: self.evm.pc,
                    op: instruction.opcode.code,
                    name: instruction.opcode.name(),
                    data: instruction.argument.clone(),
                },
            });

            if self.log {
                let data = instruction
                    .argument
                    .as_ref()
                    .map(|data| format!("0x{}", hex::encode(data)));
                println!(
                    "\n{:#04x}: {} {}",
                    self.evm.pc,
                    instruction.opcode.name(),
                    data.unwrap_or_default()
                );
            }

            self.execute_instruction(code, call, ctx, ext, instruction)
                .await?;

            if self.log {
                println!(
                    "MEMORY:{}",
                    if self.evm.memory.is_empty() {
                        " []"
                    } else {
                        ""
                    }
                );
                self.evm
                    .memory
                    .chunks(32)
                    .enumerate()
                    .for_each(|(index, word)| {
                        let offset = index << 5;
                        let word = hex::encode(word);
                        println!("{offset:#04x}: {word}");
                    });
                println!(
                    "STACK:{}",
                    if self.evm.stack.is_empty() { " []" } else { "" }
                );
                self.evm
                    .stack
                    .iter()
                    .rev()
                    .enumerate()
                    .for_each(|(i, word)| println!("{:>4}: {word:#02x}", i + 1));
            }
        }

        Ok((self.tracer, self.evm, self.ret))
    }

    pub async fn execute_instruction(
        &mut self,
        code: &Bytecode,
        call: &Call,
        ctx: &mut Context,
        ext: &mut Ext,
        instruction: &Instruction,
    ) -> Result<(), ExecutorError> {
        let mut pc_increment = true;

        let opcode = instruction.opcode.code;
        match opcode {
            // 0x00: STOP
            0x00 => {
                self.evm.stopped = true;
                self.evm.reverted = false;
                self.ret.clear();
            }
            // 0x01..0x0b: Arithmetic Operations
            0x01 => {
                // ADD
                let a = self.evm.pop()?;
                let b = self.evm.pop()?;
                let (res, _) = a.overflowing_add(b);
                self.evm.push(res)?;
            }
            0x02 => {
                // MUL
                let a = self.evm.pop()?;
                let b = self.evm.pop()?;
                let (res, _) = a.overflowing_mul(b);
                self.evm.push(res)?;
            }
            0x03 => {
                // SUB
                let a = self.evm.pop()?;
                let b = self.evm.pop()?;
                let (res, _) = a.overflowing_sub(b);
                self.evm.push(res)?;
            }
            0x04 => {
                // DIV
                let a = self.evm.pop()?;
                let b = self.evm.pop()?;
                if b.is_zero() || a.is_zero() {
                    self.evm.push(U256::zero())?;
                } else {
                    self.evm.push(a / b)?;
                }
            }
            0x05 => {
                // SDIV
                let a = self.evm.pop()?;
                let b = self.evm.pop()?;
                let a_signed = I256::from_be_bytes(a.to_big_endian());
                let b_signed = I256::from_be_bytes(b.to_big_endian());
                let res = if b.is_zero() {
                    I256::from(0)
                } else if a_signed == I256::MIN && b_signed == I256::from(-1) {
                    I256::MIN
                } else {
                    a_signed / b_signed
                };
                self.evm.push(U256::from_big_endian(&res.to_be_bytes()))?;
            }
            0x06 => {
                // MOD
                let a = self.evm.pop()?;
                let b = self.evm.pop()?;
                if b.is_zero() {
                    self.evm.push(U256::zero())?;
                } else {
                    self.evm.push(a % b)?;
                }
            }
            0x07 => {
                // SMOD
                let a = self.evm.pop()?;
                let b = self.evm.pop()?;
                let a_signed = I256::from_be_bytes(a.to_big_endian());
                let b_signed = I256::from_be_bytes(b.to_big_endian());
                let res = if b.is_zero() {
                    I256::from(0)
                } else {
                    a_signed % b_signed
                };
                self.evm.push(U256::from_big_endian(&res.to_be_bytes()))?;
            }
            0x08 => {
                // ADDMOD
                todo!()
            }
            0x09 => {
                // MULMOD
                todo!()
            }
            0x0a => {
                // EXP
                let base = self.evm.pop()?;
                let exponent = self.evm.pop()?;
                self.evm.push(base.pow(exponent))?;
            }
            0x0b => {
                // SIGNEXTEND
                todo!()
            }

            // 0x10s: Comparison & Bitwise Logic
            0x10 => {
                // LT
                let a = self.evm.pop()?;
                let b = self.evm.pop()?;
                self.evm
                    .push(if a < b { U256::one() } else { U256::zero() })?;
            }
            0x11 => {
                // GT
                let a = self.evm.pop()?;
                let b = self.evm.pop()?;
                self.evm
                    .push(if a > b { U256::one() } else { U256::zero() })?;
            }
            0x12 => {
                // SLT
                let a = self.evm.pop()?;
                let b = self.evm.pop()?;
                let a_signed = I256::from_be_bytes(a.to_big_endian());
                let b_signed = I256::from_be_bytes(b.to_big_endian());
                self.evm.push(if a_signed < b_signed {
                    U256::one()
                } else {
                    U256::zero()
                })?;
            }
            0x13 => {
                // SGT
                let a = self.evm.pop()?;
                let b = self.evm.pop()?;
                let a_signed = I256::from_be_bytes(a.to_big_endian());
                let b_signed = I256::from_be_bytes(b.to_big_endian());
                self.evm.push(if a_signed > b_signed {
                    U256::one()
                } else {
                    U256::zero()
                })?;
            }
            0x14 => {
                // EQ
                let a = self.evm.pop()?;
                let b = self.evm.pop()?;
                self.evm
                    .push(if a == b { U256::one() } else { U256::zero() })?;
            }
            0x15 => {
                // ISZERO
                let a = self.evm.pop()?;
                self.evm.push(if a.is_zero() {
                    U256::one()
                } else {
                    U256::zero()
                })?;
            }
            0x16 => {
                // AND
                let a = self.evm.pop()?;
                let b = self.evm.pop()?;
                self.evm.push(a & b)?;
            }
            0x17 => {
                // OR
                let a = self.evm.pop()?;
                let b = self.evm.pop()?;
                self.evm.push(a | b)?;
            }
            0x18 => {
                // XOR
                let a = self.evm.pop()?;
                let b = self.evm.pop()?;
                self.evm.push(a ^ b)?;
            }
            0x19 => {
                // NOT
                let a = self.evm.pop()?;
                self.evm.push(!a)?;
            }
            0x1a => {
                // BYTE
                let index = self.evm.pop()?;
                let value: U256 = self.evm.pop()?;
                if index < U256::from(32) {
                    let byte_index = 31 - index.as_usize();
                    self.evm.push(U256::from(value.byte(byte_index)))?;
                } else {
                    self.evm.push(U256::zero())?;
                }
            }
            0x1b => {
                // SHL
                let shift = self.evm.pop()?.as_usize();
                let value = self.evm.pop()?;

                let ret = value << shift;
                self.evm.push(ret)?;
            }
            0x1c => {
                // SHR
                let shift = self.evm.pop()?.as_usize();
                let value = self.evm.pop()?;

                let ret = value >> shift;
                self.evm.push(ret)?;
            }
            0x1d => {
                // SAR
                todo!("0x1d:SAR")
            }

            0x20 => {
                // SHA3 (KECCAK256)
                let offset = self.evm.pop()?.as_usize();
                let size = self.evm.pop()?.as_usize();

                if offset + size > self.evm.memory.len() {
                    return Err(ExecutorError::MissingData);
                }
                let data = &self.evm.memory[offset..offset + size];
                let hash = U256::from_big_endian(&keccak256(data));
                self.evm.push(hash)?;
            }

            // 30-3f
            0x33 => {
                // CALLER
                self.evm.push((&call.from).into())?;
            }
            0x34 => {
                // CALLVALUE
                self.evm.push(call.value)?;
            }
            0x35 => {
                // CALLDATALOAD
                let offset = self.evm.pop()?.as_usize();
                if offset > call.calldata.len() {
                    return Err(ExecutorError::MissingData);
                }
                let mut data = [0u8; 32];
                let copy = call.calldata.len().min(offset + 32) - offset;
                data[0..copy].copy_from_slice(&call.calldata[offset..offset + copy]);
                self.evm.push(U256::from_big_endian(&data))?;
            }
            0x36 => {
                // CALLDATASIZE
                self.evm.push(U256::from(call.calldata.len()))?;
            }
            0x39 => {
                // CODECOPY
                let dest_offset = self.evm.pop()?.as_usize();
                let offset = self.evm.pop()?.as_usize();
                let size = self.evm.pop()?.as_usize();

                if self.evm.memory.len() < dest_offset + size {
                    self.evm.memory.resize(dest_offset + size, 0);
                }

                self.evm.memory[dest_offset..dest_offset + size]
                    .copy_from_slice(&code.bytecode[offset..offset + size]);
            }

            // 40-4a

            // 0x50s: Stack, Memory, Storage and Flow Operations
            0x50 => {
                // POP
                self.evm.pop()?;
            }
            0x51 => {
                // MLOAD
                let offset = self.evm.pop()?.as_usize();
                let end = offset + 32;
                if end > self.evm.memory.len() {
                    self.evm.memory.resize(end, 0);
                }
                let value = U256::from_big_endian(&self.evm.memory[offset..end]);
                self.evm.push(value)?;
            }
            0x52 => {
                // MSTORE
                let offset = self.evm.pop()?.as_usize();
                let value = self.evm.pop()?;
                let end = offset + 32;
                if end > self.evm.memory.len() {
                    self.evm.memory.resize(end, 0);
                }
                let bytes = &value.to_big_endian();
                self.evm.memory[offset..end].copy_from_slice(bytes);
            }
            0x53 => {
                // MSTORE8
                let offset = self.evm.pop()?.as_usize();
                let value = self.evm.pop()?;
                if offset >= self.evm.memory.len() {
                    self.evm.memory.resize(offset + 1, 0);
                }
                self.evm.memory[offset] = value.to_little_endian()[0];
            }
            0x54 => {
                // SLOAD
                let key = self.evm.pop()?;
                let val = ext.get(&call.to, &key).await?;
                self.evm.push(val)?;
                self.evm.state.push((call.to, key, val, None));
                self.tracer.add(Event {
                    data: EventData::State(StateEvent::R(call.to, key, val)),
                    depth: ctx.depth,
                    reverted: false,
                });
            }
            0x55 => {
                // SSTORE
                let key = self.evm.pop()?;
                let val = ext.get(&call.to, &key).await?;
                let new = self.evm.pop()?;
                ext.put(&call.to, key, val).await?;
                self.evm.state.push((call.to, key, val, Some(new)));
                self.tracer.add(Event {
                    data: EventData::State(StateEvent::W(call.to, key, val, new)),
                    depth: ctx.depth,
                    reverted: false,
                });
            }
            0x56 => {
                // JUMP
                let dest = self.evm.pop()?.as_usize();
                let dest = code.resolve_jump(dest).ok_or(ExecutorError::InvalidJump)?;
                if code.instructions[dest].opcode.code != 0x5b {
                    return Err(ExecutorError::InvalidJump);
                }
                self.evm.pc = dest;
                pc_increment = false;
            }
            0x57 => {
                // JUMPI
                let dest = self.evm.pop()?.as_usize();
                let dest = code.resolve_jump(dest).ok_or(ExecutorError::InvalidJump)?;
                let cond = self.evm.pop()?;
                if !cond.is_zero() {
                    if code.instructions[dest].opcode.code != 0x5b {
                        return Err(ExecutorError::InvalidJump);
                    }
                    self.evm.pc = dest;
                    pc_increment = false;
                }
            }
            0x58 => {
                // PC
                self.evm.push(U256::from(instruction.offset))?;
            }
            0x59 => {
                // MSIZE
                self.evm.push(U256::from(self.evm.memory.len()))?;
            }
            0x5b => {
                // JUMPDEST: noop, a valid destination for JUMP/JUMPI
            }

            0x5f => {
                // PUSH0
                self.evm.push(U256::zero())?;
            }
            // 0x60..=0x7f: PUSH1 to PUSH32
            0x60..=0x7f => {
                let arg = instruction
                    .argument
                    .as_ref()
                    .ok_or(ExecutorError::MissingData)?;
                self.evm.push(U256::from_big_endian(arg))?;
            }

            // 0x80..=0x8f: DUP1 to DUP16
            0x80..=0x8f => {
                let n = instruction.opcode.n as usize;
                if self.evm.stack.len() < n {
                    return Err(ExecutorError::StackUnderflow);
                }
                let val = self.evm.stack[self.evm.stack.len() - n];
                self.evm.push(val)?;
            }

            // 0x90..=0x9f: SWAP1 to SWAP16
            0x90..=0x9f => {
                let n = instruction.opcode.n as usize;
                if self.evm.stack.len() <= n {
                    return Err(ExecutorError::StackUnderflow);
                }
                let stack_len = self.evm.stack.len();
                self.evm.stack.swap(stack_len - 1, stack_len - 1 - n);
            }

            #[allow(unused_variables)]
            0xf0 => {
                // CREATE
                let value = self.evm.pop()?;
                let offset = self.evm.pop()?;
                let size = self.evm.pop()?;

                todo!("CREATE");
                // put address of the created contract on the stack
            }
            #[allow(unused_variables)]
            0xf1 => {
                // CALL
                let gas = self.evm.pop()?;
                let address = &self.evm.pop()?;
                let value = self.evm.pop()?;
                let args_offset = self.evm.pop()?.as_usize();
                let args_size = self.evm.pop()?.as_usize();
                let ret_offset = self.evm.pop()?.as_usize();
                let ret_size = self.evm.pop()?.as_usize();

                let bytecode = ext.code(&address.into()).await?;
                let code = Decoder::decode(bytecode)?;
                let executor = Executor::<T>::with_tracer(self.tracer.fork());

                let nested_call = Call {
                    calldata: self.evm.memory[args_offset..args_offset + args_size].to_vec(),
                    value,
                    from: call.to,
                    to: address.into(),
                };

                let mut nexted_ctx = Context {
                    depth: ctx.depth + 1,
                    from: call.to,
                    origin: ctx.origin,
                    is_static_call: false,
                    is_delegate_call: false,
                };

                let future =
                    executor.execute_with_context(&code, &nested_call, &mut nexted_ctx, ext);
                let (tracer, evm, ret) = Box::pin(future).await?;
                self.tracer.join(tracer, evm.reverted);

                if !evm.reverted {
                    if ret.len() == ret_size {
                        let size = ret_offset + ret_size;
                        if size > self.evm.memory.len() {
                            if size > 1_000_000_000 {
                                // TODO: make the limit configurable
                                return Err(ExecutorError::OutOfMemory(size));
                            }
                            self.evm.memory.resize(size, 0);
                        }
                        self.evm.memory[ret_offset..ret_offset + ret_size].copy_from_slice(&ret);
                        self.evm.push(U256::one())?;
                    }
                } else {
                    for (address, key, val, _) in
                        evm.state.iter().filter(|(_, _, _, new)| new.is_some())
                    {
                        ext.put(address, *key, *val).await?;
                    }
                    self.evm.push(U256::zero())?;
                }
            }
            #[allow(unused_variables)]
            0xf2 => {
                // CALLCODE
                unimplemented!("CALLCODE");
                // let gas = self.state.pop()?;
                // let address = self.state.pop()?;
                // let value = self.state.pop()?;
                // let args_offset = self.state.pop()?;
                // let args_size = self.state.pop()?;
                // let ret_offset = self.state.pop()?;
                // let ret_size = self.state.pop()?;
            }
            0xf3 | 0xfd => {
                // RETURN | REVERT
                self.evm.stopped = true;
                self.evm.reverted = opcode == 0xfd;

                let offset = self.evm.pop()?.as_usize();
                let size = self.evm.pop()?.as_usize();

                if size > 0 {
                    if offset > self.evm.memory.len() || offset + size > self.evm.memory.len() {
                        return Err(ExecutorError::MissingData);
                    }
                    self.ret = self.evm.memory[offset..offset + size].to_vec();
                } else {
                    self.ret.clear();
                }
            }
            #[allow(unused_variables)]
            0xf4 => {
                // DELEGATECALL
                let gas = self.evm.pop()?;
                let address = self.evm.pop()?;
                let args_offset = self.evm.pop()?;
                let args_size = self.evm.pop()?;
                let ret_offset = self.evm.pop()?;
                let ret_size = self.evm.pop()?;

                todo!("DELEGATECALL");
            }
            #[allow(unused_variables)]
            0xf5 => {
                // CREATE2
                let value = self.evm.pop()?;
                let offset = self.evm.pop()?;
                let size = self.evm.pop()?;
                let salt = self.evm.pop()?;

                todo!("CREATE2");
                // put address of the created contract on the stack
            }
            #[allow(unused_variables)]
            0xfa => {
                // STATICCALL
                let gas = self.evm.pop()?;
                let address = self.evm.pop()?;
                let args_offset = self.evm.pop()?;
                let args_size = self.evm.pop()?;
                let ret_offset = self.evm.pop()?;
                let ret_size = self.evm.pop()?;

                todo!("STATICCALL");
            }
            0xfe => {
                // INVALID
                self.evm.stopped = true;
                self.evm.reverted = true;
                // TODO: gas: consume or refund?
            }
            0xff => {
                // SELFDESTRUCT
                todo!("SELFDESTRUCT");
            }
            _ => {
                return Err(ExecutorError::UnknownOpcode(opcode));
            }
        }

        if pc_increment {
            self.evm.pc += 1;
        }

        Ok(())
    }
}
