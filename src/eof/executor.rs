use crate::{
    common::{call::Call, word::Word},
    eof::decoder::EofContainer,
    executor::{Context, Evm, ExecutorError},
    ext::Ext,
    tracer::{Event, EventData, EventTracer},
};

/// EOF-specific instruction for the EOF execution engine
#[derive(Debug, Clone)]
pub struct EofInstruction {
    pub opcode: u8,
    pub offset: usize,
    pub immediate: Vec<u8>,
}

/// EOF Executor - handles execution of EOF bytecode
pub struct EofExecutor<T: EventTracer> {
    tracer: T,
    ret: Vec<u8>,
}

impl<T: EventTracer> EofExecutor<T> {
    pub fn new(tracer: T) -> Self {
        Self {
            tracer,
            ret: Vec::new(),
        }
    }

    /// Execute EOF bytecode
    pub async fn execute(
        mut self,
        container: &EofContainer,
        call: &Call,
        evm: &mut Evm,
        ext: &mut Ext,
        ctx: Context,
    ) -> (T, Vec<u8>) {
        // Validate the container
        if let Err(e) = container.validate() {
            tracing::error!("EOF validation failed: {}", e);
            evm.stopped = true;
            evm.reverted = true;
            return (self.tracer, vec![]);
        }

        // Start execution from code section 0
        if container.code_sections.is_empty() {
            tracing::warn!("EOF container has no code sections");
            evm.stopped = true;
            evm.reverted = false;
            return (self.tracer, vec![]);
        }

        let code_section = &container.code_sections[0];
        let type_metadata = &container.types[0];

        tracing::debug!(
            "Executing EOF code section 0: {} bytes, inputs={}, outputs={}, max_stack={}",
            code_section.len(),
            type_metadata.inputs,
            type_metadata.outputs,
            type_metadata.max_stack_height
        );

        // Decode instructions from the code section
        let instructions = self.decode_eof_section(code_section);

        // Execute instructions
        let mut pc = 0;
        while pc < instructions.len() && !evm.stopped {
            let instruction = &instructions[pc];

            match self
                .execute_eof_instruction(
                    instruction,
                    container,
                    call,
                    evm,
                    ext,
                    ctx,
                    &mut pc,
                )
                .await
            {
                Ok(gas_cost) => {
                    // Generate trace event for this instruction
                    self.tracer.push(Event {
                        depth: ctx.depth,
                        reverted: false,
                        data: EventData::OpCode {
                            pc: instruction.offset,
                            op: instruction.opcode,
                            name: self.get_opcode_name(instruction.opcode),
                            data: if instruction.immediate.is_empty() {
                                None
                            } else {
                                Some(instruction.immediate.clone().into())
                            },
                            gas_cost: gas_cost.as_i64(),
                            gas_used: evm.gas.used + gas_cost.as_i64(),
                            gas_back: 0, // TODO: track refunds
                            gas_left: evm.gas.remaining() - gas_cost.as_i64(),
                            stack: evm.stack.clone(),
                            memory: evm.memory.chunks(32).map(Word::from_bytes).collect(),
                            extra: serde_json::json!({}),
                        },
                    });

                    if evm.gas(gas_cost.as_i64()).is_err() {
                        evm.stopped = true;
                        evm.reverted = true;
                        return (self.tracer, vec![]);
                    }
                }
                Err(e) => {
                    tracing::error!("EOF instruction failed: {}", e);
                    evm.stopped = true;
                    evm.reverted = true;
                    return (self.tracer, vec![]);
                }
            }

            pc += 1;
        }

        (self.tracer, self.ret)
    }

    /// Decode EOF code section into instructions
    fn decode_eof_section(&self, code: &[u8]) -> Vec<EofInstruction> {
        let mut instructions = Vec::new();
        let mut pos = 0;

        while pos < code.len() {
            let opcode = code[pos];
            let offset = pos;
            pos += 1;

            // Determine immediate data size based on opcode
            let immediate_size = match opcode {
                // RJUMP, RJUMPI - 2 byte immediate (signed offset)
                0xe0 | 0xe1 => 2,
                // RJUMPV - 1 byte count + 2*count bytes offsets
                0xe2 => {
                    if pos < code.len() {
                        let count = code[pos] as usize;
                        1 + count * 2
                    } else {
                        0
                    }
                }
                // CALLF, JUMPF - 2 byte immediate (section index)
                0xe3 | 0xe4 => 2,
                // DATALOADN - 2 byte immediate (offset)
                0xd1 => 2,
                // DUPN, SWAPN, EXCHANGE - 1 byte immediate
                0xe6 | 0xe7 | 0xe8 => 1,
                // EOFCREATE - 1 byte immediate (container index)
                0xec => 1,
                // RETURNCONTRACT - 1 byte immediate (container index)
                0xee => 1,
                // PUSH opcodes
                0x60..=0x7f => (opcode - 0x5f) as usize,
                _ => 0,
            };

            let mut immediate = Vec::new();
            if immediate_size > 0 && pos + immediate_size <= code.len() {
                immediate.extend_from_slice(&code[pos..pos + immediate_size]);
                pos += immediate_size;
            }

            instructions.push(EofInstruction {
                opcode,
                offset,
                immediate,
            });
        }

        instructions
    }

    /// Execute a single EOF instruction
    async fn execute_eof_instruction(
        &mut self,
        instruction: &EofInstruction,
        container: &EofContainer,
        call: &Call,
        evm: &mut Evm,
        ext: &mut Ext,
        ctx: Context,
        pc: &mut usize,
    ) -> eyre::Result<Word> {
        let opcode = instruction.opcode;
        let mut gas;

        match opcode {
            // EOF-specific opcodes
            0xe0 => {
                // RJUMP - relative jump (unconditional)
                if instruction.immediate.len() != 2 {
                    return Err(ExecutorError::MissingData.into());
                }
                let offset = i16::from_be_bytes([instruction.immediate[0], instruction.immediate[1]]);
                *pc = ((*pc as i32) + (offset as i32) + 1) as usize;
                *pc = pc.wrapping_sub(1); // Subtract 1 because it will be incremented
                gas = 2.into();
            }
            0xe1 => {
                // RJUMPI - relative jump (conditional)
                if instruction.immediate.len() != 2 {
                    return Err(ExecutorError::MissingData.into());
                }
                let cond = evm.pop()?;
                if !cond.is_zero() {
                    let offset = i16::from_be_bytes([instruction.immediate[0], instruction.immediate[1]]);
                    *pc = ((*pc as i32) + (offset as i32) + 1) as usize;
                    *pc = pc.wrapping_sub(1); // Subtract 1 because it will be incremented
                }
                gas = 4.into();
            }
            0xe2 => {
                // RJUMPV - relative jump via jump table
                if instruction.immediate.is_empty() {
                    return Err(ExecutorError::MissingData.into());
                }
                let count = instruction.immediate[0] as usize;
                let case = evm.pop()?.as_usize();

                if case < count {
                    let offset_pos = 1 + case * 2;
                    if offset_pos + 2 <= instruction.immediate.len() {
                        let offset = i16::from_be_bytes([
                            instruction.immediate[offset_pos],
                            instruction.immediate[offset_pos + 1],
                        ]);
                        *pc = ((*pc as i32) + (offset as i32) + 1) as usize;
                        *pc = pc.wrapping_sub(1);
                    }
                }
                gas = (4 + 2 * count).into();
            }
            0xe3 => {
                // CALLF - call function (subroutine)
                // For now, treat as STOP since we don't have full call stack support
                tracing::warn!("CALLF not fully implemented, stopping execution");
                evm.stopped = true;
                gas = 5.into();
            }
            0xe4 => {
                // JUMPF - tail call to function
                // For now, treat as STOP
                tracing::warn!("JUMPF not fully implemented, stopping execution");
                evm.stopped = true;
                gas = 5.into();
            }
            0xe5 => {
                // RETF - return from function
                evm.stopped = true;
                self.ret.clear();
                gas = 3.into();
            }
            0xe6 => {
                // DUPN - duplicate stack item at depth n
                if instruction.immediate.len() != 1 {
                    return Err(ExecutorError::MissingData.into());
                }
                let n = instruction.immediate[0] as usize + 1;
                if evm.stack.len() < n {
                    return Err(ExecutorError::StackUnderflow.into());
                }
                let val = evm.stack[evm.stack.len() - n];
                evm.push(val)?;
                gas = 3.into();
            }
            0xe7 => {
                // SWAPN - swap top stack item with item at depth n
                if instruction.immediate.len() != 1 {
                    return Err(ExecutorError::MissingData.into());
                }
                let n = instruction.immediate[0] as usize + 1;
                if evm.stack.len() <= n {
                    return Err(ExecutorError::StackUnderflow.into());
                }
                let stack_len = evm.stack.len();
                evm.stack.swap(stack_len - 1, stack_len - 1 - n);
                gas = 3.into();
            }
            0xe8 => {
                // EXCHANGE - exchange stack items
                if instruction.immediate.len() != 1 {
                    return Err(ExecutorError::MissingData.into());
                }
                let imm = instruction.immediate[0];
                let n = ((imm >> 4) + 1) as usize;
                let m = ((imm & 0x0F) + 1) as usize;

                let stack_len = evm.stack.len();
                if stack_len < n + m + 1 {
                    return Err(ExecutorError::StackUnderflow.into());
                }
                evm.stack.swap(stack_len - 1 - n, stack_len - 1 - n - m);
                gas = 3.into();
            }
            0xd0 => {
                // DATALOAD - load 32 bytes from data section
                let offset = evm.pop()?.as_usize();
                let mut data = [0u8; 32];
                let copy_len = container.data.len().saturating_sub(offset).min(32);
                if copy_len > 0 {
                    data[..copy_len].copy_from_slice(&container.data[offset..offset + copy_len]);
                }
                evm.push(Word::from_bytes(&data))?;
                gas = 4.into();
            }
            0xd1 => {
                // DATALOADN - load 32 bytes from data section (static offset)
                if instruction.immediate.len() != 2 {
                    return Err(ExecutorError::MissingData.into());
                }
                let offset = u16::from_be_bytes([instruction.immediate[0], instruction.immediate[1]]) as usize;
                let mut data = [0u8; 32];
                let copy_len = container.data.len().saturating_sub(offset).min(32);
                if copy_len > 0 {
                    data[..copy_len].copy_from_slice(&container.data[offset..offset + copy_len]);
                }
                evm.push(Word::from_bytes(&data))?;
                gas = 3.into();
            }
            0xd2 => {
                // DATASIZE - get size of data section
                evm.push(Word::from(container.data.len()))?;
                gas = 2.into();
            }
            0xd3 => {
                // DATACOPY - copy data section to memory
                let mem_offset = evm.pop()?.as_usize();
                let data_offset = evm.pop()?.as_usize();
                let size = evm.pop()?.as_usize();

                if mem_offset + size > evm.memory.len() {
                    let padding = 32 - (mem_offset + size) % 32;
                    evm.memory.resize(mem_offset + size + padding % 32, 0);
                }

                let copy_len = container.data.len().saturating_sub(data_offset).min(size);
                if copy_len > 0 {
                    evm.memory[mem_offset..mem_offset + copy_len]
                        .copy_from_slice(&container.data[data_offset..data_offset + copy_len]);
                }
                // Zero-fill rest
                for i in copy_len..size {
                    evm.memory[mem_offset + i] = 0;
                }

                gas = (3 + 3 * size.div_ceil(32)).into();
                gas += evm.memory_expansion_cost();
            }
            0xd4 => {
                // RETURNDATALOAD - load from return data buffer
                let offset = evm.pop()?.as_usize();
                let mut data = [0u8; 32];
                let copy_len = self.ret.len().saturating_sub(offset).min(32);
                if copy_len > 0 {
                    data[..copy_len].copy_from_slice(&self.ret[offset..offset + copy_len]);
                }
                evm.push(Word::from_bytes(&data))?;
                gas = 3.into();
            }
            0xec => {
                // EOFCREATE - create contract with EOF container
                tracing::warn!("EOFCREATE not fully implemented");
                evm.push(Word::zero())?; // Return zero address for now
                gas = 32000.into();
            }
            0xee => {
                // RETURNCONTRACT - return deployed contract
                tracing::warn!("RETURNCONTRACT not fully implemented");
                evm.stopped = true;
                gas = 0.into();
            }
            0xf7 => {
                // RETURNDATA COPY (different behavior in EOF)
                let dest_offset = evm.pop()?.as_usize();
                let offset = evm.pop()?.as_usize();
                let size = evm.pop()?.as_usize();

                if dest_offset + size > evm.memory.len() {
                    let padding = 32 - (dest_offset + size) % 32;
                    evm.memory.resize(dest_offset + size + padding % 32, 0);
                }

                let copy_len = self.ret.len().saturating_sub(offset).min(size);
                if copy_len > 0 {
                    evm.memory[dest_offset..dest_offset + copy_len]
                        .copy_from_slice(&self.ret[offset..offset + copy_len]);
                }

                gas = (3 + 3 * size.div_ceil(32)).into();
                gas += evm.memory_expansion_cost();
            }
            0xf8 => {
                // EXTCALL - external call (EOF version)
                tracing::warn!("EXTCALL not fully implemented, using simplified version");
                let _target_addr = evm.pop()?;
                let _value = evm.pop()?;
                let _args_offset = evm.pop()?;
                let _args_size = evm.pop()?;
                evm.push(Word::one())?; // Success
                gas = 2600.into();
            }
            0xf9 => {
                // EXTDELEGATECALL - external delegatecall (EOF version)
                tracing::warn!("EXTDELEGATECALL not fully implemented");
                evm.push(Word::one())?; // Success
                gas = 2600.into();
            }
            0xfb => {
                // EXTSTATICCALL - external staticcall (EOF version)
                tracing::warn!("EXTSTATICCALL not fully implemented");
                evm.push(Word::one())?; // Success
                gas = 2600.into();
            }

            // Standard EVM opcodes that are allowed in EOF
            // These work the same as in legacy EVM
            _ => {
                // For now, delegate to a helper that handles standard opcodes
                gas = self.execute_standard_opcode(instruction, call, evm, ext, ctx, pc).await?;
            }
        }

        Ok(gas)
    }

    /// Get human-readable name for an opcode
    fn get_opcode_name(&self, opcode: u8) -> String {
        match opcode {
            0x00 => "STOP".to_string(),
            0x01 => "ADD".to_string(),
            0x02 => "MUL".to_string(),
            0x03 => "SUB".to_string(),
            0x10 => "LT".to_string(),
            0x14 => "EQ".to_string(),
            0x15 => "ISZERO".to_string(),
            0x16 => "AND".to_string(),
            0x17 => "OR".to_string(),
            0x20 => "KECCAK256".to_string(),
            0x30 => "ADDRESS".to_string(),
            0x33 => "CALLER".to_string(),
            0x34 => "CALLVALUE".to_string(),
            0x35 => "CALLDATALOAD".to_string(),
            0x36 => "CALLDATASIZE".to_string(),
            0x50 => "POP".to_string(),
            0x51 => "MLOAD".to_string(),
            0x52 => "MSTORE".to_string(),
            0x53 => "MSTORE8".to_string(),
            0x59 => "MSIZE".to_string(),
            0x5f => "PUSH0".to_string(),
            0x60..=0x7f => format!("PUSH{}", opcode - 0x5f),
            0x80..=0x8f => format!("DUP{}", opcode - 0x7f),
            0x90..=0x9f => format!("SWAP{}", opcode - 0x8f),
            0xd0 => "DATALOAD".to_string(),
            0xd1 => "DATALOADN".to_string(),
            0xd2 => "DATASIZE".to_string(),
            0xd3 => "DATACOPY".to_string(),
            0xd4 => "RETURNDATALOAD".to_string(),
            0xe0 => "RJUMP".to_string(),
            0xe1 => "RJUMPI".to_string(),
            0xe2 => "RJUMPV".to_string(),
            0xe3 => "CALLF".to_string(),
            0xe4 => "JUMPF".to_string(),
            0xe5 => "RETF".to_string(),
            0xe6 => "DUPN".to_string(),
            0xe7 => "SWAPN".to_string(),
            0xe8 => "EXCHANGE".to_string(),
            0xec => "EOFCREATE".to_string(),
            0xee => "RETURNCONTRACT".to_string(),
            0xf3 => "RETURN".to_string(),
            0xf7 => "RETURNDATACOPY".to_string(),
            0xf8 => "EXTCALL".to_string(),
            0xf9 => "EXTDELEGATECALL".to_string(),
            0xfb => "EXTSTATICCALL".to_string(),
            0xfd => "REVERT".to_string(),
            _ => format!("UNKNOWN({:#02x})", opcode),
        }
    }

    /// Execute standard EVM opcodes (those that work the same in EOF and legacy)
    async fn execute_standard_opcode(
        &mut self,
        instruction: &EofInstruction,
        call: &Call,
        evm: &mut Evm,
        _ext: &mut Ext,
        ctx: Context,
        _pc: &mut usize,
    ) -> eyre::Result<Word> {
        let opcode = instruction.opcode;
        let mut gas;

        use crate::common::hash::keccak256;

        match opcode {
            // STOP
            0x00 => {
                evm.stopped = true;
                evm.reverted = false;
                self.ret.clear();
                gas = 0.into();
            }
            // Arithmetic Operations (0x01-0x0b)
            0x01 => { // ADD
                let a = evm.pop()?;
                let b = evm.pop()?;
                let (res, _) = a.overflowing_add(b);
                evm.push(res)?;
                gas = 3.into();
            }
            0x02 => { // MUL
                let a = evm.pop()?;
                let b = evm.pop()?;
                let (res, _) = a.overflowing_mul(b);
                evm.push(res)?;
                gas = 5.into();
            }
            0x03 => { // SUB
                let a = evm.pop()?;
                let b = evm.pop()?;
                let (res, _) = a.overflowing_sub(b);
                evm.push(res)?;
                gas = 3.into();
            }
            0x04 => { // DIV
                let a = evm.pop()?;
                let b = evm.pop()?;
                let res = if b.is_zero() { Word::zero() } else { a / b };
                evm.push(res)?;
                gas = 5.into();
            }
            0x05 => { // SDIV
                let a = evm.pop()?;
                let b = evm.pop()?;
                let res = if b.is_zero() { Word::zero() } else {
                    // Simplified - proper signed division would need i256
                    a / b
                };
                evm.push(res)?;
                gas = 5.into();
            }
            0x06 => { // MOD
                let a = evm.pop()?;
                let b = evm.pop()?;
                let res = if b.is_zero() { Word::zero() } else { a % b };
                evm.push(res)?;
                gas = 5.into();
            }
            0x07 => { // SMOD
                let a = evm.pop()?;
                let b = evm.pop()?;
                let res = if b.is_zero() { Word::zero() } else { a % b };
                evm.push(res)?;
                gas = 5.into();
            }
            0x08 => { // ADDMOD
                let a = evm.pop()?;
                let b = evm.pop()?;
                let n = evm.pop()?;
                let res = if n.is_zero() {
                    Word::zero()
                } else {
                    // (a + b) % n - simplified
                    let (sum, _) = a.overflowing_add(b);
                    sum % n
                };
                evm.push(res)?;
                gas = 8.into();
            }
            0x09 => { // MULMOD
                let a = evm.pop()?;
                let b = evm.pop()?;
                let n = evm.pop()?;
                let res = if n.is_zero() {
                    Word::zero()
                } else {
                    let (prod, _) = a.overflowing_mul(b);
                    prod % n
                };
                evm.push(res)?;
                gas = 8.into();
            }
            0x0a => { // EXP
                let a = evm.pop()?;
                let exponent = evm.pop()?;
                let res = a.pow(exponent);
                evm.push(res)?;
                gas = 10.into(); // Simplified gas
            }
            0x0b => { // SIGNEXTEND
                let b = evm.pop()?.as_usize();
                let x = evm.pop()?;
                let res = if b < 31 {
                    // let bit_index = (b + 1) * 8 - 1;
                    let bytes = x.into_bytes();
                    let sign_bit = (bytes[31 - b] >> 7) & 1;
                    if sign_bit == 1 {
                        // Extend with 1s
                        let mut result = [0xffu8; 32];
                        result[32 - b - 1..].copy_from_slice(&bytes[32 - b - 1..]);
                        Word::from_bytes(&result)
                    } else {
                        x
                    }
                } else {
                    x
                };
                evm.push(res)?;
                gas = 5.into();
            }
            0x10 => { // LT
                let a = evm.pop()?;
                let b = evm.pop()?;
                evm.push(if a < b { Word::one() } else { Word::zero() })?;
                gas = 3.into();
            }
            0x11 => { // GT
                let a = evm.pop()?;
                let b = evm.pop()?;
                evm.push(if a > b { Word::one() } else { Word::zero() })?;
                gas = 3.into();
            }
            0x12 => { // SLT (signed less than)
                let a = evm.pop()?;
                let b = evm.pop()?;
                // Simplified signed comparison
                evm.push(if a < b { Word::one() } else { Word::zero() })?;
                gas = 3.into();
            }
            0x13 => { // SGT (signed greater than)
                let a = evm.pop()?;
                let b = evm.pop()?;
                evm.push(if a > b { Word::one() } else { Word::zero() })?;
                gas = 3.into();
            }
            0x14 => { // EQ
                let a = evm.pop()?;
                let b = evm.pop()?;
                evm.push(if a == b { Word::one() } else { Word::zero() })?;
                gas = 3.into();
            }
            0x15 => { // ISZERO
                let a = evm.pop()?;
                evm.push(if a.is_zero() { Word::one() } else { Word::zero() })?;
                gas = 3.into();
            }
            0x16 => { // AND
                let a = evm.pop()?;
                let b = evm.pop()?;
                evm.push(a & b)?;
                gas = 3.into();
            }
            0x17 => { // OR
                let a = evm.pop()?;
                let b = evm.pop()?;
                evm.push(a | b)?;
                gas = 3.into();
            }
            0x18 => { // XOR
                let a = evm.pop()?;
                let b = evm.pop()?;
                evm.push(a ^ b)?;
                gas = 3.into();
            }
            0x19 => { // NOT
                let a = evm.pop()?;
                evm.push(!a)?;
                gas = 3.into();
            }
            0x1a => { // BYTE
                let i = evm.pop()?.as_usize();
                let x = evm.pop()?;
                let byte = if i < 32 {
                    let bytes = x.into_bytes();
                    Word::from(bytes[i] as usize)
                } else {
                    Word::zero()
                };
                evm.push(byte)?;
                gas = 3.into();
            }
            0x1b => { // SHL (shift left)
                let shift = evm.pop()?.as_usize();
                let value = evm.pop()?;
                let res = if shift >= 256 {
                    Word::zero()
                } else {
                    value << shift
                };
                evm.push(res)?;
                gas = 3.into();
            }
            0x1c => { // SHR (logical shift right)
                let shift = evm.pop()?.as_usize();
                let value = evm.pop()?;
                let res = if shift >= 256 {
                    Word::zero()
                } else {
                    value >> shift
                };
                evm.push(res)?;
                gas = 3.into();
            }
            0x1d => { // SAR (arithmetic shift right)
                let shift = evm.pop()?.as_usize();
                let value = evm.pop()?;
                // Check sign bit (most significant bit)
                let bytes = value.into_bytes();
                let is_negative = (bytes[0] & 0x80) != 0;

                let res = if shift >= 256 {
                    if is_negative {
                        Word::from_bytes(&[0xff; 32])
                    } else {
                        Word::zero()
                    }
                } else {
                    let shifted = value >> shift;
                    if is_negative && shift > 0 {
                        // Fill with 1s from the left
                        let mask = Word::from_bytes(&[0xff; 32]) << (256 - shift);
                        shifted | mask
                    } else {
                        shifted
                    }
                };
                evm.push(res)?;
                gas = 3.into();
            }
            0x20 => { // KECCAK256
                let offset = evm.pop()?.as_usize();
                let size = evm.pop()?.as_usize();
                if offset + size > evm.memory.len() {
                    return Err(ExecutorError::MissingData.into());
                }
                let data = &evm.memory[offset..offset + size];
                let hash = keccak256(data);
                evm.push(Word::from_bytes(&hash))?;
                gas = (30 + 6 * size.div_ceil(32)).into();
            }
            // Environmental Information (0x30-0x3f)
            0x30 => { // ADDRESS
                let this = if call.to.is_zero() {
                    ctx.created
                } else {
                    call.to
                };
                evm.push((&this).into())?;
                gas = 2.into();
            }
            0x33 => { // CALLER
                evm.push((&call.from).into())?;
                gas = 2.into();
            }
            0x34 => { // CALLVALUE
                evm.push(call.value)?;
                gas = 2.into();
            }
            0x35 => { // CALLDATALOAD
                let offset = evm.pop()?.as_usize();
                let mut data = [0u8; 32];
                let copy = call.data.len().saturating_sub(offset).min(32);
                if copy > 0 {
                    data[..copy].copy_from_slice(&call.data[offset..offset + copy]);
                }
                evm.push(Word::from_bytes(&data))?;
                gas = 3.into();
            }
            0x36 => { // CALLDATASIZE
                evm.push(Word::from(call.data.len()))?;
                gas = 2.into();
            }
            0x37 => { // CALLDATACOPY
                let dest_offset = evm.pop()?.as_usize();
                let offset = evm.pop()?.as_usize();
                let size = evm.pop()?.as_usize();

                if dest_offset + size > evm.memory.len() {
                    let padding = 32 - (dest_offset + size) % 32;
                    evm.memory.resize(dest_offset + size + padding % 32, 0);
                }

                let copy_len = call.data.len().saturating_sub(offset).min(size);
                if copy_len > 0 {
                    evm.memory[dest_offset..dest_offset + copy_len]
                        .copy_from_slice(&call.data[offset..offset + copy_len]);
                }
                // Zero-fill rest
                for i in copy_len..size {
                    evm.memory[dest_offset + i] = 0;
                }

                gas = (3 + 3 * size.div_ceil(32)).into();
                gas += evm.memory_expansion_cost();
            }
            0x3d => { // RETURNDATASIZE
                evm.push(Word::from(self.ret.len()))?;
                gas = 2.into();
            }
            0x3e => { // RETURNDATACOPY
                let dest_offset = evm.pop()?.as_usize();
                let offset = evm.pop()?.as_usize();
                let size = evm.pop()?.as_usize();

                if offset + size > self.ret.len() {
                    return Err(ExecutorError::MissingData.into());
                }

                if dest_offset + size > evm.memory.len() {
                    let padding = 32 - (dest_offset + size) % 32;
                    evm.memory.resize(dest_offset + size + padding % 32, 0);
                }

                evm.memory[dest_offset..dest_offset + size]
                    .copy_from_slice(&self.ret[offset..offset + size]);

                gas = (3 + 3 * size.div_ceil(32)).into();
                gas += evm.memory_expansion_cost();
            }
            // Memory Operations (0x50-0x5f)
            0x50 => { // POP
                evm.pop()?;
                gas = 2.into();
            }
            0x51 => { // MLOAD
                let offset = evm.pop()?.as_usize();
                let end = offset + 32;
                if end > evm.memory.len() {
                    let padding = 32 - end % 32;
                    evm.memory.resize(end + padding % 32, 0);
                }
                let value = Word::from_bytes(&evm.memory[offset..end]);
                evm.push(value)?;
                gas = 3.into();
                gas += evm.memory_expansion_cost();
            }
            0x52 => { // MSTORE
                let offset = evm.pop()?.as_usize();
                let value = evm.pop()?;
                let end = offset + 32;
                if end > evm.memory.len() {
                    let padding = 32 - end % 32;
                    evm.memory.resize(end + padding % 32, 0);
                }
                evm.memory[offset..end].copy_from_slice(&value.into_bytes());
                gas = 3.into();
                gas += evm.memory_expansion_cost();
            }
            0x53 => { // MSTORE8
                let offset = evm.pop()?.as_usize();
                let value = evm.pop()?;
                if offset >= evm.memory.len() {
                    let padding = 32 - (offset + 1) % 32;
                    evm.memory.resize(offset + 1 + padding % 32, 0);
                }
                evm.memory[offset] = value.into_bytes()[31];
                gas = 3.into();
                gas += evm.memory_expansion_cost();
            }
            0x59 => { // MSIZE
                evm.push(Word::from(evm.memory.len()))?;
                gas = 2.into();
            }
            0x5f => { // PUSH0
                evm.push(Word::zero())?;
                gas = 2.into();
            }
            // PUSH opcodes (0x60-0x7f)
            0x60..=0x7f => {
                if instruction.immediate.is_empty() {
                    return Err(ExecutorError::MissingData.into());
                }
                evm.push(Word::from_bytes(&instruction.immediate))?;
                gas = 3.into();
            }
            // DUP opcodes (0x80-0x8f)
            0x80..=0x8f => {
                let n = (opcode - 0x7f) as usize;
                if evm.stack.len() < n {
                    return Err(ExecutorError::StackUnderflow.into());
                }
                let val = evm.stack[evm.stack.len() - n];
                evm.push(val)?;
                gas = 3.into();
            }
            // SWAP opcodes (0x90-0x9f)
            0x90..=0x9f => {
                let n = (opcode - 0x8f) as usize;
                if evm.stack.len() <= n {
                    return Err(ExecutorError::StackUnderflow.into());
                }
                let stack_len = evm.stack.len();
                evm.stack.swap(stack_len - 1, stack_len - 1 - n);
                gas = 3.into();
            }
            // RETURN / REVERT (0xf3, 0xfd)
            0xf3 | 0xfd => {
                evm.stopped = true;
                evm.reverted = opcode == 0xfd;

                let offset = evm.pop()?.as_usize();
                let size = evm.pop()?.as_usize();

                if size > 0 {
                    if offset + size > evm.memory.len() {
                        let padding = 32 - (offset + size) % 32;
                        evm.memory.resize(offset + size + padding % 32, 0);
                    }
                    self.ret = evm.memory[offset..offset + size].to_vec();
                } else {
                    self.ret.clear();
                }
                gas = evm.memory_expansion_cost();
            }
            // Prohibited opcodes in EOF - should never reach here
            0x38 | 0x39 | 0x3b | 0x3c | 0x3f | // CODESIZE, CODECOPY, EXTCODESIZE, EXTCODECOPY, EXTCODEHASH
            0x56 | 0x57 | 0x58 | 0x5a |       // JUMP, JUMPI, PC, GAS
            0xf0 | 0xf1 | 0xf2 | 0xf5 | 0xff  // CREATE, CALL, CALLCODE, CREATE2, SELFDESTRUCT
            => {
                tracing::error!("Prohibited opcode {:#02x} in EOF execution", opcode);
                return Err(ExecutorError::InvalidOpcode(opcode).into());
            }
            _ => {
                tracing::warn!("Unimplemented opcode {:#02x} in EOF executor", opcode);
                evm.stopped = true;
                gas = 0.into();
            }
        }

        Ok(gas)
    }
}