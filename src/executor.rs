use eyre::Context as _;
use i256::I256;
use serde_json::json;
use thiserror::Error;

use crate::{
    common::{
        address::Address,
        block::Header,
        call::Call,
        hash::keccak256,
        word::{Word, decode_error_string},
    },
    decoder::{Bytecode, Decoder, Instruction},
    ext::Ext,
    precompiles,
    tracer::{AccountEvent, CallType, Event, EventData, EventTracer, HashAlg, StateEvent},
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
    #[error("Expected JUMPDEST but got: {0}")]
    InvalidJump(u8),
    #[error("Missing data")]
    MissingData,
    #[error("Wrong returned data size: expected {exp} but got {got}")]
    WrongReturnDataSize { exp: usize, got: usize },
    #[error("Invalid opcode: {0:#02x}")]
    InvalidOpcode(u8),
    #[error("Unknown opcode: {0:#02x}")]
    UnknownOpcode(u8),
    #[error("Call run out of gas")]
    OutOfGas(),
    #[error("Insufficient funds: have {have:?}, need {need:?}")]
    InsufficientFunds { have: Word, need: Word },
    #[error("Unallowed opcode from static call: {0}")]
    StaticCallViolation(u8),
    #[error("Invalid allocation: {0}")]
    InvalidAllocation(usize),
}

const STACK_LIMIT: usize = 1024;

const CALL_DEPTH_LIMIT: usize = 1024;

// 1MB: opinionated allocation sanity check limit
const ALLOCATION_SANITY_LIMIT: usize = 1024 * 1024;

#[derive(Debug, Default, Eq, PartialEq)]
pub enum StateTouch {
    #[default]
    Noop,
    Get(Address, Word, Word, bool),
    Put(Address, Word, Word, Word, bool),
}

#[derive(Debug, Default, Eq, PartialEq)]
pub enum AccountTouch {
    #[default]
    Noop,
    WarmUp(Address),
    GetNonce(Address, u64),
    GetValue(Address, Word),
    GetCode(Address, Word, Vec<u8>),
    SetNonce(Address, u64, u64),
    SetValue(Address, Word, Word),
    SetCode(Address, (Word, Vec<u8>), (Word, Vec<u8>)),
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

    pub refund: i64,
}

impl Evm {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn memory_expansion_cost(&mut self) -> Word {
        let mem_len = self.memory.len().div_ceil(32);
        let mem_cost = (mem_len * mem_len) / 512 + (3 * mem_len);
        let mem_cost = Word::from(mem_cost);
        let exp_cost = mem_cost - self.mem_cost;
        self.mem_cost = mem_cost;
        exp_cost
    }

    pub(crate) fn address_access_cost(&mut self, address: &Address, ext: &mut Ext) -> Word {
        // EIP-2929: Check if address has been accessed during this transaction
        if precompiles::is_precompile(address) {
            return Word::from(100);
        }
        let is_warm = ext.is_address_warm(address);
        if !is_warm {
            ext.warm_address(address);
            self.account.push(AccountTouch::WarmUp(*address));
        }
        if is_warm {
            Word::from(100) // warm access
        } else {
            Word::from(2600) // cold access
        }
    }

    pub(crate) fn error(&mut self, e: eyre::Report) -> eyre::Result<()> {
        self.stopped = true;
        self.reverted = true;
        // On exceptional halt, consume all remaining gas
        self.gas.used += self.gas.remaining();
        Err(e)
    }

    pub fn push(&mut self, value: Word) -> eyre::Result<()> {
        if self.stack.len() >= STACK_LIMIT {
            self.error(ExecutorError::StackOverflow.into())?;
        }
        self.stack.push(value);
        Ok(())
    }

    pub fn pop(&mut self) -> eyre::Result<Word> {
        if let Some(word) = self.stack.pop() {
            Ok(word)
        } else {
            self.error(ExecutorError::StackUnderflow.into())
                .map(|_| Word::zero())
        }
    }

    pub fn gas(&mut self, cost: i64) -> eyre::Result<()> {
        match self.gas.sub(cost) {
            Ok(_) => Ok(()),
            Err(e) => self.error(e.into()),
        }
    }

    pub async fn get(&mut self, ext: &mut Ext, addr: &Address, key: &Word) -> eyre::Result<Word> {
        match ext.get(addr, key).await {
            Ok(word) => Ok(word),
            Err(e) => self.error(e).map(|_| Word::zero()),
        }
    }

    pub async fn put(
        &mut self,
        ext: &mut Ext,
        addr: &Address,
        key: Word,
        val: Word,
    ) -> eyre::Result<()> {
        match ext.put(addr, key, val).await {
            Ok(_) => Ok(()),
            Err(e) => self.error(e),
        }
    }

    pub async fn revert(&mut self, ext: &mut Ext) -> eyre::Result<()> {
        for st in self.state.iter().rev() {
            match st {
                StateTouch::Put(address, key, val, _, is_warm) => {
                    if *is_warm {
                        ext.put(address, *key, *val).await?
                    } else {
                        ext.accessed_storage.remove(&(*address, *key));
                    }
                }
                StateTouch::Get(address, key, _, is_warm) => {
                    if *is_warm {
                        // nothing to do
                    } else {
                        ext.accessed_storage.remove(&(*address, *key));
                    }
                }
                _ => (),
            }
        }
        for at in self.account.iter().rev() {
            match at {
                AccountTouch::SetNonce(addr, val, _new) => {
                    ext.account_mut(addr).nonce = (*val).into();
                }
                AccountTouch::SetValue(addr, val, _new) => {
                    ext.account_mut(addr).value = *val;
                }
                AccountTouch::SetCode(addr, (old_hash, old_code), _new) => {
                    *ext.code_mut(addr) = (old_code.clone(), *old_hash);
                }
                AccountTouch::WarmUp(addr) => {
                    ext.accessed_addresses.remove(addr);
                }
                _ => (),
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct Gas {
    pub limit: i64,
    pub used: i64,
    pub refund: i64,
}

impl Gas {
    pub fn new(limit: i64) -> Self {
        Self {
            limit,
            used: 0,
            refund: 0,
        }
    }

    pub fn remaining(&self) -> i64 {
        self.limit - self.used
    }

    pub fn finalized(&self, call_cost: i64, reverted: bool) -> i64 {
        if reverted {
            self.used + call_cost
        } else {
            let used = self.used + call_cost;
            let cap = self.refund.min(used / 5);
            used.saturating_sub(cap)
        }
    }

    pub fn fork(&self, limit: i64) -> Self {
        Self {
            limit,
            used: 0,
            refund: 0,
        }
    }

    pub fn refund(&mut self, gas: i64) {
        self.refund += gas;
    }

    pub fn sub(&mut self, gas: i64) -> Result<(), ExecutorError> {
        if gas > self.remaining() {
            self.used += gas.min(self.remaining());
            return Err(ExecutorError::OutOfGas());
        }
        self.used += gas;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Context {
    pub created: Address,
    pub origin: Address,
    pub depth: usize,

    pub call_type: CallType,
    // block, gas price, etc
}

#[derive(Default)]
pub struct Executor<T: EventTracer> {
    header: Header,
    tracer: T,
    ret: Vec<u8>,
    log: bool,
    debug: serde_json::Value,
}

impl<T: EventTracer> Executor<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_header(self, header: Header) -> Self {
        Self { header, ..self }
    }

    pub fn with_log(self) -> Self {
        Self { log: true, ..self }
    }

    pub fn set_log(&mut self, log: bool) {
        self.log = log;
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
    ) -> eyre::Result<(T, Vec<u8>)> {
        let mut gas = call.gas.as_i64();
        let call_cost = 21000;
        gas -= call_cost;

        let data_cost = {
            let total_calldata_len = call.data.len();
            let nonzero_bytes_count = call.data.iter().filter(|byte| byte != &&0).count();
            nonzero_bytes_count * 16 + (total_calldata_len - nonzero_bytes_count) * 4
        };
        gas -= data_cost as i64;

        evm.gas = Gas::new(gas);

        // TODO: sort out value transfer!
        let src = ext.balance(&call.from).await?;
        let dst = ext.balance(&call.to).await?;
        if !call.value.is_zero() && !call.to.is_zero() {
            // if src < call.value {
            //     return Err(ExecutorError::InsufficientFunds {
            //         have: src,
            //         need: call.value,
            //     }
            //     .into());
            // }

            ext.account_mut(&call.from).value -= call.value;
            evm.account
                .push(AccountTouch::SetValue(call.from, src, src - call.value));
            self.tracer.push(Event {
                data: EventData::Account(AccountEvent::SetValue {
                    address: call.from,
                    val: src,
                    new: src - call.value,
                }),
                depth: 0,
                reverted: false,
            });

            ext.account_mut(&call.to).value += call.value;
            evm.account
                .push(AccountTouch::SetValue(call.to, dst, dst + call.value));
            self.tracer.push(Event {
                data: EventData::Account(AccountEvent::SetValue {
                    address: call.to,
                    val: src,
                    new: src + call.value,
                }),
                depth: 0,
                reverted: false,
            });
        }

        let nonce = ext.nonce(&call.from).await?;
        ext.account_mut(&call.from).nonce = nonce + Word::one();
        evm.account.push(AccountTouch::SetNonce(
            call.from,
            nonce.as_u64(),
            nonce.as_u64() + 1,
        ));
        self.tracer.push(Event {
            data: EventData::Account(AccountEvent::SetNonce {
                address: call.from,
                val: nonce.as_u64(),
                new: nonce.as_u64() + 1,
            }),
            depth: 0,
            reverted: false,
        });

        let is_transfer_only =
            code.bytecode.is_empty() && call.data.is_empty() && !call.to.is_zero();
        if is_transfer_only {
            evm.stopped = true;
            evm.reverted = false;
            return Ok((self.tracer, vec![]));
        }

        let created = call.from.create(nonce);
        let ctx = Context {
            created,
            origin: call.from,
            depth: 1,
            ..Context::default()
        };

        let tracer = self.tracer.fork();
        let mut executor = Executor::<T>::with_tracer(tracer).with_header(self.header);
        executor.set_log(self.log);
        let (tracer, ret) = executor
            .execute_with_context(code, call, evm, ext, ctx)
            .await;
        self.tracer.join(tracer, evm.reverted);

        // TODO: sort out gas fee value reduction!
        // (see: https://www.blocknative.com/blog/eip-1559-fees)
        /*let gas_price = Word::from(1_000_000_000);
        let gas_final = evm.gas.finalized(0); // TODO: use finalised gas
        let gas_fee = Word::from(gas_final) * gas_price;
        let src = ext.balance(&call.from).await?;
        if src < gas_fee {
            return Err(ExecutorError::InsufficientFunds {
                have: src,
                need: gas_fee,
            }
            .into());
        }
        ext.account_mut(&call.from).value -= gas_fee;
        evm.account.push(AccountTouch::SetValue(
            call.from,
            src,
            src - gas_fee,
        ));
        self.tracer.push(Event {
            data: EventData::Account(AccountEvent::SetValue {
                address: call.from,
                val: src,
                new: src - gas_fee
            }),
            depth: 0,
            reverted: false,
        });*/

        Ok((self.tracer, ret))
    }

    pub async fn execute_with_context(
        mut self,
        code: &Bytecode,
        call: &Call,
        evm: &mut Evm,
        ext: &mut Ext,
        ctx: Context,
    ) -> (T, Vec<u8>) {
        // EIP-2929: Pre-warm sender and target addresses at transaction start
        if ctx.depth == 1 {
            ext.pull(&call.from).await.expect("pre-warm:from");
            ext.warm_address(&call.from);
            evm.account.push(AccountTouch::WarmUp(call.from));

            if !call.to.is_zero() {
                ext.pull(&call.to).await.expect("pre-warm:to");
                ext.warm_address(&call.to);
                evm.account.push(AccountTouch::WarmUp(call.to));
            }

            // EIP-3651 (Shanghai): Pre-warm coinbase address
            let coinbase = self.header.miner;
            if !coinbase.is_zero() {
                ext.pull(&coinbase).await.expect("pre-warm:coinbase");
                ext.warm_address(&coinbase);
                evm.account.push(AccountTouch::WarmUp(coinbase));
            }
        }

        self.tracer.push(Event {
            data: EventData::Call {
                r#type: ctx.call_type,
                data: call.data.clone().into(),
                value: call.value,
                from: call.from,
                to: call.to,
                gas: call.gas,
            },
            depth: ctx.depth,
            reverted: false,
        });

        if ctx.depth > CALL_DEPTH_LIMIT {
            evm.stopped = true;
            evm.reverted = true;
            return (self.tracer, vec![]);
        }

        while !evm.stopped && evm.pc < code.instructions.len() {
            let pc = evm.pc;
            let instruction = &code.instructions[pc];
            match self
                .execute_instruction(code, call, evm, ext, ctx, instruction)
                .await {
                Ok(cost) => {
                    let cost = cost.as_i64();
                    let charged_cost = cost.min(evm.gas.remaining());
                    if !instruction.is_call() {
                        // HERE: TODO: remove this label
                        let refund = evm.gas.refund - evm.refund;
                        evm.refund = evm.gas.refund;

                        self.debug = json!({
                            "is_call": false,
                            "gas_left": evm.gas.remaining() - cost,
                            "gas_cost": cost,
                            "evm.gas.used": evm.gas.used,
                            "evm.gas.back": evm.gas.refund,
                        });

                        self.tracer.push(Event {
                            depth: ctx.depth,
                            reverted: false,
                            data: EventData::OpCode {
                                pc: instruction.offset,
                                op: instruction.opcode.code,
                                name: instruction.opcode.name(),
                                data: instruction.argument.clone().map(Into::into),
                                gas_cost: charged_cost,
                                gas_used: (evm.gas.used + charged_cost),
                                gas_back: refund,
                                gas_left: evm.gas.remaining() - charged_cost,
                                stack: evm.stack.clone(),
                                memory: evm.memory.chunks(32).map(Word::from_bytes).collect(),
                                debug: json!(self.debug),
                            },
                        });
                    }
                    if instruction.opcode.code == 0xfe {
                        // INVALID opcode
                        evm.gas.sub(evm.gas.remaining()).expect("must succeed");
                        evm.stopped = true;
                        evm.reverted = true;
                        return (self.tracer, vec![]);
                    }
                    if evm.gas(cost).is_err() {
                        // out of gas
                        evm.stopped = true;
                        evm.reverted = true;
                        return (self.tracer, vec![]);
                    }
                }
                Err(_) => {
                    // opcode failed
                    evm.stopped = true;
                    evm.reverted = true;
                    return (self.tracer, vec![]);
                }
            }

            if self.log {
                let data = instruction
                    .argument
                    .as_ref()
                    .map(|data| format!("0x{}", hex::encode(data)));
                println!(
                    "{:#06x}: {} {}",
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

        (self.tracer, self.ret)
    }

    async fn execute_instruction(
        &mut self,
        code: &Bytecode,
        call: &Call,
        evm: &mut Evm,
        ext: &mut Ext,
        ctx: Context,
        instruction: &Instruction,
    ) -> eyre::Result<Word> {
        self.debug = json!({});
        let mut gas = Word::zero();
        let mut pc_increment = true;

        let this = if call.to.is_zero() {
            ctx.created
        } else {
            call.to
        };

        let opcode = instruction.opcode.code;
        match opcode {
            // STOP
            0x00 => {
                evm.pc = code.instructions.len();
                evm.stopped = true;
                evm.reverted = false;
                self.ret.clear();

                self.tracer.push(Event {
                    data: EventData::Return {
                        ok: true,
                        data: vec![].into(),
                        error: None,
                        gas_used: evm.gas.used,
                    },
                    depth: ctx.depth,
                    reverted: evm.reverted,
                });
                return Ok(gas);
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
                let a = evm.pop()?;
                let b = evm.pop()?;
                let m = evm.pop()?;
                let res = if m.is_zero() {
                    Word::zero()
                } else {
                    (&a).add_modulo(&b, &m)
                };
                evm.push(res)?;
                gas = 8.into();
            }
            0x09 => {
                // MULMOD
                let a = evm.pop()?;
                let b = evm.pop()?;
                let m = evm.pop()?;
                let res = a.mul_modulo(&b, &m);
                evm.push(res)?;
                gas = 8.into();
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
                let x = evm.pop()?.as_usize();
                let b = evm.pop()?;

                let bit = ((x + 1) << 3) - 1;
                let neg = b.bit(bit);

                let mask = Word::max() << (bit + 1);
                let y = if neg { b | mask } else { b & !mask };
                evm.push(y)?;
                gas = 5.into();
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
                let index = evm.pop()?.as_usize();
                let value: Word = evm.pop()?;
                if index < 32 {
                    evm.push(Word::from(value.into_bytes()[index]))?;
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
                    return Err(ExecutorError::MissingData.into());
                }
                let data = &evm.memory[offset..offset + size];
                let sha3 = keccak256(data);
                let hash = Word::from_bytes(&sha3);
                self.tracer.push(Event {
                    data: EventData::Hash {
                        data: data.to_vec().into(),
                        hash: sha3.to_vec().into(),
                        alg: HashAlg::Keccak256,
                    },
                    depth: ctx.depth,
                    reverted: false,
                });
                #[cfg(feature = "tracing")]
                tracing::debug!(
                    preimage = hex::encode(data),
                    keccak256 = hex::encode(sha3),
                    "HASH"
                );
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
                // EIP-2929: Use proper address access tracking
                gas = evm.address_access_cost(&addr, ext);
                let value = ext.balance(&addr).await?;
                evm.push(value)?;
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
                let value = if matches!(ctx.call_type, CallType::Static) {
                    Word::zero()
                } else {
                    call.value
                };
                evm.push(value)?;
                gas = 2.into();
            }
            0x35 => {
                // CALLDATALOAD
                let offset = evm.pop()?.as_usize();
                if offset > call.data.len() {
                    evm.push(Word::zero())?;
                } else {
                    let mut data = [0u8; 32];
                    let copy = call.data.len().min(offset + 32) - offset;
                    data[0..copy].copy_from_slice(&call.data[offset..offset + copy]);
                    evm.push(Word::from_bytes(&data))?;
                }
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
                if size > 0 && dest_offset + size > evm.memory.len() {
                    if dest_offset + size > ALLOCATION_SANITY_LIMIT {
                        return Err(ExecutorError::InvalidAllocation(dest_offset + size).into());
                    }
                    let padding = 32 - (dest_offset + size) % 32;
                    evm.memory.resize(dest_offset + size + padding % 32, 0);
                }
                let mut buffer = call.data.clone();
                if size > 0 && offset + size > buffer.len() {
                    if offset + size > ALLOCATION_SANITY_LIMIT {
                        return Err(ExecutorError::InvalidAllocation(offset + size).into());
                    }
                    let padding = 32 - (offset + size) % 32;
                    buffer.resize(offset + size + padding % 32, 0);
                }
                if size > 0 {
                    evm.memory[dest_offset..dest_offset + size]
                        .copy_from_slice(&buffer[offset..offset + size]);
                }
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
                    if dest_offset + size > ALLOCATION_SANITY_LIMIT {
                        return Err(ExecutorError::InvalidAllocation(dest_offset + size).into());
                    }
                    let padding = 32 - (dest_offset + size) % 32;
                    evm.memory.resize(dest_offset + size + padding % 32, 0);
                }
                let mut code = code.bytecode.clone();
                if code.len() < offset + size {
                    code.resize(offset + size, 0);
                }
                evm.memory[dest_offset..dest_offset + size]
                    .copy_from_slice(&code[offset..offset + size]);
                gas = (3 + 3 * size.div_ceil(32)).into();
                gas += evm.memory_expansion_cost();
            }
            0x3a => {
                // GASPRICE
                evm.push(ext.gas_price)?;
                gas = 2.into();
            }
            0x3b => {
                // EXTCODESIZE
                let address: Address = (&evm.pop()?).into();
                gas = evm.address_access_cost(&address, ext);
                let code_size = ext.code(&address).await?.0.len();
                evm.push(Word::from(code_size))?;
            }
            0x3c => {
                // EXTCODECOPY
                let address: Address = (&evm.pop()?).into();
                let dest_offset = evm.pop()?.as_usize();
                let offset = evm.pop()?.as_usize();
                let size = evm.pop()?.as_usize();

                let (code, _) = ext.code(&address).await?;
                if evm.memory.len() < dest_offset + size {
                    if dest_offset + size > ALLOCATION_SANITY_LIMIT {
                        return Err(ExecutorError::InvalidAllocation(dest_offset + size).into());
                    }
                    let padding = 32 - (dest_offset + size) % 32;
                    evm.memory.resize(dest_offset + size + padding % 32, 0);
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
                    if dest_offset + size > ALLOCATION_SANITY_LIMIT {
                        return Err(ExecutorError::InvalidAllocation(dest_offset + size).into());
                    }
                    let padding = 32 - (dest_offset + size) % 32;
                    evm.memory.resize(dest_offset + size + padding % 32, 0);
                }
                // Copy only the available return data and zero-fill the rest
                let available = self.ret.len().saturating_sub(offset);
                let to_copy = size.min(available);

                if to_copy > 0 {
                    evm.memory[dest_offset..dest_offset + to_copy]
                        .copy_from_slice(&self.ret[offset..offset + to_copy]);
                }
                if to_copy < size {
                    for b in &mut evm.memory[dest_offset + to_copy..dest_offset + size] {
                        *b = 0;
                    }
                }
                gas = (3 + 3 * size.div_ceil(32)).into();
                gas += evm.memory_expansion_cost();
            }
            0x3f => {
                // EXTCODEHASH
                let address: Address = (&evm.pop()?).into();
                let is_empty = ext.is_empty(&address).await?;
                if is_empty {
                    evm.push(Word::zero())?;
                } else {
                    let (_, hash) = ext.code(&address).await?;
                    evm.push(hash)?;
                }
                gas = evm.address_access_cost(&address, ext);
            }

            // 40-4a
            0x40 => {
                // BLOCKHASH
                let block_number = evm.pop()?;
                let block_hash = ext.get_block_hash(block_number).await?;
                evm.push(block_hash)?;
                gas = 20.into();
            }
            0x41 => {
                // COINBASE
                evm.push((&self.header.miner).into())?;
                gas = 2.into();
            }
            0x42 => {
                // TIMESTAMP
                evm.push(self.header.timestamp)?;
                gas = 2.into();
            }
            0x43 => {
                // NUMBER
                evm.push(self.header.number)?;
                gas = 2.into();
            }
            0x44 => {
                // PREVRANDAO
                evm.push(self.header.mix_hash)?;
                gas = 2.into();
            }
            0x45 => {
                // GASLIMIT
                evm.push(self.header.gas_limit)?;
                gas = 2.into();
            }
            0x46 => {
                // CHAINID
                evm.push(Word::one())?; // TODO: From TX
                gas = 2.into();
            }
            0x47 => {
                // SELFBALANCE
                let balance = ext.balance(&this).await?;

                self.debug["SELFBALANCE"] = json!({
                    "address": this,
                    "balance": balance,
                });

                evm.push(balance)?;
                gas = 5.into();
            }
            0x48 => {
                // BASEFEE
                evm.push(self.header.base_fee)?;
                gas = 2.into();
            }
            0x49 => {
                // BLOBHASH
                let _index = evm.pop()?;
                // evm.push(self.header.extra_data)?;
                evm.push(Word::zero())?;
                // TODO: make it work properly?
                // > tx.blob_versioned_hashes[index] if index < len(tx.blob_versioned_hashes),
                // > and otherwise with a zeroed bytes32 value."
                // (See: https://www.evm.codes/?fork=prague#49)
                gas = 3.into();
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
                    if end > ALLOCATION_SANITY_LIMIT {
                        return Err(ExecutorError::InvalidAllocation(end).into());
                    }
                    let padding = 32 - end % 32;
                    evm.memory.resize(end + padding % 32, 0);
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
                    if end > ALLOCATION_SANITY_LIMIT {
                        return Err(ExecutorError::InvalidAllocation(end).into());
                    }
                    let padding = 32 - end % 32;
                    evm.memory.resize(end + padding % 32, 0);
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
                    if offset + 1 > ALLOCATION_SANITY_LIMIT {
                        return Err(ExecutorError::InvalidAllocation(offset + 1).into());
                    }
                    let padding = 32 - (offset + 1) % 32;
                    evm.memory.resize(offset + 1 + padding % 32, 0);
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
                let is_warm = ext.is_storage_warm(&this, &key);
                if !is_warm {
                    ext.warm_storage(&this, &key);
                }
                let val = evm.get(ext, &this, &key).await?;
                evm.push(val)?;
                evm.state.push(StateTouch::Get(this, key, val, is_warm));
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

                self.debug["SLOAD"] = json!({
                    "address": this,
                    "key": key,
                    "val": val,
                    "is_warm": is_warm,
                });
            }
            0x55 => {
                // SSTORE
                if matches!(ctx.call_type, CallType::Static) {
                    return Err(ExecutorError::StaticCallViolation(opcode).into());
                }

                let key = evm.pop()?;
                let new = evm.pop()?;

                // EIP-2200: Check gas stipend (must have at least 2300 gas remaining)
                if evm.gas.remaining() <= 2300 {
                    self.tracer.push(Event {
                        depth: ctx.depth,
                        reverted: false,
                        data: EventData::OpCode {
                            pc: instruction.offset,
                            op: instruction.opcode.code,
                            name: instruction.opcode.name(),
                            data: instruction.argument.clone().map(Into::into),
                            gas_cost: 0,
                            gas_used: evm.gas.used,
                            gas_back: 0,
                            gas_left: evm.gas.remaining(),
                            stack: evm.stack.clone(),
                            memory: evm.memory.chunks(32).map(Word::from_bytes).collect(),
                            debug: json!({
                                "is_call": false,
                                "gas_left": evm.gas.remaining(),
                                "gas_cost": 0,
                                "evm.gas.used": evm.gas.used,
                                "evm.gas.back": evm.gas.refund,
                            }),
                        },
                    });
                    evm.error(ExecutorError::OutOfGas().into())?;
                }

                let is_warm = ext.is_storage_warm(&this, &key);
                if !is_warm {
                    ext.warm_storage(&this, &key);
                }

                let val = evm.get(ext, &this, &key).await?;
                let original = ext.original.get(&(this, key)).cloned().unwrap_or_default();

                evm.put(ext, &this, key, new).await?;

                /*
                new: value from the stack input.
                val: current value of the storage slot.
                original: value of the storage slot before the current transaction.

                if new == val
                    100
                else if val == original
                    if original == 0
                        20000
                    else
                        2900
                else
                    100
                */

                // Calculate gas cost according to EIP-2200
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

                /*
                new: value from the stack input.
                val: current value of the storage slot.
                original: value of the storage slot before the current transaction.

                if new != val
                    if val == original
                        if original != 0 and new == 0
                            gas_refunds += 4800
                    else
                        if original != 0
                            if val == 0
                                gas_refunds -= 4800
                            else if new == 0
                                gas_refunds += 4800
                        if new == original
                            if original == 0
                                gas_refunds += 20000 - 100
                            else
                                gas_refunds += 5000 - 2100 - 100
                 */

                // Calculate gas refunds according to EIP-2200
                let mut refund_traces = Vec::new();
                let mut gas_refund = 0i64;
                if new != val {
                    if val == original {
                        if !original.is_zero() && new.is_zero() {
                            refund_traces.push("+4800");
                            gas_refund += 4800;
                        }
                    } else {
                        if !original.is_zero() {
                            if val.is_zero() {
                                refund_traces.push("-4800");
                                gas_refund -= 4800;
                            } else if new.is_zero() {
                                refund_traces.push("+4800");
                                gas_refund += 4800;
                            }
                        }
                        if new == original {
                            if original.is_zero() {
                                refund_traces.push("+19900");
                                gas_refund += 20_000 - 100;
                            } else {
                                refund_traces.push("+2800");
                                gas_refund += 5000 - 2100 - 100;
                            }
                        }
                    }
                }

                self.debug["sstore"] = json!({
                    "is_warm": is_warm,
                    "original": original,
                    "key": key,
                    "val": val,
                    "new": new,
                    "gas_cost": gas_cost,
                    "gas_back": gas_refund,
                    "refund": refund_traces
                        .into_iter()
                        .map(ToOwned::to_owned)
                        .map(serde_json::Value::from)
                        .collect::<Vec<_>>()
                });

                evm.gas.refund(gas_refund);
                gas = gas_cost.into();
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
                    .push(StateTouch::Put(this, key, val, new, is_warm));
            }
            0x56 => {
                // JUMP
                let dest = evm.pop()?.as_usize();
                let dest = code.resolve_jump(dest).unwrap_or(dest);
                if code.instructions[dest].opcode.code != 0x5b && dest != 0 {
                    evm.error(ExecutorError::InvalidJump(code.instructions[dest].opcode.code).into())?;
                }
                evm.pc = dest;
                pc_increment = false;
                gas = 8.into();
            }
            0x57 => {
                // JUMPI
                let dest = evm.pop()?.as_usize();
                let dest = code.resolve_jump(dest).unwrap_or(dest);
                let cond = evm.pop()?;
                if !cond.is_zero() {
                    if code.instructions[dest].opcode.code != 0x5b && dest != 0 {
                        evm.error(ExecutorError::InvalidJump(code.instructions[dest].opcode.code).into())?;
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
                let val = (evm.gas.remaining() - 2) as u64;
                evm.push(val.into())?;
                gas = 2.into();
            }
            0x5b => {
                // JUMPDEST: noop, a valid destination for JUMP/JUMPI
                gas = 1.into();
            }
            0x5c => {
                // TLOAD
                let key = evm.pop()?;
                let val = ext.transient.get(&(this, key)).copied().unwrap_or_default();
                evm.push(val)?;
                gas = 100.into();
            }
            0x5d => {
                // TSTORE
                let key = evm.pop()?;
                let val = evm.pop()?;
                ext.transient.insert((this, key), val);
                gas = 100.into();
            }
            0x5e => {
                // MCOPY
                let dest_offset = evm.pop()?.as_usize();
                let offset = evm.pop()?.as_usize();
                let size = evm.pop()?.as_usize();
                if dest_offset + size > evm.memory.len() {
                    if dest_offset + size > ALLOCATION_SANITY_LIMIT {
                        return Err(ExecutorError::InvalidAllocation(dest_offset + size).into());
                    }
                    let padding = 32 - (dest_offset + size) % 32;
                    evm.memory.resize(dest_offset + size + padding % 32, 0);
                }
                let mut buffer = vec![0u8; size];
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
                    evm.error(ExecutorError::StackUnderflow.into())?;
                }
                let val = evm.stack[evm.stack.len() - n];
                evm.push(val)?;
                gas = 3.into();
            }

            0x90..=0x9f => {
                // SWAP1..SWAP16
                let n = instruction.opcode.n as usize;
                if evm.stack.len() <= n {
                    evm.error(ExecutorError::StackUnderflow.into())?;
                }
                let stack_len = evm.stack.len();
                evm.stack.swap(stack_len - 1, stack_len - 1 - n);
                gas = 3.into();
            }

            0xa0..=0xa4 => {
                // LOG0..LOG4
                if matches!(ctx.call_type, CallType::Static) {
                    return Err(ExecutorError::StaticCallViolation(opcode).into());
                }
                let n = instruction.opcode.n as usize;
                let offset = evm.pop()?.as_usize();
                let size = evm.pop()?.as_usize();

                let mut topics = Vec::with_capacity(n);
                for _ in 0..n {
                    topics.push(evm.pop()?);
                }
                topics.reverse();

                let data = if offset + size > evm.memory.len() {
                    let mut data = evm.memory.clone();
                    data.resize(offset + size, 0);
                    data
                } else {
                    evm.memory[offset..offset + size].to_vec()
                };
                evm.logs.push(Log(this, topics, data));

                gas = 375.into();
                gas += (375 * n + 8 * size).into();
                gas += evm.memory_expansion_cost();
            }

            0xf0 => {
                // CREATE
                if matches!(ctx.call_type, CallType::Static) {
                    return Err(ExecutorError::StaticCallViolation(opcode).into());
                }
                self.create(instruction, this, call, &mut gas, evm, ext, ctx)
                    .await?;
            }
            0xf1 => {
                // CALL
                if matches!(ctx.call_type, CallType::Static) {
                    let value = evm
                        .stack
                        .iter()
                        .rev()
                        .nth(2)
                        .ok_or(ExecutorError::StackUnderflow)?;
                    if !value.is_zero() {
                        return Err(ExecutorError::StaticCallViolation(opcode).into());
                    }
                }
                let ctx = Context {
                    call_type: CallType::Call,
                    ..ctx
                };
                self.call(instruction, this, call, &mut gas, evm, ext, ctx)
                    .await
                    .with_context(|| "opcode: CALL")?;
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
                self.call(instruction, this, call, &mut gas, evm, ext, ctx)
                    .await?;
            }
            0xf3 | 0xfd => {
                // RETURN
                // REVERT
                evm.stopped = true;
                evm.reverted = opcode == 0xfd;

                let offset = evm.pop()?.as_usize();
                let size = evm.pop()?.as_usize();

                if size > 0 {
                    if offset + size > evm.memory.len() {
                        if offset + size > ALLOCATION_SANITY_LIMIT {
                            return Err(ExecutorError::InvalidAllocation(offset + size).into());
                        }
                        let padding = 32 - (offset + size) % 32;
                        evm.memory.resize(offset + size + padding % 32, 0);
                    }
                    self.ret = evm.memory[offset..offset + size].to_vec();
                } else {
                    self.ret.clear();
                }
                gas = evm.memory_expansion_cost();

                self.tracer.push(Event {
                    data: EventData::Return {
                        ok: !evm.reverted,
                        data: self.ret.clone().into(),
                        error: if evm.reverted {
                            decode_error_string(&self.ret)
                        } else {
                            None
                        },
                        gas_used: evm.gas.used,
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
                self.call(instruction, this, call, &mut gas, evm, ext, ctx)
                    .await?;
            }
            0xf5 => {
                // CREATE2
                if matches!(ctx.call_type, CallType::Static) {
                    return Err(ExecutorError::StaticCallViolation(opcode).into());
                }
                let ctx = Context {
                    call_type: CallType::Create2,
                    ..ctx
                };
                self.create(instruction, this, call, &mut gas, evm, ext, ctx)
                    .await?;
            }
            0xfa => {
                // STATICCALL
                let ctx = Context {
                    call_type: CallType::Static,
                    ..ctx
                };
                self.call(instruction, this, call, &mut gas, evm, ext, ctx)
                    .await?;
            }
            0xfe => {
                // INVALID: handled outside of `execute_instruction`
            }
            0xff => {
                // SELFDESTRUCT
                if matches!(ctx.call_type, CallType::Static) {
                    return Err(ExecutorError::StaticCallViolation(opcode).into());
                }
                todo!("SELFDESTRUCT");
            }
            _ => {
                return Err(ExecutorError::UnknownOpcode(opcode).into());
            }
        }

        if pc_increment {
            evm.pc += 1;
        }

        Ok(gas)
    }

    #[allow(clippy::too_many_arguments)]
    async fn call(
        &mut self,
        instruction: &Instruction,
        this: Address,
        call: &Call,
        gas: &mut Word,
        evm: &mut Evm,
        ext: &mut Ext,
        ctx: Context,
    ) -> eyre::Result<()> {
        let call_gas = evm.pop()?.min(i64::MAX.into()); // avoid possible i64 overflow
        let address: Address = (&evm.pop()?).into();
        let value = if !matches!(ctx.call_type, CallType::Static | CallType::Delegate) {
            evm.pop()?
        } else if matches!(ctx.call_type, CallType::Static) {
            Word::zero() // STATICCALL always has value = 0
        } else {
            call.value // DELEGATECALL inherits value from parent
        };
        let args_offset = evm.pop()?.as_usize();
        let args_size = evm.pop()?.as_usize();
        let ret_offset = evm.pop()?.as_usize();
        let ret_size = evm.pop()?.as_usize();

        // Handle memory expansion for arguments and return data
        let size = (args_offset + args_size).max(ret_offset + ret_size);
        if (ret_size > 0 || args_size > 0) && size > evm.memory.len() {
            if size > ALLOCATION_SANITY_LIMIT {
                return Err(ExecutorError::InvalidAllocation(size).into());
            }
            let size = size.div_ceil(32) * 32;
            evm.memory.resize(size, 0);
        }
        let memory_expansion_cost = evm.memory_expansion_cost().as_i64();

        let mut create_cost = 0;
        let is_empty = !precompiles::is_precompile(&address) && ext.is_empty(&address).await?;
        if !value.is_zero() && is_empty {
            create_cost = 25000; // account creation cost
        }

        // Calculate address access cost (EIP-2929)
        let (code, codehash) = ext.code(&address).await?;
        let mut access_cost = evm.address_access_cost(&address, ext).as_i64();

        // Check and resolve delegation: CODE = <0xef0100> + <20 bytes address>
        let code = if code.len() == 23 && code.starts_with(&[0xef, 0x01, 0x00]) {
            let target = Address::try_from(&code[3..]).expect("address");
            // eprintln!("DEBUG: delegation {} -> {}", address, target);
            access_cost += evm.address_access_cost(&target, ext).as_i64();
            let (code, _) = ext.code(&target).await?;
            code
        } else {
            code
        };

        // Calculate base gas cost
        let mut base_gas_cost = access_cost + memory_expansion_cost + create_cost;

        let mut gas_stipend_adjustment = 0;

        // Add value transfer cost if applicable (not for DELEGATECALL/STATICCALL)
        if !matches!(ctx.call_type, CallType::Static | CallType::Delegate) && !value.is_zero() {
            base_gas_cost += 9000; // value transfer cost
            gas_stipend_adjustment = 2300;
        }

        // Calculate available gas for forwarding using "all but one 64th" rule
        let remaining_gas = evm.gas.remaining().saturating_sub(base_gas_cost);
        let all_but_one_64th = remaining_gas - remaining_gas / 64;
        let gas_to_forward = call_gas.as_i64().min(all_but_one_64th) + gas_stipend_adjustment;

        // For EVM accounting: only charge the outer EVM for base cost
        // (forwarded gas was already "spent" by allocating it to inner call)
        *gas = base_gas_cost.unsigned_abs().into();

        // For tracing: report the total cost including forwarded gas (to match REVM)
        let total_gas_cost_for_tracing = base_gas_cost + gas_to_forward - gas_stipend_adjustment;

        let data = if args_size > 0 {
            &evm.memory[args_offset..args_offset + args_size]
        } else {
            &[]
        };

        // Handle precompile call
        if precompiles::is_precompile(&address) {
            let gas_cost = precompiles::gas_cost(&address, data);
            let result = match precompiles::execute(&address, data) {
                Ok(ret) => {
                    self.ret = ret;
                    Word::one()
                }
                Err(_) => Word::zero(),
            };

            self.tracer.push(Event {
                depth: ctx.depth,
                reverted: false,
                data: EventData::OpCode {
                    pc: instruction.offset,
                    op: instruction.opcode.code,
                    name: instruction.opcode.name(),
                    data: instruction.argument.clone().map(Into::into),
                    gas_cost: total_gas_cost_for_tracing,
                    gas_used: evm.gas.used + total_gas_cost_for_tracing,
                    gas_left: evm.gas.remaining() - total_gas_cost_for_tracing,
                    stack: evm.stack.clone(),
                    memory: evm.memory.chunks(32).map(Word::from_bytes).collect(),
                    gas_back: 0,
                    debug: json!({
                        "is_call": true,
                        "is_precompile": true,
                        "gas_left": evm.gas.remaining() - base_gas_cost,
                        "gas_cost": total_gas_cost_for_tracing,
                        "evm.gas.used": evm.gas.used,
                        "evm.gas.refund": evm.gas.refund,
                        "call.address": address,
                        "call.input": hex::encode(&evm.memory[args_offset..args_offset + args_size]),
                        "call.result": result,
                        "call.gas": call_gas.as_u64(),
                        "access_cost": access_cost,
                        "precompile_gas_cost": gas_cost,
                        "memory_expansion_cost": memory_expansion_cost,
                        "ret": hex::encode(&self.ret),
                    }),
                },
            });

            let copy_len = self.ret.len().min(ret_size);
            evm.memory[ret_offset..ret_offset + copy_len].copy_from_slice(&self.ret[..copy_len]);

            evm.push(result)?;
            evm.gas(gas_cost)?;
            return Ok(());
        }

        let inner_call = Call {
            data: data.to_vec(),
            value,
            from: if matches!(ctx.call_type, CallType::Delegate | CallType::Callcode) {
                call.from
            } else {
                this
            },
            to: if matches!(ctx.call_type, CallType::Delegate | CallType::Callcode) {
                this
            } else {
                address
            },
            gas: (gas_to_forward as u64).into(),
        };
        let mut inner_evm = Evm {
            gas: Gas::new(gas_to_forward),
            ..Default::default()
        };

        // Apply value transfer BEFORE call execution
        let mut transferred = false;
        let sender_balance = ext.balance(&this).await?;
        let receiver_balance = ext.balance(&address).await?;
        if !value.is_zero() && !matches!(ctx.call_type, CallType::Static | CallType::Delegate) {
            if sender_balance >= value {
                // For self-calls (where sender == receiver), no net balance change
                if this != address {
                    ext.account_mut(&this).value = sender_balance - value;
                    ext.account_mut(&address).value = receiver_balance + value;
                    transferred = true;
                }
            } else {
                // TODO: insufficient funds to transfer
            }
        }

        let inner_ctx = Context {
            depth: ctx.depth + 1,
            ..ctx
        };

        let code = Decoder::decode(code);
        self.tracer.push(Event {
            data: EventData::Account(AccountEvent::GetCode {
                address,
                codehash,
                bytecode: code.bytecode.clone().into(),
            }),
            depth: ctx.depth,
            reverted: false,
        });

        let mut executor =
            Executor::<T>::with_tracer(self.tracer.fork()).with_header(self.header.clone());
        executor.set_log(self.log);
        let future =
            executor.execute_with_context(&code, &inner_call, &mut inner_evm, ext, inner_ctx);
        let (tracer, ret) = Box::pin(future).await;

        // HERE: TODO: remove this label
        self.tracer.push(Event {
            depth: ctx.depth,
            reverted: false,
            data: EventData::OpCode {
                pc: instruction.offset,
                op: instruction.opcode.code,
                name: instruction.opcode.name(),
                data: instruction.argument.clone().map(Into::into),
                stack: evm.stack.clone(),
                memory: evm.memory.chunks(32).map(Word::from_bytes).collect(),
                gas_back: 0,
                gas_cost: total_gas_cost_for_tracing,
                gas_used: evm.gas.used + total_gas_cost_for_tracing,
                gas_left: evm.gas.remaining() - total_gas_cost_for_tracing,
                debug: json!({
                    "is_call": true,
                    "gas_left": evm.gas.remaining() - base_gas_cost,
                    "gas_cost": total_gas_cost_for_tracing,
                    "evm.gas.used": evm.gas.used,
                    "evm.gas.refund": evm.gas.refund,
                    "args_offset": args_offset,
                    "args_size": args_size,
                    "ret_offset": ret_offset,
                    "ret_size": ret_size,
                    "memory.len": evm.memory.len(),
                    "call.from": inner_call.from,
                    "call.to": inner_call.to,
                    "call.input": hex::encode(data),
                    "call.value": value,
                    "call.gas": call_gas.as_u64(),
                    "access_cost": access_cost,
                    "inner_evm.reverted": inner_evm.reverted,
                    "is_empty": is_empty,
                    "code.len": code.bytecode.len(),
                    // "code.hex": hex::encode(&code.bytecode),
                    "ret": hex::encode(&self.ret),
                }),
            },
        });
        self.tracer.join(tracer, inner_evm.reverted);

        let copy_len = ret.len().min(ret_size);
        if copy_len > 0 {
            evm.memory[ret_offset..ret_offset + copy_len].copy_from_slice(&ret[..copy_len]);
        }

        if inner_evm.reverted {
            // When call reverts, charge the gas that was used
            evm.gas.used += inner_evm.gas.used;
            evm.gas.used -= gas_stipend_adjustment;
            // Don't add refunds from reverted calls
            evm.refund = evm.gas.refund;
            self.ret = ret;
            evm.push(Word::zero())?;
            inner_evm.revert(ext).await?;
            // Revert the value transfer if it was performed
            if transferred {
                ext.account_mut(&this).value = sender_balance;
                ext.account_mut(&address).value = receiver_balance;
            }
            return Ok(());
        }

        // Call succeeded: charge actual gas used
        evm.gas.used += inner_evm.gas.used;
        evm.gas.used -= gas_stipend_adjustment;

        // Only add refunds if call succeeded
        evm.gas.refund += inner_evm.gas.refund;
        evm.refund = evm.gas.refund;

        evm.account.extend(inner_evm.account.into_iter());
        evm.state.extend(inner_evm.state.into_iter());

        // Preserve the actual return data as-is for RETURNDATA* opcodes
        self.ret = ret;
        evm.push(Word::one())?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn create(
        &mut self,
        instruction: &Instruction,
        this: Address,
        _call: &Call,
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

        // HERE: TODO: apply proper create costs (Create: Scenario 2 - From-Code Create)

        if offset + size > evm.memory.len() {
            if offset + size > ALLOCATION_SANITY_LIMIT {
                return Err(ExecutorError::InvalidAllocation(offset + size).into());
            }
            let padding = 32 - (offset + size) % 32;
            evm.memory.resize(offset + size + padding % 32, 0);
        }

        let memory_expansion_cost = evm.memory_expansion_cost().as_i64();

        let bytecode = evm.memory[offset..offset + size].to_vec();
        let word_size = bytecode.len().div_ceil(32) as i64;
        let init_code_cost = 2 * word_size + 
            if matches!(ctx.call_type, CallType::Create2) {  
                6 * word_size 
            } else { 
                0 
            };
        let code = Decoder::decode(bytecode);

        let nonce = ext.nonce(&this).await?;
        let created = if !matches!(ctx.call_type, CallType::Create2) {
            this.create(nonce)
        } else {
            // (See: https://www.evm.codes/?fork=cancun#f5)
            // initialisation_code = memory[offset:offset+size]
            // address = keccak256(0xff + sender_address + salt + keccak256(initialisation_code))[12:]
            let mut buffer = Vec::with_capacity(1 + 20 + 32 + 32);
            buffer.push(0xffu8);
            buffer.extend_from_slice(&this.0);  // Use 'this' (current contract), not call.from
            buffer.extend_from_slice(&salt.into_bytes());
            buffer.extend_from_slice(&keccak256(&code.bytecode));  // Use raw bytecode from memory
            let mut hash = keccak256(&buffer);
            hash[0..12].copy_from_slice(&[0u8; 12]);
            Address::from(&Word::from_bytes(&hash))
        };

        ext.warm_address(&created);
        evm.account.push(AccountTouch::WarmUp(created));

        let create_cost = 32000;
        let base_gas_cost = memory_expansion_cost + create_cost + init_code_cost;
        let remaining_gas = evm.gas.remaining().saturating_sub(base_gas_cost);
        let all_but_one_64th = remaining_gas - remaining_gas / 64;
        let gas_to_forward = all_but_one_64th;

        let inner_call = Call {
            data: vec![],
            value,
            from: this,
            to: Address::zero(),
            gas: gas_to_forward.unsigned_abs().into(),
        };
        let mut inner_evm = Evm {
            gas: Gas::new(gas_to_forward),
            ..Default::default()
        };
        let inner_ctx = Context {
            created,
            depth: ctx.depth + 1,
            ..ctx
        };
        let mut executor =
            Executor::<T>::with_tracer(self.tracer.fork()).with_header(self.header.clone());
        executor.set_log(self.log);
        let future =
            executor.execute_with_context(&code, &inner_call, &mut inner_evm, ext, inner_ctx);
        let (tracer, code) = Box::pin(future).await;

        let deployed_code_cost = if !inner_evm.reverted {
            200 * code.len() as i64
        } else {
            0
        };
        let base_gas_cost =
            memory_expansion_cost + create_cost + init_code_cost + deployed_code_cost;

        // For tracing: report the total cost including gas forwarded (to match REVM)
        // REVM reports the forwarded gas, not the used gas, because from the outer EVM's
        // perspective, all forwarded gas is "spent" even if the inner execution didn't use it all
        // Note: deployed_code_cost is NOT included in the forwarded gas calculation
        let base_cost_without_deployed = memory_expansion_cost + create_cost + init_code_cost;
        let total_gas_cost_for_tracing = base_cost_without_deployed + gas_to_forward;

        // HERE: TODO: remove this label
        self.tracer.push(Event {
            depth: ctx.depth,
            reverted: false,
            data: EventData::OpCode {
                pc: instruction.offset,
                op: instruction.opcode.code,
                name: instruction.opcode.name(),
                data: instruction.argument.clone().map(Into::into),
                stack: evm.stack.clone(),
                memory: evm.memory.chunks(32).map(Word::from_bytes).collect(),
                gas_back: 0,
                gas_cost: total_gas_cost_for_tracing,
                gas_used: evm.gas.used + total_gas_cost_for_tracing,
                gas_left: evm.gas.remaining() - total_gas_cost_for_tracing,
                debug: json!({
                    "is_call": true,
                    "evm.gas.used": evm.gas.used,
                    "evm.gas.refund": evm.gas.refund,
                    "call.created": created,
                    "call.value": value,
                    "inner_evm.reverted": inner_evm.reverted,
                    "inner_call": inner_call,
                }),
            },
        });

        self.tracer.join(tracer, inner_evm.reverted);

        evm.gas.used += base_gas_cost;
        evm.gas.used += inner_evm.gas.used;
        *gas = Word::zero();

        if inner_evm.reverted {
            inner_evm.revert(ext).await?;
            evm.push(Word::zero())?;
            return Ok(());
        }

        let hash = keccak256(&code);
        let (old_code, old_hash) = ext.code(&created).await?;
        *ext.code_mut(&created) = (code.clone(), Word::from_bytes(&hash));
        let nonce = ext.account_mut(&this).nonce;
        ext.account_mut(&this).nonce += Word::one();
        evm.account.push(AccountTouch::SetNonce(
            this,
            nonce.as_u64(),
            nonce.as_u64() + 1,
        ));
        self.tracer.push(Event {
            data: EventData::Account(AccountEvent::SetNonce {
                address: this,
                val: nonce.as_u64(),
                new: nonce.as_u64() + 1,
            }),
            depth: ctx.depth,
            reverted: false,
        });
        evm.account.push(AccountTouch::SetCode(
            created,
            (old_hash, old_code),
            (Word::from_bytes(&hash), code.clone()),
        ));
        self.tracer.push(Event {
            data: EventData::Account(AccountEvent::SetCode {
                address: created,
                codehash: Word::from_bytes(&hash),
                bytecode: code.into(),
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
        evm.push((&created).into())?;

        Ok(())
    }
}
