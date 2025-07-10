use i256::I256;
use primitive_types::U256;
use thiserror::Error;

use crate::{
    common::{address::Address, hash::sha3},
    decoder::{DecodedBytecode, Instruction},
};

#[derive(Error, Debug)]
pub enum InterpreterError {
    #[error("Stack overflow")]
    StackOverflow,
    #[error("Stack underflow")]
    StackUnderflow,
    #[error("Unsupported opcode: 0x{0:02x}:{1}")]
    UnsupportedOpcode(u8, String),
    #[error("Invalid jump")]
    InvalidJump,
    #[error("Missing data")]
    MissingData,
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
}

pub struct Interpreter<'a> {
    bytecode: &'a DecodedBytecode,
    state: EvmState,
    call: &'a Call,
    ret: Vec<u8>,
}

impl<'a> Interpreter<'a> {
    pub fn new(bytecode: &'a DecodedBytecode, call: &'a Call) -> Self {
        Self {
            bytecode,
            state: EvmState::default(),
            call,
            ret: Vec::new(),
        }
    }

    pub fn execute(&mut self, _call: &Call) -> Result<&[u8], InterpreterError> {
        // TODO: `call` should probably be used somehow here
        while !self.state.stopped && self.state.pc < self.bytecode.instructions.len() {
            let instruction = &self.bytecode.instructions[self.state.pc];

            let data = instruction
                .argument
                .as_ref()
                .map(|data| format!("0x{}", hex::encode(data)));
            println!(
                "\n0x{:04x}: {} {}",
                self.state.pc,
                instruction.opcode.name(),
                data.unwrap_or_default()
            );

            self.execute_instruction(instruction)?;

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
                    println!("0x{offset:04x}: {word}");
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
                .for_each(|word| println!("0x{word:04x}"));
        }

        Ok(&self.ret)
    }

    pub fn execute_instruction(
        &mut self,
        instruction: &Instruction,
    ) -> Result<(), InterpreterError> {
        let mut pc_increment = true;

        match instruction.opcode.code {
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
                let hash = U256::from_big_endian(&sha3(data));
                self.state.push(hash)?;
            }

            // 30-3f
            0x33 => {
                // CALLER
                self.state.push((&self.call.from).into())?;
            }
            0x34 => {
                // CALLVALUE
                self.state.push(self.call.value)?;
            }
            0x35 => {
                // CALLDATALOAD
                let offset = self.state.pop()?.as_usize();
                if offset > self.call.calldata.len() {
                    return Err(InterpreterError::MissingData);
                }
                let mut data = [0u8; 32];
                let copy = self.call.calldata.len().min(offset + 32) - offset;
                data[0..copy].copy_from_slice(&self.call.calldata[offset..offset + copy]);
                self.state.push(U256::from_big_endian(&data))?;
            }
            0x36 => {
                // CALLDATASIZE
                self.state.push(U256::from(self.call.calldata.len()))?;
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

                // unimplemented!("SLOAD(0x{key:0x})");
                eprintln!("SLOAD(0x{key:0x})");

                self.state.push(U256::zero())?;
            }
            0x55 => {
                // SSTORE
                let key = self.state.pop()?;
                let val = self.state.pop()?;

                // unimplemented!("SSTORE(0x{key:0x}, 0x{val:0x})");
                eprintln!("SSTORE(0x{key:0x}, 0x{val:0x})");
            }
            0x56 => {
                // JUMP
                let dest = self.state.pop()?.as_usize();
                let dest = self
                    .bytecode
                    .resolve_jump(dest)
                    .ok_or(InterpreterError::InvalidJump)?;
                if self.bytecode.instructions[dest].opcode.code != 0x5b {
                    return Err(InterpreterError::InvalidJump);
                }
                self.state.pc = dest;
                pc_increment = false;
            }
            0x57 => {
                // JUMPI
                let dest = self.state.pop()?.as_usize();
                let dest = self
                    .bytecode
                    .resolve_jump(dest)
                    .ok_or(InterpreterError::InvalidJump)?;
                let cond = self.state.pop()?;
                if !cond.is_zero() {
                    if self.bytecode.instructions[dest].opcode.code != 0x5b {
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

            // f3: RETURN
            0xf3 => {
                // TODO: return the data
                self.state.stopped = true;
            }

            // fd: REVERT
            0xfd => {
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

            _ => {
                return Err(InterpreterError::UnsupportedOpcode(
                    instruction.opcode.code,
                    instruction.opcode.name.to_string(),
                ));
            }
        }

        if pc_increment {
            self.state.pc += 1;
        }

        Ok(())
    }
}
