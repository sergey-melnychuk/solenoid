use thiserror::Error;

use crate::opcodes::{Opcode, get_opcode};

#[derive(Error, Debug)]
pub enum DecoderError {
    #[error("Invalid opcode 0x{0:02x} found at position {1}")]
    InvalidOpcode(u8, usize),
    #[error("Unexpected end of bytecode at offset {1}:{0}")]
    BufferUnderflow(String, usize),
}

#[derive(Debug)]
pub struct Instruction {
    pub opcode: Opcode,
    pub offset: usize,
    pub argument: Option<Vec<u8>>,
}

#[derive(Debug)]
pub struct Bytecode {
    pub bytecode: Vec<u8>,
    pub instructions: Vec<Instruction>,
    pub jumptable: Vec<(usize, usize)>,
}

impl Bytecode {
    pub fn new(
        bytecode: Vec<u8>,
        instructions: Vec<Instruction>,
        jumptable: Vec<(usize, usize)>,
    ) -> Self {
        Self {
            bytecode,
            instructions,
            jumptable,
        }
    }

    pub fn resolve_jump(&self, offset: usize) -> Option<usize> {
        let index = self
            .jumptable
            .binary_search_by_key(&offset, |(key, _)| *key)
            .ok()?;
        Some(self.jumptable[index].1)
    }
}

pub struct Decoder;

impl Decoder {
    pub fn decode(bytecode: Vec<u8>) -> Result<Bytecode, DecoderError> {
        let mut instructions = Vec::new();
        let mut jumptable = Vec::new();

        let mut pos = 0;
        while pos < bytecode.len() {
            let opcode = get_opcode(bytecode[pos]);
            let mut instruction = Instruction {
                opcode,
                offset: pos,
                argument: None,
            };

            // JUMPDEST
            if opcode.code == 0x5b {
                jumptable.push((pos, instructions.len()));
            }

            pos += 1;

            let len = opcode.push_len();
            if len > 0 {
                let from = pos;
                let till = pos + len;

                if till > bytecode.len() {
                    return Err(DecoderError::BufferUnderflow(opcode.name(), pos));
                }

                instruction.argument = Some(bytecode[from..till].to_vec());
                pos = till;
            }

            instructions.push(instruction);
        }

        Ok(Bytecode {
            bytecode,
            instructions,
            jumptable,
        })
    }
}
