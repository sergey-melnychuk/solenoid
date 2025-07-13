use thiserror::Error;

use crate::opcodes::{Opcode, get_opcode};

#[derive(Error, Debug)]
pub enum DecoderError {
    #[error("Invalid opcode 0x{0:02x} found at position {1}")]
    InvalidOpcode(u8, usize),
    #[error("Unexpected end of bytecode after {0} instruction at position {1}")]
    UnexpectedEndOfBytecode(String, usize),
}

#[derive(Debug)]
pub struct Instruction {
    pub opcode: Opcode,
    pub offset: usize,
    pub argument: Option<Vec<u8>>,
}

#[derive(Debug)]
pub struct Bytecode {
    pub instructions: Vec<Instruction>,
    pub jumptable: Vec<(usize, usize)>,
}

impl Bytecode {
    pub fn new(instructions: Vec<Instruction>, jumptable: Vec<(usize, usize)>) -> Self {
        Self {
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
    pub fn decode(code: &[u8]) -> Result<Bytecode, DecoderError> {
        let mut instructions = Vec::new();
        let mut jumptable = Vec::new();

        let mut pos = 0;
        while pos < code.len() {
            let opcode = get_opcode(code[pos]);
            let mut instruction = Instruction {
                opcode,
                offset: pos,
                argument: None,
            };

            // JUMPDEST opcode
            if opcode.code == 0x5b {
                jumptable.push((pos, instructions.len()));
            }

            pos += 1; // Move past the opcode byte

            let push_bytes = opcode.push_width();
            if push_bytes > 0 {
                let start = pos;
                let end = pos + push_bytes;

                if end > code.len() {
                    return Err(DecoderError::UnexpectedEndOfBytecode(
                        opcode.name.to_string(),
                        pos,
                    ));
                }

                instruction.argument = Some(code[start..end].to_vec());
                pos = end;
            }

            instructions.push(instruction);
        }

        Ok(Bytecode {
            instructions,
            jumptable,
        })
    }
}
