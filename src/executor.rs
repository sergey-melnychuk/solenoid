use std::time::SystemTime;

use i256::I256;
use thiserror::Error;

use crate::{
    common::{
        address::Address,
        call::Call,
        hash::{self, keccak256},
        word::Word,
    },
    decoder::{Bytecode, Decoder, DecoderError, Instruction},
    ext::Ext,
    tracer::{AccountEvent, CallType, Event, EventData, EventTracer, StateEvent},
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
    #[error("Wrong returned data size: expected {exp} but got {got}")]
    WrongReturnDataSize { exp: usize, got: usize },
    #[error("Invalid opcode: {0:#02x}")]
    InvalidOpcode(u8),
    #[error("Unknown opcode: {0:#02x}")]
    UnknownOpcode(u8),
    #[error("Bytcode decoding error: {0}")]
    DecoderError(#[from] DecoderError),
    #[error("Call run out of gas")]
    OutOfGas(),
    #[error("Insufficient funds: have {have:?}, need {need:?}")]
    InsufficientFunds { have: Word, need: Word },
    #[error("Unallowed opcode from static call: {0}")]
    StaticCallViolation(u8),
    #[error("{0}")]
    Eyre(#[from] eyre::ErrReport),
}

const STACK_LIMIT: usize = 1024;

const CALL_DEPTH_LIMIT: usize = 1024;

#[derive(Debug, Default, Eq, PartialEq)]
pub struct StateTouch(pub Address, pub Word, pub Word, pub Option<Word>, pub Word);

impl StateTouch {
    pub fn is_write(&self) -> bool {
        let Self(_, _, _, new, _) = self;
        new.is_some()
    }
}

#[derive(Debug, Default, Eq, PartialEq)]
pub enum AccountTouch {
    #[default]
    Empty,
    Nonce(Address, u64, u64),
    Value(Address, Word, Word),
    Code(Address, Word, Vec<u8>),
}

#[derive(Debug, Clone)]
pub struct Log(pub Address, pub Vec<Word>, pub Vec<u8>);

#[derive(Debug, Default)]
pub struct Evm {
    pub memory: Vec<u8>,
    pub stack: Vec<Word>,
    pub gas: Gas,
    pub pc: usize,
    pub stopped: bool,
    pub reverted: bool,

    pub mem_cost: Word,
    pub logs: Vec<Log>,
    pub state: Vec<StateTouch>,
    pub account: Vec<AccountTouch>,
}

impl Evm {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn memory_expansion_cost(&mut self) -> Word {
        let memory_byte_size = self.memory.len();
        let memory_size_word = memory_byte_size.div_ceil(32);
        let mem_cost = (memory_size_word * memory_size_word) / 512 + (3 * memory_size_word);
        let exp_cost = Word::from(mem_cost) - self.mem_cost;
        self.mem_cost = exp_cost;
        exp_cost
    }

    pub(crate) fn address_access_cost(&mut self, address: &Address, ext: &Ext) -> Word {
        let is_warm = ext.state.contains_key(address);
        if is_warm {
            Word::from(100)
        } else {
            Word::from(2600)
        }
    }

    pub(crate) fn error(&mut self, e: ExecutorError) -> Result<(), ExecutorError> {
        self.stopped = true;
        self.reverted = true;
        Err(e)
    }

    pub fn push(&mut self, value: Word) -> Result<(), ExecutorError> {
        if self.stack.len() >= STACK_LIMIT {
            self.error(ExecutorError::StackOverflow)?;
        }
        self.stack.push(value);
        Ok(())
    }

    pub fn pop(&mut self) -> Result<Word, ExecutorError> {
        if let Some(word) = self.stack.pop() {
            Ok(word)
        } else {
            self.error(ExecutorError::StackUnderflow)
                .map(|_| Word::zero())
        }
    }

    pub fn gas(&mut self, cost: Word) -> Result<(), ExecutorError> {
        match self.gas.sub(cost) {
            Ok(_) => Ok(()),
            Err(e) => self.error(e),
        }
    }

    pub async fn get(
        &mut self,
        ext: &mut Ext,
        addr: &Address,
        key: &Word,
    ) -> Result<Word, ExecutorError> {
        match ext.get(addr, key).await {
            Ok(word) => Ok(word),
            Err(e) => self.error(e.into()).map(|_| Word::zero()),
        }
    }

    pub async fn put(
        &mut self,
        ext: &mut Ext,
        addr: &Address,
        key: Word,
        val: Word,
    ) -> Result<(), ExecutorError> {
        match ext.put(addr, key, val).await {
            Ok(_) => Ok(()),
            Err(e) => self.error(e.into()),
        }
    }

    pub async fn code(&mut self, ext: &mut Ext, addr: &Address) -> Result<Vec<u8>, ExecutorError> {
        match ext.code(addr).await {
            Ok(code) => Ok(code),
            Err(e) => self.error(e.into()).map(|_| Default::default()),
        }
    }

    pub async fn revert(&mut self, ext: &mut Ext) -> eyre::Result<()> {
        for StateTouch(address, key, val, _, gas) in self.state.iter().filter(|st| st.is_write()) {
            ext.put(address, *key, *val).await?;
            self.gas.add(*gas);
        }
        for ac in self.account.iter() {
            match ac {
                AccountTouch::Nonce(addr, val, _new) => {
                    ext.acc_mut(addr).nonce = (*val).into();
                }
                AccountTouch::Value(addr, val, _new) => {
                    ext.acc_mut(addr).balance = *val;
                }
                AccountTouch::Code(addr, _hash, _code) => {
                    ext.acc_mut(addr).code = Word::zero();
                    ext.code_mut(addr).clear();
                }
                AccountTouch::Empty => (),
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct Gas {
    pub limit: Word,
    pub used: Word,
}

impl Gas {
    pub fn new(limit: Word) -> Self {
        Self {
            limit,
            used: Word::zero(),
        }
    }

    pub fn remaining(&self) -> Word {
        self.limit.saturating_sub(self.used)
    }

    pub fn fork(&self, limit: Word) -> Self {
        Self {
            limit,
            used: Word::zero(),
        }
    }

    pub fn add(&mut self, gas: Word) {
        self.used -= gas;
    }

    pub fn sub(&mut self, gas: Word) -> Result<(), ExecutorError> {
        if gas > self.remaining() {
            return Err(ExecutorError::OutOfGas());
        }
        self.used += gas;
        Ok(())
    }
}

#[derive(Clone, Copy, Default)]
pub struct Context {
    pub created: Address,
    pub origin: Address,
    pub depth: usize,

    pub call_type: CallType,
    // block, gas price, etc
}

#[derive(Default)]
pub struct Executor<T: EventTracer> {
    tracer: T,
    ret: Vec<u8>,
    log: bool,
}

impl<T: EventTracer> Executor<T> {
    pub fn new() -> Self {
        let mut this = Self::default();
        let timestamp = SystemTime::UNIX_EPOCH.elapsed().unwrap().as_secs();
        this.tracer.push(Event {
            data: EventData::Init(format!("{{\"timestamp\":{timestamp}}}")),
            depth: 0,
            reverted: false,
        });
        this
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
        self,
        code: &Bytecode,
        call: &Call,
        evm: &mut Evm,
        ext: &mut Ext,
    ) -> Result<(T, Vec<u8>), ExecutorError> {
        evm.gas = Gas::new(call.gas);

        let call_cost = 21000;
        evm.gas.sub(call_cost.into())?;

        let data_cost = {
            let total_calldata_len = call.data.len();
            let nonzero_bytes_count = call.data.iter().filter(|byte| byte != &&0).count();
            nonzero_bytes_count * 16 + (total_calldata_len - nonzero_bytes_count) * 4
        };
        evm.gas.sub(data_cost.into())?;

        let is_create = call.to.is_zero();
        let is_transfer = code.bytecode.is_empty() || call.data.is_empty() && !is_create;
        if is_transfer {
            let src = ext.balance(&call.from).await?;
            let dst = ext.balance(&call.to).await?;

            if src < call.value {
                return Err(ExecutorError::InsufficientFunds {
                    have: src,
                    need: call.value,
                });
            }

            // TODO: handle EIP-1559 here?
            // See: https://www.blocknative.com/blog/eip-1559-fees
            let gas_price = Word::one() * 1_000_000.into(); // 100 Gwei
            let gas_fee = evm.gas.used * gas_price;

            if src < call.value + gas_fee {
                return Err(ExecutorError::InsufficientFunds {
                    have: src,
                    need: call.value + gas_fee,
                });
            }

            let nonce = ext.acc_mut(&call.from).nonce;
            ext.acc_mut(&call.from).nonce = nonce + Word::one();
            evm.account.push(AccountTouch::Nonce(
                call.from,
                nonce.as_u64(),
                nonce.as_u64() + 1,
            ));

            ext.acc_mut(&call.from).balance -= call.value + gas_fee;
            evm.account.push(AccountTouch::Value(
                call.from,
                src,
                src - call.value - gas_fee,
            ));

            ext.acc_mut(&call.to).balance += call.value;
            evm.account
                .push(AccountTouch::Value(call.to, dst, dst + call.value));

            evm.stopped = true;
            evm.reverted = false;
            Ok((self.tracer, vec![]))
        } else {
            let nonce = ext.acc_mut(&call.from).nonce;
            let address = call.from.of_smart_contract(nonce);
            let ctx = Context {
                created: address,
                ..Context::default()
            };
            self.execute_with_context(code, call, evm, ext, ctx).await
        }
    }

    pub async fn execute_with_context(
        mut self,
        code: &Bytecode,
        call: &Call,
        evm: &mut Evm,
        ext: &mut Ext,
        ctx: Context,
    ) -> Result<(T, Vec<u8>), ExecutorError> {
        self.tracer.push(Event {
            data: EventData::Call {
                r#type: ctx.call_type,
                data: call.data.clone().into(),
                value: call.value,
                from: call.from,
                to: call.to,
                gas: call.gas.as_u64(),
            },
            depth: ctx.depth,
            reverted: false,
        });

        if ctx.depth > CALL_DEPTH_LIMIT {
            return Err(ExecutorError::CallDepthLimitReached);
        }

        while !evm.stopped && evm.pc < code.instructions.len() {
            let instruction = &code.instructions[evm.pc];

            self.tracer.push(Event {
                depth: ctx.depth,
                reverted: false,
                data: EventData::OpCode {
                    pc: evm.pc,
                    op: instruction.opcode.code,
                    name: instruction.opcode.name(),
                    data: instruction.argument.clone().map(|vec| vec.into()),
                },
            });

            let cost = self
                .execute_instruction(code, call, evm, ext, ctx, instruction)
                .await?;

            self.tracer.push(Event {
                depth: ctx.depth,
                reverted: false,
                data: EventData::GasSub {
                    pc: evm.pc - 1,
                    op: instruction.opcode.code,
                    name: instruction.opcode.name(),
                    gas: cost.as_u64(),
                },
            });

            evm.gas(cost)?;

            if self.log {
                let data = instruction
                    .argument
                    .as_ref()
                    .map(|data| format!("0x{}", hex::encode(data)));
                println!(
                    "{:#04x}: {} {}",
                    evm.pc,
                    instruction.opcode.name(),
                    data.unwrap_or_default()
                );
                println!("MEMORY:{}", if evm.memory.is_empty() { " []" } else { "" });
                evm.memory.chunks(32).enumerate().for_each(|(index, word)| {
                    let offset = index << 5;
                    let word = hex::encode(word);
                    println!("{offset:#04x}: {word}");
                });
                println!("STACK:{}", if evm.stack.is_empty() { " []" } else { "" });
                evm.stack
                    .iter()
                    .rev()
                    .enumerate()
                    .for_each(|(i, word)| println!("{:>4}: {word:#02x}", i + 1));
                println!();
            }
        }

        Ok((self.tracer, self.ret))
    }

    async fn execute_instruction(
        &mut self,
        code: &Bytecode,
        call: &Call,
        evm: &mut Evm,
        ext: &mut Ext,
        ctx: Context,
        instruction: &Instruction,
    ) -> Result<Word, ExecutorError> {
        let mut gas = Word::zero();
        let mut pc_increment = true;

        let this = if call.to.is_zero() {
            ctx.created
        } else {
            call.to
        };

        let opcode = instruction.opcode.code;
        match opcode {
            // 0x00: STOP
            0x00 => {
                evm.stopped = true;
                evm.reverted = false;
                self.ret.clear();
            }
            // 0x01..0x0b: Arithmetic Operations
            0x01 => {
                // ADD
                let a = evm.pop()?;
                let b = evm.pop()?;
                let (res, _) = a.overflowing_add(b);
                evm.push(res)?;
                gas = 3.into();
            }
            0x02 => {
                // MUL
                let a = evm.pop()?;
                let b = evm.pop()?;
                let (res, _) = a.overflowing_mul(b);
                evm.push(res)?;
                gas = 5.into();
            }
            0x03 => {
                // SUB
                let a = evm.pop()?;
                let b = evm.pop()?;
                let (res, _) = a.overflowing_sub(b);
                evm.push(res)?;
                gas = 3.into();
            }
            0x04 => {
                // DIV
                let a = evm.pop()?;
                let b = evm.pop()?;
                if b.is_zero() || a.is_zero() {
                    evm.push(Word::zero())?;
                } else {
                    evm.push(a / b)?;
                }
                gas = 5.into();
            }
            0x05 => {
                // SDIV
                let a = evm.pop()?;
                let b = evm.pop()?;
                let a_signed = I256::from_be_bytes(a.into_bytes());
                let b_signed = I256::from_be_bytes(b.into_bytes());
                let res = if b.is_zero() {
                    I256::from(0)
                } else if a_signed == I256::MIN && b_signed == I256::from(-1) {
                    I256::MIN
                } else {
                    a_signed / b_signed
                };
                evm.push(Word::from_bytes(&res.to_be_bytes()))?;
                gas = 5.into();
            }
            0x06 => {
                // MOD
                let a = evm.pop()?;
                let b = evm.pop()?;
                if b.is_zero() {
                    evm.push(Word::zero())?;
                } else {
                    evm.push(a % b)?;
                }
                gas = 5.into();
            }
            0x07 => {
                // SMOD
                let a = evm.pop()?;
                let b = evm.pop()?;
                let a_signed = I256::from_be_bytes(a.into_bytes());
                let b_signed = I256::from_be_bytes(b.into_bytes());
                let res = if b.is_zero() {
                    I256::from(0)
                } else {
                    a_signed % b_signed
                };
                evm.push(Word::from_bytes(&res.to_be_bytes()))?;
                gas = 5.into();
            }
            0x08 => {
                // ADDMOD
                // gas = 8.into();
                todo!("ADDMOD");
            }
            0x09 => {
                // MULMOD
                // gas = 8.into();
                todo!("MULMOD");
            }
            0x0a => {
                // EXP
                let base = evm.pop()?;
                let exponent = evm.pop()?;
                evm.push(base.pow(exponent))?;

                let exp_bytes = exponent
                    .into_bytes()
                    .into_iter()
                    .skip_while(|byte| byte == &0)
                    .count();
                gas = (10 + exp_bytes * 50).into();
            }
            0x0b => {
                // SIGNEXTEND
                // gas = 5.into();
                todo!("SIGNEXTEND")
            }

            // 0x10s: Comparison & Bitwise Logic
            0x10 => {
                // LT
                let a = evm.pop()?;
                let b = evm.pop()?;
                evm.push(if a < b { Word::one() } else { Word::zero() })?;
                gas = 3.into();
            }
            0x11 => {
                // GT
                let a = evm.pop()?;
                let b = evm.pop()?;
                evm.push(if a > b { Word::one() } else { Word::zero() })?;
                gas = 3.into();
            }
            0x12 => {
                // SLT
                let a = evm.pop()?;
                let b = evm.pop()?;
                let a_signed = I256::from_be_bytes(a.into_bytes());
                let b_signed = I256::from_be_bytes(b.into_bytes());
                evm.push(if a_signed < b_signed {
                    Word::one()
                } else {
                    Word::zero()
                })?;
                gas = 3.into();
            }
            0x13 => {
                // SGT
                let a = evm.pop()?;
                let b = evm.pop()?;
                let a_signed = I256::from_be_bytes(a.into_bytes());
                let b_signed = I256::from_be_bytes(b.into_bytes());
                evm.push(if a_signed > b_signed {
                    Word::one()
                } else {
                    Word::zero()
                })?;
                gas = 3.into();
            }
            0x14 => {
                // EQ
                let a = evm.pop()?;
                let b = evm.pop()?;
                evm.push(if a == b { Word::one() } else { Word::zero() })?;
                gas = 3.into();
            }
            0x15 => {
                // ISZERO
                let a = evm.pop()?;
                evm.push(if a.is_zero() {
                    Word::one()
                } else {
                    Word::zero()
                })?;
                gas = 3.into();
            }
            0x16 => {
                // AND
                let a = evm.pop()?;
                let b = evm.pop()?;
                evm.push(a & b)?;
                gas = 3.into();
            }
            0x17 => {
                // OR
                let a = evm.pop()?;
                let b = evm.pop()?;
                evm.push(a | b)?;
                gas = 3.into();
            }
            0x18 => {
                // XOR
                let a = evm.pop()?;
                let b = evm.pop()?;
                evm.push(a ^ b)?;
                gas = 3.into();
            }
            0x19 => {
                // NOT
                let a = evm.pop()?;
                evm.push(!a)?;
                gas = 3.into();
            }
            0x1a => {
                // BYTE
                let index = evm.pop()?;
                let value: Word = evm.pop()?;
                if index < Word::from(32) {
                    let byte_index = 31 - index.as_usize();
                    evm.push(Word::from(value.into_bytes()[byte_index]))?;
                } else {
                    evm.push(Word::zero())?;
                }
                gas = 3.into();
            }
            0x1b => {
                // SHL
                let shift = evm.pop()?.as_usize();
                let value = evm.pop()?;
                let ret = value << shift;
                evm.push(ret)?;
                gas = 3.into();
            }
            0x1c => {
                // SHR
                let shift = evm.pop()?.as_usize();
                let value = evm.pop()?;
                let ret = value >> shift;
                evm.push(ret)?;
                gas = 3.into();
            }
            0x1d => {
                // SAR
                let shift = evm.pop()?.as_usize();
                let value = evm.pop()?;
                let value = I256::from_be_bytes(value.into_bytes());
                let ret = value >> shift;
                let ret = Word::from_bytes(&ret.to_be_bytes());
                evm.push(ret)?;
                gas = 3.into();
            }

            0x20 => {
                // SHA3 (KECCAK256)
                let offset = evm.pop()?.as_usize();
                let size = evm.pop()?.as_usize();
                if offset + size > evm.memory.len() {
                    return Err(ExecutorError::MissingData);
                }
                let data = &evm.memory[offset..offset + size];
                let hash = Word::from_bytes(&keccak256(data));
                evm.push(hash)?;
                gas = (30 + 6 * size.div_ceil(32)).into();
            }

            // 30-3f
            0x30 => {
                // ADDRESS
                evm.push((&this).into())?;
                gas = 2.into();
            }
            0x31 => {
                // BALANCE
                let addr = (&evm.pop()?).into();
                let is_warm = ext.state.contains_key(&addr);
                let value = ext.balance(&addr).await?;
                evm.push(value)?;
                gas = if is_warm {
                    100.into() // warm
                } else {
                    2600.into() // cold
                };
            }
            0x32 => {
                // ORIGIN
                evm.push((&ctx.origin).into())?;
                gas = 2.into();
            }
            0x33 => {
                // CALLER
                evm.push((&call.from).into())?;
                gas = 2.into();
            }
            0x34 => {
                // CALLVALUE
                evm.push(call.value)?;
                gas = 2.into();
            }
            0x35 => {
                // CALLDATALOAD
                let offset = evm.pop()?.as_usize();
                if offset > call.data.len() {
                    evm.error(ExecutorError::MissingData)?;
                }
                let mut data = [0u8; 32];
                let copy = call.data.len().min(offset + 32) - offset;
                data[0..copy].copy_from_slice(&call.data[offset..offset + copy]);
                evm.push(Word::from_bytes(&data))?;
                gas = 3.into();
            }
            0x36 => {
                // CALLDATASIZE
                evm.push(Word::from(call.data.len()))?;
                gas = 2.into();
            }
            0x37 => {
                // CALLDATACOPY
                let dest_offset = evm.pop()?.as_usize();
                let offset = evm.pop()?.as_usize();
                let size = evm.pop()?.as_usize();
                let len = dest_offset + size;
                if len > evm.memory.len() {
                    evm.memory.resize(len, 0);
                }
                evm.memory[dest_offset..dest_offset + size]
                    .copy_from_slice(&call.data[offset..offset + size]);
                gas = (3 + 3 * size.div_ceil(32)).into();
                gas += evm.memory_expansion_cost();
            }
            0x38 => {
                // CODESIZE
                let len = code.bytecode.len();
                evm.push(len.into())?;
                gas = 2.into();
            }
            0x39 => {
                // CODECOPY
                let dest_offset = evm.pop()?.as_usize();
                let offset = evm.pop()?.as_usize();
                let size = evm.pop()?.as_usize();
                if evm.memory.len() < dest_offset + size {
                    evm.memory.resize(dest_offset + size, 0);
                }
                evm.memory[dest_offset..dest_offset + size]
                    .copy_from_slice(&code.bytecode[offset..offset + size]);
                gas = (3 + 3 * size.div_ceil(32)).into();
                gas += evm.memory_expansion_cost();
            }
            0x3a => {
                // GASPRICE
                todo!("GASPRICE")
            }
            0x3b => {
                // EXTCODESIZE
                let address: Address = (&evm.pop()?).into();
                let code_size = ext.code(&address).await?.len();
                evm.push(Word::from(code_size))?;
                gas = evm.address_access_cost(&address, ext);
            }
            0x3c => {
                // EXTCODECOPY
                let address: Address = (&evm.pop()?).into();
                let dest_offset = evm.pop()?.as_usize();
                let offset = evm.pop()?.as_usize();
                let size = evm.pop()?.as_usize();

                let code = ext.code(&address).await?;
                if evm.memory.len() < dest_offset + size {
                    evm.memory.resize(dest_offset + size, 0);
                }
                evm.memory[dest_offset..dest_offset + size]
                    .copy_from_slice(&code[offset..offset + size]);
                gas = (3 * size.div_ceil(32)).into();
                gas += evm.memory_expansion_cost();
                gas += evm.address_access_cost(&address, ext);
            }
            0x3d => {
                // RETURNDATASIZE
                gas = 2.into();
                evm.push(self.ret.len().into())?;
            }
            0x3e => {
                // RETURNDATACOPY
                let dest_offset = evm.pop()?.as_usize();
                let offset = evm.pop()?.as_usize();
                let size = evm.pop()?.as_usize();
                if evm.memory.len() < dest_offset + size {
                    evm.memory.resize(dest_offset + size, 0);
                }
                if self.ret.len() < offset + size {
                    self.ret.resize(offset + size, 0);
                }
                evm.memory[dest_offset..dest_offset + size]
                    .copy_from_slice(&self.ret[offset..offset + size]);
                gas = (3 + 3 * size.div_ceil(32)).into();
                gas += evm.memory_expansion_cost();
            }
            0x3f => {
                // EXTCODEHASH
                let address: Address = (&evm.pop()?).into();
                let exists = ext.state.contains_key(&address);
                if !exists {
                    evm.push(Word::zero())?;
                }
                let code = ext.code(&address).await?;
                if code.is_empty() {
                    evm.push(Word::from_bytes(&hash::empty()))?;
                }
                evm.push(ext.acc_mut(&address).code)?;

                gas += evm.address_access_cost(&address, ext);
            }

            // 40-4a
            0x40 => {
                // BLOCKHASH
                todo!("BLOCKHASH")
            }
            0x41 => {
                // COINBASE
                todo!("COINBASE")
            }
            0x42 => {
                // TIMESTAMP
                todo!("TIMESTAMP")
            }
            0x43 => {
                // NUMBER
                todo!("NUMBER")
            }
            0x44 => {
                // PREVRANDAO
                todo!("PREVRANDAO")
            }
            0x45 => {
                // GASLIMIT
                todo!("GASLIMIT")
            }
            0x46 => {
                // CHAINID
                todo!("CHAINID")
            }
            0x47 => {
                // SELFBALANCE
                todo!("SELFBALANCE")
            }
            0x48 => {
                // BASEFEE
                todo!("BASEFEE")
            }
            0x49 => {
                // BLOBHASH
                todo!("BLOBHASH")
            }
            0x4a => {
                // BLOBBASEFEE
                todo!("BLOBBASEFEE")
            }

            // 0x50s: Stack, Memory, Storage and Flow Operations
            0x50 => {
                // POP
                evm.pop()?;
                gas = 2.into();
            }
            0x51 => {
                // MLOAD
                let offset = evm.pop()?.as_usize();
                let end = offset + 32;
                if end > evm.memory.len() {
                    evm.memory.resize(end, 0);
                }
                let value = Word::from_bytes(&evm.memory[offset..end]);
                evm.push(value)?;
                gas = 3.into();
                gas += evm.memory_expansion_cost();
            }
            0x52 => {
                // MSTORE
                let offset = evm.pop()?.as_usize();
                let value = evm.pop()?;
                let end = offset + 32;
                if end > evm.memory.len() {
                    evm.memory.resize(end, 0);
                }
                let bytes = &value.into_bytes();
                evm.memory[offset..end].copy_from_slice(bytes);
                gas = 3.into();
                gas += evm.memory_expansion_cost();
            }
            0x53 => {
                // MSTORE8
                let offset = evm.pop()?.as_usize();
                let value = evm.pop()?;
                if offset >= evm.memory.len() {
                    evm.memory.resize(offset + 1, 0);
                }
                evm.memory[offset] = value
                    .into_bytes()
                    .iter()
                    .rev()
                    .nth(0)
                    .copied()
                    .unwrap_or_default();
                gas = 3.into();
                gas += evm.memory_expansion_cost();
            }
            0x54 => {
                // SLOAD
                let key = evm.pop()?;
                let is_warm = ext
                    .state
                    .get(&this)
                    .map(|s| s.data.contains_key(&key))
                    .unwrap_or_default();
                let val = evm.get(ext, &this, &key).await?;
                evm.push(val)?;
                evm.state
                    .push(StateTouch(this, key, val, None, Word::zero()));
                self.tracer.push(Event {
                    data: EventData::State(StateEvent::Get {
                        address: this,
                        key,
                        val,
                    }),
                    depth: ctx.depth,
                    reverted: false,
                });
                gas = if is_warm {
                    100.into() // warm
                } else {
                    2100.into() // cold
                };
            }
            0x55 => {
                // SSTORE
                if matches!(ctx.call_type, CallType::Static) {
                    return Err(ExecutorError::StaticCallViolation(opcode));
                }
                let key = evm.pop()?;

                let is_warm = ext
                    .state
                    .get(&this)
                    .map(|s| s.data.contains_key(&key))
                    .unwrap_or_default();

                let val = evm.get(ext, &this, &key).await?;
                let original = ext.original.get(&(this, key)).cloned().unwrap_or_default();

                let new = evm.pop()?;
                evm.put(ext, &this, key, new).await?;

                // https://www.evm.codes/?fork=cancun#55
                let mut gas_cost = if val == new {
                    100
                } else if val == original {
                    if original.is_zero() { 20_000 } else { 2900 }
                } else {
                    100
                };
                if !is_warm {
                    gas_cost += 2100;
                }
                gas = gas_cost.into();

                // https://www.evm.codes/?fork=cancun#55
                let mut gas_refund = Word::zero();
                {
                    if val != new {
                        if val == original {
                            if !original.is_zero() && new.is_zero() {
                                gas_refund += 4800.into();
                            }
                        } else {
                            if !original.is_zero() {
                                if val.is_zero() {
                                    gas_refund = gas_refund.saturating_sub(4800.into());
                                } else if new.is_zero() {
                                    gas_refund += 4800.into();
                                }
                            }
                            if new == original {
                                if original.is_zero() {
                                    gas_refund += (20_000 - 100).into();
                                } else {
                                    gas_refund += (5000 - 2100 - 100).into();
                                }
                            }
                        }
                    }
                }
                self.tracer.push(Event {
                    data: EventData::State(StateEvent::Put {
                        address: this,
                        key,
                        val,
                        new,
                        gas_refund,
                    }),
                    depth: ctx.depth,
                    reverted: false,
                });
                evm.state
                    .push(StateTouch(this, key, val, Some(new), gas_refund));
            }
            0x56 => {
                // JUMP
                let dest = evm.pop()?.as_usize();
                let dest = code.resolve_jump(dest).ok_or(ExecutorError::InvalidJump)?;
                if code.instructions[dest].opcode.code != 0x5b {
                    evm.error(ExecutorError::InvalidJump)?;
                }
                evm.pc = dest;
                pc_increment = false;
                gas = 8.into();
            }
            0x57 => {
                // JUMPI
                let dest = evm.pop()?.as_usize();
                let dest = code.resolve_jump(dest).ok_or(ExecutorError::InvalidJump)?;
                let cond = evm.pop()?;
                if !cond.is_zero() {
                    if code.instructions[dest].opcode.code != 0x5b {
                        evm.error(ExecutorError::InvalidJump)?;
                    }
                    evm.pc = dest;
                    pc_increment = false;
                }
                gas = 10.into();
            }
            0x58 => {
                // PC
                evm.push(Word::from(instruction.offset))?;
                gas = 2.into();
            }
            0x59 => {
                // MSIZE
                evm.push(Word::from(evm.memory.len()))?;
                gas = 2.into();
            }
            0x5a => {
                // GAS
                evm.push(evm.gas.remaining() - Word::from(2))?;
                gas = 2.into();
            }
            0x5b => {
                // JUMPDEST: noop, a valid destination for JUMP/JUMPI
                gas = 1.into();
            }
            0x5c => {
                // TLOAD
                // gas = 100.into();
                todo!("TLOAD");
            }
            0x5d => {
                // TSTORE
                // gas = 100.into();
                todo!("TSTORE");
            }
            0x5e => {
                // MCOPY
                let dest_offset = evm.pop()?.as_usize();
                let offset = evm.pop()?.as_usize();
                let size = evm.pop()?.as_usize();
                let len = dest_offset + size;
                if len > evm.memory.len() {
                    evm.memory.resize(len, 0);
                }
                let mut buffer = Vec::with_capacity(size);
                buffer.copy_from_slice(&evm.memory[offset..offset + size]);
                evm.memory[dest_offset..dest_offset + size].copy_from_slice(&buffer);

                let words = size.div_ceil(32);
                gas = (3 + 3 * words).into();
                gas += evm.memory_expansion_cost();
            }
            0x5f => {
                // PUSH0
                evm.push(Word::zero())?;
                gas = 2.into();
            }

            0x60..=0x7f => {
                // PUSH1..PUSH32
                let arg = instruction
                    .argument
                    .as_ref()
                    .ok_or(ExecutorError::MissingData)?;
                evm.push(Word::from_bytes(arg))?;
                gas = 3.into();
            }

            0x80..=0x8f => {
                // DUP1..DUP16
                let n = instruction.opcode.n as usize;
                if evm.stack.len() < n {
                    evm.error(ExecutorError::StackUnderflow)?;
                }
                let val = evm.stack[evm.stack.len() - n];
                evm.push(val)?;
                gas = 3.into();
            }

            0x90..=0x9f => {
                // SWAP1..SWAP16
                let n = instruction.opcode.n as usize;
                if evm.stack.len() <= n {
                    evm.error(ExecutorError::StackUnderflow)?;
                }
                let stack_len = evm.stack.len();
                evm.stack.swap(stack_len - 1, stack_len - 1 - n);
                gas = 3.into();
            }

            0xa0..0xa4 => {
                // LOG0..LOG4
                if matches!(ctx.call_type, CallType::Static) {
                    return Err(ExecutorError::StaticCallViolation(opcode));
                }
                let n = instruction.opcode.n as usize;
                let offset = evm.pop()?.as_usize();
                let size = evm.pop()?.as_usize();

                let mut topics = Vec::with_capacity(n);
                for i in 0..n {
                    topics[n - 1 - i] = evm.pop()?;
                }

                if offset + size > evm.memory.len() {
                    evm.memory.resize(offset + size, 0);
                }
                let data = evm.memory[offset..offset + size].to_vec();
                evm.logs.push(Log(this, topics, data));

                gas = 375.into();
                gas += (375 * n + 8 * size).into();
                gas += evm.memory_expansion_cost();
            }

            0xf0 => {
                // CREATE
                if matches!(ctx.call_type, CallType::Static) {
                    return Err(ExecutorError::StaticCallViolation(opcode));
                }
                self.create(this, call, &mut gas, evm, ext, ctx).await?;
            }
            0xf1 => {
                // CALL
                if matches!(ctx.call_type, CallType::Static) {
                    let value = evm
                        .stack
                        .iter()
                        .rev()
                        .nth(3)
                        .ok_or(ExecutorError::StackUnderflow)?;
                    if !value.is_zero() {
                        return Err(ExecutorError::StaticCallViolation(opcode));
                    }
                }
                self.call(this, &mut gas, evm, ext, ctx).await?;
            }
            0xf2 => {
                // CALLCODE
                let ctx = Context {
                    call_type: CallType::Callcode,
                    ..ctx
                };
                // Creates a new sub context as if calling itself, but with the code of the given account.
                // In particular the storage [, the current sender and the current value] remain the same.
                // DELEGATECALL difference:  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
                self.call(this, &mut gas, evm, ext, ctx).await?;
            }
            0xf3 | 0xfd => {
                // RETURN | REVERT
                evm.stopped = true;
                evm.reverted = opcode == 0xfd;

                let offset = evm.pop()?.as_usize();
                let size = evm.pop()?.as_usize();

                if size > 0 {
                    if offset + size > evm.memory.len() {
                        evm.memory.resize(offset + size, 0);
                    }
                    self.ret = evm.memory[offset..offset + size].to_vec();
                } else {
                    self.ret.clear();
                }
                gas = evm.memory_expansion_cost();

                self.tracer.push(Event {
                    data: EventData::Return {
                        data: self.ret.clone().into(),
                        gas_used: evm.gas.used.as_u64(),
                    },
                    depth: ctx.depth,
                    reverted: evm.reverted,
                });
            }
            0xf4 => {
                // DELEGATECALL
                let ctx = Context {
                    call_type: CallType::Delegate,
                    ..ctx
                };
                // Creates a new sub context as if calling itself, but with the code of the given account.
                // In particular the storage, the current sender and the current value remain the same.
                self.call(this, &mut gas, evm, ext, ctx).await?;
            }
            0xf5 => {
                // CREATE2
                if matches!(ctx.call_type, CallType::Static) {
                    return Err(ExecutorError::StaticCallViolation(opcode));
                }
                let ctx = Context {
                    call_type: CallType::Create2,
                    ..ctx
                };
                self.create(this, call, &mut gas, evm, ext, ctx).await?;
            }
            0xfa => {
                // STATICCALL
                let ctx = Context {
                    call_type: CallType::Static,
                    ..ctx
                };
                self.call(this, &mut gas, evm, ext, ctx).await?;
            }
            0xfe => {
                // INVALID
                evm.gas.sub(evm.gas.remaining())?;
                evm.error(ExecutorError::InvalidOpcode(0xfe))?;
            }
            0xff => {
                // SELFDESTRUCT
                if matches!(ctx.call_type, CallType::Static) {
                    return Err(ExecutorError::StaticCallViolation(opcode));
                }
                todo!("SELFDESTRUCT");
            }
            _ => {
                return Err(ExecutorError::UnknownOpcode(opcode));
            }
        }

        if pc_increment {
            evm.pc += 1;
        }

        Ok(gas)
    }

    async fn call(
        &mut self,
        this: Address,
        gas: &mut Word,
        evm: &mut Evm,
        ext: &mut Ext,
        ctx: Context,
    ) -> eyre::Result<()> {
        let call_gas = evm.pop()?;
        let address = &evm.pop()?;
        let value = if !matches!(ctx.call_type, CallType::Static | CallType::Delegate) {
            evm.pop()?
        } else {
            Word::zero()
        };
        let args_offset = evm.pop()?.as_usize();
        let args_size = evm.pop()?.as_usize();
        let ret_offset = evm.pop()?.as_usize();
        let ret_size = evm.pop()?.as_usize();

        *gas = evm.memory_expansion_cost() + Word::from(21000);

        let bytecode = evm.code(ext, &address.into()).await?;
        let code = Decoder::decode(bytecode)?;

        let inner_call = Call {
            data: evm.memory[args_offset..args_offset + args_size].to_vec(),
            value,
            from: this,
            to: if matches!(ctx.call_type, CallType::Delegate | CallType::Callcode) {
                this
            } else {
                address.into()
            },
            gas: call_gas,
        };
        let mut inner_evm = Evm {
            gas: Gas::new(call_gas),
            ..Default::default()
        };
        let inner_ctx = Context {
            created: Address::zero(),
            origin: ctx.origin,
            depth: ctx.depth + 1,
            ..ctx
        };

        let executor = Executor::<T>::with_tracer(self.tracer.fork());
        let future =
            executor.execute_with_context(&code, &inner_call, &mut inner_evm, ext, inner_ctx);
        let (tracer, ret) = Box::pin(future).await?;
        self.tracer.join(tracer, inner_evm.reverted);

        if !inner_evm.reverted {
            if ret.len() != ret_size {
                evm.error(ExecutorError::WrongReturnDataSize {
                    exp: ret_size,
                    got: ret.len(),
                })?;
            }
            let size = ret_offset + ret_size;
            if size > evm.memory.len() {
                evm.memory.resize(size, 0);
                *gas += evm.memory_expansion_cost();
            }
            evm.memory[ret_offset..ret_offset + ret_size].copy_from_slice(&ret);
            self.ret = ret;
            for acc in inner_evm.account {
                evm.account.push(acc);
            }
            for sta in inner_evm.state {
                evm.state.push(sta);
            }
            *gas += evm.gas.used;
            evm.push(Word::one())?;
        } else {
            *gas += evm.gas.used;
            inner_evm.revert(ext).await?;
            evm.push(Word::zero())?;
        }

        Ok(())
    }

    async fn create(
        &mut self,
        this: Address,
        call: &Call,
        gas: &mut Word,
        evm: &mut Evm,
        ext: &mut Ext,
        ctx: Context,
    ) -> eyre::Result<()> {
        let value = evm.pop()?;
        let offset = evm.pop()?.as_usize();
        let size = evm.pop()?.as_usize();
        let salt = if matches!(ctx.call_type, CallType::Create2) {
            evm.pop()?
        } else {
            Word::zero()
        };

        if offset + size > evm.memory.len() {
            evm.memory.resize(offset + size, 0);
        }
        *gas = evm.memory_expansion_cost() + Word::from(32000);
        evm.gas(*gas)?;

        let bytecode = evm.memory[offset..offset + size].to_vec();
        let code = Decoder::decode(bytecode)?;

        let nonce = ext.acc_mut(&this).nonce;
        let address = if !matches!(ctx.call_type, CallType::Create2) {
            this.of_smart_contract(nonce)
        } else {
            // (See: https://www.evm.codes/?fork=cancun#f5)
            // initialisation_code = memory[offset:offset+size]
            // address = keccak256(0xff + sender_address + salt + keccak256(initialisation_code))[12:]
            let mut buffer = Vec::with_capacity(1 + 20 + 32 + 32);
            buffer.push(0xffu8);
            buffer.extend_from_slice(&call.from.0);
            buffer.extend_from_slice(&salt.into_bytes());
            buffer.extend_from_slice(&keccak256(&code.bytecode));
            let mut hash = keccak256(&buffer);
            hash[0..12].copy_from_slice(&[0u8; 12]);
            Address::from(&Word::from_bytes(&hash))
        };

        let inner_call = Call {
            data: vec![],
            value,
            from: this,
            to: Address::zero(),
            gas: evm.gas.remaining(),
        };
        let mut inner_evm = Evm {
            gas: Gas::new(evm.gas.remaining()),
            ..Default::default()
        };
        let inner_ctx = Context {
            created: address,
            origin: ctx.origin,
            depth: ctx.depth + 1,
            ..ctx
        };
        let executor = Executor::<T>::with_tracer(self.tracer.fork());
        let future =
            executor.execute_with_context(&code, &inner_call, &mut inner_evm, ext, inner_ctx);
        let (tracer, code) = Box::pin(future).await?;
        self.tracer.join(tracer, inner_evm.reverted);

        if !inner_evm.reverted {
            *gas += inner_evm.gas.used;

            let hash = keccak256(&code);
            *ext.code_mut(&address) = code.clone();
            ext.acc_mut(&address).code = Word::from_bytes(&hash);
            ext.acc_mut(&call.from).nonce += Word::one();
            evm.account.push(AccountTouch::Code(
                address,
                Word::from_bytes(&hash),
                code.clone(),
            ));
            evm.account.push(AccountTouch::Nonce(
                call.from,
                nonce.as_u64(),
                nonce.as_u64() + 1,
            ));
            self.tracer.push(Event {
                data: EventData::Account(AccountEvent::Deploy {
                    address,
                    code_hash: Word::from_bytes(&hash),
                    byte_code: code,
                }),
                depth: ctx.depth,
                reverted: false,
            });
            self.tracer.push(Event {
                data: EventData::Account(AccountEvent::Nonce {
                    address: call.from,
                    new: nonce.as_u64() + 1,
                }),
                depth: ctx.depth,
                reverted: false,
            });
            for acc in inner_evm.account {
                evm.account.push(acc);
            }
            for state in inner_evm.state {
                evm.state.push(state);
            }
            evm.push((&address).into())?;
        } else {
            *gas += evm.gas.used;
            inner_evm.revert(ext).await?;
            evm.push(Word::zero())?;
        }

        Ok(())
    }
}
