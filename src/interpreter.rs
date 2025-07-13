use std::{collections::HashMap, time::Instant};

use i256::I256;
use primitive_types::U256;
use thiserror::Error;

use crate::{
    common::{address::Address, hash::keccak256},
    decoder::{Bytecode, Decoder, DecoderError, Instruction},
    eth::EthClient,
};

#[derive(Error, Debug)]
pub enum InterpreterError {
    #[error("Stack overflow")]
    StackOverflow,
    #[error("Stack underflow")]
    StackUnderflow,
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

#[derive(Debug, Default)]
pub struct EvmState {
    pub stack: Vec<U256>,
    pub memory: Vec<u8>,
    pub pc: usize,
    pub stopped: bool,
}

impl EvmState {
    pub fn push(&mut self, value: U256) -> Result<(), InterpreterError> {
        if self.stack.len() >= STACK_LIMIT {
            return Err(InterpreterError::StackOverflow);
        }
        self.stack.push(value);
        Ok(())
    }

    pub fn pop(&mut self) -> Result<U256, InterpreterError> {
        self.stack.pop().ok_or(InterpreterError::StackUnderflow)
    }
}

#[derive(Debug)]
pub struct Call {
    pub calldata: Vec<u8>,
    pub value: U256,
    pub from: Address,
    pub to: Address,
}

#[derive(Default)]
pub struct State {
    r: Vec<(U256, U256)>,
    w: Vec<(U256, U256)>,
}

impl State {
    fn get(&self, key: &U256) -> Option<U256> {
        self.r
            .iter()
            .filter(|(k, _)| k == key)
            .map(|(_, v)| v)
            .next()
            .cloned()
    }

    fn hit(&mut self, key: U256, val: U256) {
        self.r.push((key, val));
    }

    fn put(&mut self, key: U256, val: U256) {
        if let Some((_, v)) = self.w.iter_mut().find(|(k, _)| k == &key) {
            *v = val;
        } else {
            self.w.push((key, val));
        }
    }
}

pub struct Ext {
    block_hash: String,
    state: HashMap<Address, State>,
    eth: EthClient,
}

pub struct Account {
    pub balance: U256,
    pub nonce: U256,
    pub code: U256,
    pub root: U256,
}

impl Ext {
    pub fn new(block_hash: String, eth: EthClient) -> Self {
        Self {
            block_hash,
            state: Default::default(),
            eth,
        }
    }

    fn hit(&mut self, addr: &Address, key: U256, val: U256) {
        let e = self.state.entry(addr.clone()).or_default();
        e.hit(key, val);
    }

    pub async fn get(&mut self, addr: &Address, key: &U256) -> eyre::Result<U256> {
        let val = if let Some(val) = self.state.get(addr).and_then(|s| s.get(key)) {
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

        self.hit(addr, *key, val);
        Ok(val)
    }

    pub async fn put(&mut self, addr: &Address, key: U256, val: U256) {
        let state = self.state.entry(addr.clone()).or_default();
        state.put(key, val);
    }

    pub async fn acc(&mut self, _addr: &Address) -> eyre::Result<Account> {
        todo!("eth_getAccount");
    }

    pub async fn code(&mut self, addr: &Address) -> eyre::Result<Vec<u8>> {
        let address = format!("0x{}", hex::encode(addr.0));
        self.eth.get_code(&self.block_hash, &address).await
    }
}

#[derive(Default)]
pub struct Interpreter {
    state: EvmState,
    ret: Vec<u8>,
}

impl Interpreter {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn execute(
        &mut self,
        code: &Bytecode,
        call: &Call,
        ext: &mut Ext,
    ) -> Result<&[u8], InterpreterError> {
        while !self.state.stopped && self.state.pc < code.instructions.len() {
            let instruction = &code.instructions[self.state.pc];

            let data = instruction
                .argument
                .as_ref()
                .map(|data| format!("0x{}", hex::encode(data)));
            println!(
                "\n{:#04x}: {} {}",
                self.state.pc,
                instruction.opcode.name(),
                data.unwrap_or_default()
            );

            self.execute_instruction(code, call, ext, instruction)
                .await?;

            println!(
                "MEMORY:{}",
                if self.state.memory.is_empty() {
                    " []"
                } else {
                    ""
                }
            );
            self.state
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
                if self.state.stack.is_empty() {
                    " []"
                } else {
                    ""
                }
            );
            self.state
                .stack
                .iter()
                .rev()
                .for_each(|word| println!("{word:#02x}"));
        }

        Ok(&self.ret)
    }

    pub async fn execute_instruction(
        &mut self,
        code: &Bytecode,
        call: &Call,
        ext: &mut Ext,
        instruction: &Instruction,
    ) -> Result<(), InterpreterError> {
        let mut pc_increment = true;

        let opcode = instruction.opcode.code;
        match opcode {
            // 0x00: STOP
            0x00 => {
                self.state.stopped = true;
            }
            // 0x01..0x0b: Arithmetic Operations
            0x01 => {
                // ADD
                let a = self.state.pop()?;
                let b = self.state.pop()?;
                let res = a.saturating_add(b);
                self.state.push(res)?;
            }
            0x02 => {
                // MUL
                let a = self.state.pop()?;
                let b = self.state.pop()?;
                let res = a.saturating_mul(b);
                self.state.push(res)?;
            }
            0x03 => {
                // SUB
                let a = self.state.pop()?;
                let b = self.state.pop()?;
                let res = a.saturating_sub(b);
                self.state.push(res)?;
            }
            0x04 => {
                // DIV
                let a = self.state.pop()?;
                let b = self.state.pop()?;
                if b.is_zero() {
                    self.state.push(U256::zero())?;
                } else {
                    self.state.push(a / b)?;
                }
            }
            0x05 => {
                // SDIV
                let a = self.state.pop()?;
                let b = self.state.pop()?;
                let a_signed = I256::from_be_bytes(a.to_big_endian());
                let b_signed = I256::from_be_bytes(b.to_big_endian());
                let res = if b.is_zero() {
                    I256::from(0)
                } else if a_signed == I256::MIN && b_signed == I256::from(-1) {
                    I256::MIN
                } else {
                    a_signed / b_signed
                };
                self.state.push(U256::from_big_endian(&res.to_be_bytes()))?;
            }
            0x06 => {
                // MOD
                let a = self.state.pop()?;
                let b = self.state.pop()?;
                if b.is_zero() {
                    self.state.push(U256::zero())?;
                } else {
                    self.state.push(a % b)?;
                }
            }
            0x07 => {
                // SMOD
                let a = self.state.pop()?;
                let b = self.state.pop()?;
                let a_signed = I256::from_be_bytes(a.to_big_endian());
                let b_signed = I256::from_be_bytes(b.to_big_endian());
                let res = if b.is_zero() {
                    I256::from(0)
                } else {
                    a_signed % b_signed
                };
                self.state.push(U256::from_big_endian(&res.to_be_bytes()))?;
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
                let base = self.state.pop()?;
                let exponent = self.state.pop()?;
                self.state.push(base.pow(exponent))?;
            }
            0x0b => {
                // SIGNEXTEND
                todo!()
            }

            // 0x10s: Comparison & Bitwise Logic
            0x10 => {
                // LT
                let a = self.state.pop()?;
                let b = self.state.pop()?;
                self.state
                    .push(if a < b { U256::one() } else { U256::zero() })?;
            }
            0x11 => {
                // GT
                let a = self.state.pop()?;
                let b = self.state.pop()?;
                self.state
                    .push(if a > b { U256::one() } else { U256::zero() })?;
            }
            0x12 => {
                // SLT
                todo!()
            }
            0x13 => {
                // SGT
                todo!()
            }
            0x14 => {
                // EQ
                let a = self.state.pop()?;
                let b = self.state.pop()?;
                self.state
                    .push(if a == b { U256::one() } else { U256::zero() })?;
            }
            0x15 => {
                // ISZERO
                let a = self.state.pop()?;
                self.state.push(if a.is_zero() {
                    U256::one()
                } else {
                    U256::zero()
                })?;
            }
            0x16 => {
                // AND
                let a = self.state.pop()?;
                let b = self.state.pop()?;
                self.state.push(a & b)?;
            }
            0x17 => {
                // OR
                let a = self.state.pop()?;
                let b = self.state.pop()?;
                self.state.push(a | b)?;
            }
            0x18 => {
                // XOR
                let a = self.state.pop()?;
                let b = self.state.pop()?;
                self.state.push(a ^ b)?;
            }
            0x19 => {
                // NOT
                let a = self.state.pop()?;
                self.state.push(!a)?;
            }
            0x1a => {
                // BYTE
                let i = self.state.pop()?;
                let x = self.state.pop()?;
                let mut ret = U256::zero();
                if i < U256::from(32) {
                    let byte_index = 31 - i.as_usize();
                    ret = U256::from(x.byte(byte_index));
                }
                self.state.push(ret)?;
            }
            0x1b => {
                // SHL
                todo!()
            }
            0x1c => {
                // SLT
                todo!()
            }
            0x1d => {
                // SAR
                todo!()
            }

            0x20 => {
                // SHA3 (KECCAK256)
                let offset = self.state.pop()?.as_usize();
                let size = self.state.pop()?.as_usize();

                if offset + size > self.state.memory.len() {
                    return Err(InterpreterError::MissingData);
                }
                let data = &self.state.memory[offset..offset + size];
                let hash = U256::from_big_endian(&keccak256(data));
                self.state.push(hash)?;
            }

            // 30-3f
            0x33 => {
                // CALLER
                self.state.push((&call.from).into())?;
            }
            0x34 => {
                // CALLVALUE
                self.state.push(call.value)?;
            }
            0x35 => {
                // CALLDATALOAD
                let offset = self.state.pop()?.as_usize();
                if offset > call.calldata.len() {
                    return Err(InterpreterError::MissingData);
                }
                let mut data = [0u8; 32];
                let copy = call.calldata.len().min(offset + 32) - offset;
                data[0..copy].copy_from_slice(&call.calldata[offset..offset + copy]);
                self.state.push(U256::from_big_endian(&data))?;
            }
            0x36 => {
                // CALLDATASIZE
                self.state.push(U256::from(call.calldata.len()))?;
            }

            // 40-4a

            // 0x50s: Stack, Memory, Storage and Flow Operations
            0x50 => {
                // POP
                self.state.pop()?;
            }
            0x51 => {
                // MLOAD
                let offset = self.state.pop()?.as_usize();
                let end = offset + 32;
                if end > self.state.memory.len() {
                    self.state.memory.resize(end, 0);
                }
                let value = U256::from_big_endian(&self.state.memory[offset..end]);
                self.state.push(value)?;
            }
            0x52 => {
                // MSTORE
                let offset = self.state.pop()?.as_usize();
                let value = self.state.pop()?;
                let end = offset + 32;
                if end > self.state.memory.len() {
                    self.state.memory.resize(end, 0);
                }
                let bytes = &value.to_big_endian();
                self.state.memory[offset..end].copy_from_slice(bytes);
            }
            0x53 => {
                // MSTORE8
                let offset = self.state.pop()?.as_usize();
                let value = self.state.pop()?;
                if offset >= self.state.memory.len() {
                    self.state.memory.resize(offset + 1, 0);
                }
                self.state.memory[offset] = value.to_little_endian()[0];
            }
            0x54 => {
                // SLOAD
                let key = self.state.pop()?;
                let val = ext.get(&call.from, &key).await?;
                self.state.push(val)?;
            }
            0x55 => {
                // SSTORE
                let key = self.state.pop()?;
                let val = self.state.pop()?;
                ext.put(&call.from, key, val).await;
            }
            0x56 => {
                // JUMP
                let dest = self.state.pop()?.as_usize();
                let dest = code
                    .resolve_jump(dest)
                    .ok_or(InterpreterError::InvalidJump)?;
                if code.instructions[dest].opcode.code != 0x5b {
                    return Err(InterpreterError::InvalidJump);
                }
                self.state.pc = dest;
                pc_increment = false;
            }
            0x57 => {
                // JUMPI
                let dest = self.state.pop()?.as_usize();
                let dest = code
                    .resolve_jump(dest)
                    .ok_or(InterpreterError::InvalidJump)?;
                let cond = self.state.pop()?;
                if !cond.is_zero() {
                    if code.instructions[dest].opcode.code != 0x5b {
                        return Err(InterpreterError::InvalidJump);
                    }
                    self.state.pc = dest;
                    pc_increment = false;
                }
            }
            0x58 => {
                // PC
                self.state.push(U256::from(instruction.offset))?;
            }
            0x59 => {
                // MSIZE
                self.state.push(U256::from(self.state.memory.len()))?;
            }
            0x5b => {
                // JUMPDEST: noop, a valid destination for JUMP/JUMPI
            }

            0x5f => {
                // PUSH0
                self.state.push(U256::zero())?;
            }
            // 0x60..=0x7f: PUSH1 to PUSH32
            0x60..=0x7f => {
                let arg = instruction
                    .argument
                    .as_ref()
                    .ok_or(InterpreterError::MissingData)?;
                self.state.push(U256::from_big_endian(arg))?;
            }

            // 0x80..=0x8f: DUP1 to DUP16
            0x80..=0x8f => {
                let n = instruction.opcode.n as usize;
                if self.state.stack.len() < n {
                    return Err(InterpreterError::StackUnderflow);
                }
                let val = self.state.stack[self.state.stack.len() - n];
                self.state.push(val)?;
            }

            // 0x90..=0x9f: SWAP1 to SWAP16
            0x90..=0x9f => {
                let n = instruction.opcode.n as usize;
                if self.state.stack.len() <= n {
                    return Err(InterpreterError::StackUnderflow);
                }
                let stack_len = self.state.stack.len();
                self.state.stack.swap(stack_len - 1, stack_len - 1 - n);
            }

            #[allow(unused_variables)]
            0xf0 => {
                // CREATE
                let value = self.state.pop()?;
                let offset = self.state.pop()?;
                let size = self.state.pop()?;

                todo!("CREATE");
                // put address of the created contract on the stack
            }
            #[allow(unused_variables)]
            0xf1 => {
                // CALL
                let gas = self.state.pop()?;
                let address = &self.state.pop()?;
                let value = self.state.pop()?;
                let args_offset = self.state.pop()?.as_usize();
                let args_size = self.state.pop()?.as_usize();
                let ret_offset = self.state.pop()?.as_usize();
                let ret_size = self.state.pop()?.as_usize();

                let code = ext.code(&address.into()).await?;
                let code = Decoder::decode(&code)?;
                let mut interpreter = Interpreter::new();

                let nested_call = Call {
                    calldata: self.state.memory[args_offset..args_offset + args_size].to_vec(),
                    value,
                    from: call.to.clone(),
                    to: address.into(),
                };

                let f = interpreter.execute(&code, &nested_call, ext);
                let ret = Box::pin(f).await;
                match ret {
                    Ok(ret) => {
                        if ret.len() == ret_size {
                            let size = ret_offset + ret_size;
                            if size > self.state.memory.len() {
                                self.state.memory.resize(size, 0);
                            }
                            self.state.memory[ret_offset..ret_offset + ret_size]
                                .copy_from_slice(ret);
                            self.state.push(U256::zero())?;
                        } else {
                            return Err(InterpreterError::WrongCallRetDataSize {
                                exp: ret_size,
                                got: ret.len(),
                            });
                        }
                    }
                    Err(e) => {
                        todo!("CALL failed: {e}");
                    }
                }
            }
            #[allow(unused_variables)]
            0xf2 => {
                // CALLCODE
                let gas = self.state.pop()?;
                let address = self.state.pop()?;
                let value = self.state.pop()?;
                let args_offset = self.state.pop()?;
                let args_size = self.state.pop()?;
                let ret_offset = self.state.pop()?;
                let ret_size = self.state.pop()?;

                todo!("CALLCODE");
            }
            0xf3 | 0xfd => {
                // REVERT | RETURN
                self.state.stopped = true;

                let offset = self.state.pop()?.as_usize();
                let size = self.state.pop()?.as_usize();

                if offset > self.state.memory.len() || offset + size > self.state.memory.len() {
                    return Err(InterpreterError::MissingData);
                }
                if size > 0 {
                    self.ret = self.state.memory[offset..offset + size].to_vec();
                } else {
                    self.ret.clear();
                }
            }
            #[allow(unused_variables)]
            0xf4 => {
                // DELEGATECALL
                let gas = self.state.pop()?;
                let address = self.state.pop()?;
                let args_offset = self.state.pop()?;
                let args_size = self.state.pop()?;
                let ret_offset = self.state.pop()?;
                let ret_size = self.state.pop()?;

                todo!("DELEGATECALL");
            }
            #[allow(unused_variables)]
            0xf5 => {
                // CREATE2
                let value = self.state.pop()?;
                let offset = self.state.pop()?;
                let size = self.state.pop()?;
                let salt = self.state.pop()?;

                todo!("CREATE2");
                // put address of the created contract on the stack
            }
            #[allow(unused_variables)]
            0xfa => {
                // STATICCALL
                let gas = self.state.pop()?;
                let address = self.state.pop()?;
                let args_offset = self.state.pop()?;
                let args_size = self.state.pop()?;
                let ret_offset = self.state.pop()?;
                let ret_size = self.state.pop()?;

                todo!("STATICCALL");
            }
            0xfe => {
                // INVALID
                return Err(InterpreterError::InvalidOpcode(opcode));
            }
            0xff => {
                // SELFDESTRUCT
                todo!("SELFDESTRUCT");
            }
            _ => {
                return Err(InterpreterError::UnknownOpcode(opcode));
            }
        }

        if pc_increment {
            self.state.pc += 1;
        }

        Ok(())
    }
}
