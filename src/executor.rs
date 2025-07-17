use std::collections::HashSet;

use i256::I256;
use thiserror::Error;

use crate::{
    common::{Word, address::Address, call::Call, hash::keccak256},
    decoder::{Bytecode, Decoder, DecoderError, Instruction},
    ext::Ext,
    tracer::{CallType, Event, EventData, EventTracer, StateEvent},
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
    #[error("Call run out of gas")]
    OutOfGas(),
    #[error("Insufficient funds: have {have}, need {need}")]
    InsufficientFunds { have: Word, need: Word },
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

#[derive(Debug, Default)]
pub enum AccountTouch {
    #[default]
    Empty,
    Nonce(Address, u64, u64),
    Value(Address, Word, Word),
    Code(Address, Word, Vec<u8>),
}

#[derive(Debug, Default)]
pub struct Evm {
    pub memory: Vec<u8>,
    pub stack: Vec<Word>,
    pub gas: Gas,
    pub pc: usize,
    pub stopped: bool,
    pub reverted: bool,

    pub mem_cost: Word,
    pub state: Vec<StateTouch>,
    pub account: Vec<AccountTouch>,
    pub dirty_state: HashSet<(Address, Word)>,
}

impl Evm {
    pub(crate) fn mem_exp_cost(&mut self) -> Word {
        let memory_byte_size = self.memory.len();
        let memory_size_word = memory_byte_size.div_ceil(32);
        let mem_cost = (memory_size_word * memory_size_word) / 512 + (3 * memory_size_word);
        let exp_cost = Word::from(mem_cost) - self.mem_cost;
        self.mem_cost = exp_cost;
        exp_cost
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
                    if let Some(acc) = ext.acc_mut(addr) {
                        acc.nonce = (*val).into();
                    }
                }
                AccountTouch::Value(addr, val, _new) => {
                    if let Some(acc) = ext.acc_mut(addr) {
                        acc.balance = *val;
                    }
                }
                AccountTouch::Code(addr, _hash, _code) => {
                    if let Some(acc) = ext.acc_mut(addr) {
                        acc.code_hash = Word::zero();
                    }
                    if let Some(code) = ext.code_mut(addr) {
                        code.clear();
                    }
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

#[derive(Default)]
pub struct Executor<T: EventTracer> {
    tracer: T,
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
        evm: &mut Evm,
        ext: &mut Ext,
    ) -> Result<(T, Vec<u8>), ExecutorError> {
        self.tracer.add(Event {
            reverted: false,
            depth: 0,
            data: EventData::Call(call.clone(), CallType::Call),
        });
        evm.gas = Gas::new(call.gas);

        let call_cost = 21000;
        evm.gas.sub(call_cost.into())?;

        let data_cost = {
            let total_calldata_len = code.bytecode.len();
            let nonzero_bytes_count = code.bytecode.iter().filter(|byte| byte == &&0).count();
            nonzero_bytes_count * 16 + (total_calldata_len - nonzero_bytes_count) * 4
        };
        evm.gas.sub(data_cost.into())?;

        if code.bytecode.is_empty() {
            // TODO: check hash & signature first
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
            let gas_price = Word::one();
            let gas_fee = evm.gas.used * gas_price;

            if src < call.value + gas_fee {
                return Err(ExecutorError::InsufficientFunds {
                    have: src,
                    need: call.value + gas_fee,
                });
            }

            if let Some(acc) = ext.acc_mut(&call.from) {
                acc.balance -= call.value + gas_fee;
            }
            evm.account.push(AccountTouch::Value(
                call.from,
                src,
                src - call.value - gas_fee,
            ));

            if let Some(acc) = ext.acc_mut(&call.to) {
                acc.balance += call.value;
            }
            evm.account
                .push(AccountTouch::Value(call.to, dst, dst + call.value));

            evm.stopped = true;
            evm.reverted = false;
            Ok((self.tracer, vec![]))
        } else {
            self.execute_with_depth(code, call, evm, ext, 0).await
        }
    }

    async fn execute_with_depth(
        mut self,
        code: &Bytecode,
        call: &Call,
        evm: &mut Evm,
        ext: &mut Ext,
        depth: usize,
    ) -> Result<(T, Vec<u8>), ExecutorError> {
        if depth > CALL_DEPTH_LIMIT {
            return Err(ExecutorError::CallDepthLimitReached);
        }

        while !evm.stopped && evm.pc < code.instructions.len() {
            let instruction = &code.instructions[evm.pc];

            if self.log {
                let data = instruction
                    .argument
                    .as_ref()
                    .map(|data| format!("0x{}", hex::encode(data)));
                println!(
                    "\n{:#04x}: {} {}",
                    evm.pc,
                    instruction.opcode.name(),
                    data.unwrap_or_default()
                );
            }

            let cost = self
                .execute_instruction(code, call, evm, ext, depth, instruction)
                .await?;
            self.tracer.add(Event {
                depth,
                reverted: false,
                data: EventData::Opcode {
                    pc: evm.pc,
                    op: instruction.opcode.code,
                    name: instruction.opcode.name(),
                    data: instruction.argument.clone(),
                    gas: cost,
                },
            });
            evm.gas(cost)?;

            if self.log {
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
            }
        }

        Ok((self.tracer, self.ret))
    }

    pub async fn execute_instruction(
        &mut self,
        code: &Bytecode,
        call: &Call,
        evm: &mut Evm,
        ext: &mut Ext,
        depth: usize,
        instruction: &Instruction,
    ) -> Result<Word, ExecutorError> {
        let mut gas = Word::zero();
        let mut pc_increment = true;

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
                let a_signed = I256::from_be_bytes(a.to_big_endian());
                let b_signed = I256::from_be_bytes(b.to_big_endian());
                let res = if b.is_zero() {
                    I256::from(0)
                } else if a_signed == I256::MIN && b_signed == I256::from(-1) {
                    I256::MIN
                } else {
                    a_signed / b_signed
                };
                evm.push(Word::from_big_endian(&res.to_be_bytes()))?;
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
                let a_signed = I256::from_be_bytes(a.to_big_endian());
                let b_signed = I256::from_be_bytes(b.to_big_endian());
                let res = if b.is_zero() {
                    I256::from(0)
                } else {
                    a_signed % b_signed
                };
                evm.push(Word::from_big_endian(&res.to_be_bytes()))?;
                gas = 5.into();
            }
            0x08 => {
                // ADDMOD
                // gas = 8.into();
                todo!();
            }
            0x09 => {
                // MULMOD
                // gas = 8.into();
                todo!();
            }
            0x0a => {
                // EXP
                let base = evm.pop()?;
                let exponent = evm.pop()?;
                evm.push(base.pow(exponent))?;

                let exp_bytes = exponent
                    .to_big_endian()
                    .into_iter()
                    .skip_while(|byte| byte == &0)
                    .count();
                gas = (10 + exp_bytes * 50).into();
            }
            0x0b => {
                // SIGNEXTEND
                // gas = 5.into();
                todo!()
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
                let a_signed = I256::from_be_bytes(a.to_big_endian());
                let b_signed = I256::from_be_bytes(b.to_big_endian());
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
                let a_signed = I256::from_be_bytes(a.to_big_endian());
                let b_signed = I256::from_be_bytes(b.to_big_endian());
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
                    evm.push(Word::from(value.byte(byte_index)))?;
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
                let value = I256::from_be_bytes(value.to_big_endian());
                let ret = value >> shift;
                let ret = Word::from_big_endian(&ret.to_be_bytes());
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
                let hash = Word::from_big_endian(&keccak256(data));
                evm.push(hash)?;
                gas = (30 + 6 * size.div_ceil(32)).into();
            }

            // 30-3f
            0x30 => {
                // ADDRESS
                evm.push((&call.to).into())?;
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
                evm.push((&call.origin).into())?;
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
                if offset > call.calldata.len() {
                    evm.error(ExecutorError::MissingData)?;
                }
                let mut data = [0u8; 32];
                let copy = call.calldata.len().min(offset + 32) - offset;
                data[0..copy].copy_from_slice(&call.calldata[offset..offset + copy]);
                evm.push(Word::from_big_endian(&data))?;
                gas = 3.into();
            }
            0x36 => {
                // CALLDATASIZE
                evm.push(Word::from(call.calldata.len()))?;
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
                    .copy_from_slice(&call.calldata[offset..offset + size]);
                gas = (3 + 3 * size.div_ceil(32)).into();
                gas += evm.mem_exp_cost();
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
                gas += evm.mem_exp_cost();
            }
            0x3a => {
                // GASPRICE
                todo!("GASPRICE")
            }
            0x3b => {
                // EXTCODESIZE
                todo!("EXTCODESIZE")
            }
            0x3c => {
                // EXTCODECOPY
                todo!("EXTCODECOPY")
            }
            0x3d => {
                // RETURNDATASIZE
                todo!("RETURNDATASIZE")
            }
            0x3e => {
                // RETURNDATACOPY
                todo!("RETURNDATACOPY")
            }
            0x3f => {
                // EXTCODEHASH
                todo!("EXTCODEHASH")
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
                let value = Word::from_big_endian(&evm.memory[offset..end]);
                evm.push(value)?;
                gas = 3.into();
                gas += evm.mem_exp_cost();
            }
            0x52 => {
                // MSTORE
                let offset = evm.pop()?.as_usize();
                let value = evm.pop()?;
                let end = offset + 32;
                if end > evm.memory.len() {
                    evm.memory.resize(end, 0);
                }
                let bytes = &value.to_big_endian();
                evm.memory[offset..end].copy_from_slice(bytes);
                gas = 3.into();
                gas += evm.mem_exp_cost();
            }
            0x53 => {
                // MSTORE8
                let offset = evm.pop()?.as_usize();
                let value = evm.pop()?;
                if offset >= evm.memory.len() {
                    evm.memory.resize(offset + 1, 0);
                }
                evm.memory[offset] = value.to_little_endian()[0];
                gas = 3.into();
                gas += evm.mem_exp_cost();
            }
            0x54 => {
                // SLOAD
                let key = evm.pop()?;
                let is_warm = ext
                    .state
                    .get(&call.to)
                    .map(|s| s.data.contains_key(&key))
                    .unwrap_or_default();
                let val = evm.get(ext, &call.to, &key).await?;
                evm.push(val)?;
                evm.state
                    .push(StateTouch(call.to, key, val, None, Word::zero()));
                self.tracer.add(Event {
                    data: EventData::State(StateEvent::R(call.to, key, val)),
                    depth,
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
                let key = evm.pop()?;

                let _is_warm = ext
                    .state
                    .get(&call.to)
                    .map(|s| s.data.contains_key(&key))
                    .unwrap_or_default();
                let _is_dirty = evm.dirty_state.contains(&(call.to, key));

                let val = evm.get(ext, &call.to, &key).await?;
                let new = evm.pop()?;
                evm.put(ext, &call.to, key, val).await?;

                self.tracer.add(Event {
                    data: EventData::State(StateEvent::W(call.to, key, val, new)),
                    depth,
                    reverted: false,
                });

                // TODO: gas calculation is tricky
                let gas_cost = Word::zero();
                evm.gas.sub(gas_cost)?;

                // TODO: gas refund calculation is even trickier!
                // See: https://www.evm.codes/?fork=cancun#55
                let gas_refund = Word::zero();
                evm.state
                    .push(StateTouch(call.to, key, val, Some(new), gas_refund));
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
                gas += evm.mem_exp_cost();
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
                evm.push(Word::from_big_endian(arg))?;
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
                let n = instruction.opcode.n as usize;
                let offset = evm.pop()?.as_usize();
                let size = evm.pop()?.as_usize();

                let mut topics = Vec::with_capacity(n);
                for i in 0..n {
                    topics[n - 1 - i] = evm.pop()?;
                }

                let _data = evm.memory[offset..offset + size].to_vec();
                // TODO: store logs

                gas = 375.into();
                gas += (375 * n + 8 * size).into();
                gas += evm.mem_exp_cost();
            }

            #[allow(unused_variables)]
            0xf0 => {
                // CREATE
                let value = evm.pop()?;
                let offset = evm.pop()?;
                let size = evm.pop()?;

                todo!("CREATE");
                // put address of the created contract on the stack
            }
            #[allow(unused_variables)]
            0xf1 => {
                // CALL
                let gas = evm.pop()?;
                let address = &evm.pop()?;
                let value = evm.pop()?;
                let args_offset = evm.pop()?.as_usize();
                let args_size = evm.pop()?.as_usize();
                let ret_offset = evm.pop()?.as_usize();
                let ret_size = evm.pop()?.as_usize();

                let bytecode = evm.code(ext, &address.into()).await?;
                let code = Decoder::decode(bytecode)?;

                let inner_call = Call {
                    calldata: evm.memory[args_offset..args_offset + args_size].to_vec(),
                    value,
                    from: call.to,
                    origin: call.origin,
                    to: address.into(),
                    gas,
                };
                let mut inner_evm = Evm::default();

                let executor = Executor::<T>::with_tracer(self.tracer.fork());
                let future =
                    executor.execute_with_depth(&code, &inner_call, &mut inner_evm, ext, depth + 1);
                let (tracer, ret) = Box::pin(future).await?;
                self.tracer.join(tracer, inner_evm.reverted);

                if !inner_evm.reverted {
                    if ret.len() == ret_size {
                        let size = ret_offset + ret_size;
                        if size > evm.memory.len() {
                            evm.memory.resize(size, 0);
                            let gas = evm.mem_exp_cost();
                            evm.gas.sub(gas)?;
                        }
                        evm.memory[ret_offset..ret_offset + ret_size].copy_from_slice(&ret);
                        evm.push(Word::one())?;
                    } else {
                        todo!("CALL: ret size mismatch data len")
                    }
                } else {
                    inner_evm.revert(ext).await?;
                    evm.push(Word::zero())?;
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
                evm.stopped = true;
                evm.reverted = opcode == 0xfd;

                let offset = evm.pop()?.as_usize();
                let size = evm.pop()?.as_usize();

                if size > 0 {
                    if offset > evm.memory.len() || offset + size > evm.memory.len() {
                        evm.stopped = true;
                        evm.reverted = true;
                        return Err(ExecutorError::MissingData);
                    }
                    self.ret = evm.memory[offset..offset + size].to_vec();
                } else {
                    self.ret.clear();
                }
            }
            #[allow(unused_variables)]
            0xf4 => {
                // DELEGATECALL
                let gas = evm.pop()?;
                let address = evm.pop()?;
                let args_offset = evm.pop()?;
                let args_size = evm.pop()?;
                let ret_offset = evm.pop()?;
                let ret_size = evm.pop()?;

                todo!("DELEGATECALL");
            }
            #[allow(unused_variables)]
            0xf5 => {
                // CREATE2
                let value = evm.pop()?;
                let offset = evm.pop()?;
                let size = evm.pop()?;
                let salt = evm.pop()?;

                // gas = 32000.into();

                todo!("CREATE2");
                // put address of the created contract on the stack
            }
            #[allow(unused_variables)]
            0xfa => {
                // STATICCALL
                let gas = evm.pop()?;
                let address = evm.pop()?;
                let args_offset = evm.pop()?;
                let args_size = evm.pop()?;
                let ret_offset = evm.pop()?;
                let ret_size = evm.pop()?;

                todo!("STATICCALL");
            }
            0xfe => {
                // INVALID
                evm.gas.sub(evm.gas.remaining())?;
                evm.error(ExecutorError::InvalidOpcode(0xfe))?;
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
            evm.pc += 1;
        }

        Ok(gas)
    }
}
