use eyre::Context as _;
use i256::I256;
use serde_json::json;
use thiserror::Error;

use crate::{
    common::{
        address::Address,
        block::Header,
        call::Call,
        hash::{empty, keccak256},
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
    #[error("Invalid jump to offset: {0}")]
    InvalidJump(usize),
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

// 2 MB: opinionated allocation sanity check limit
const ALLOCATION_SANITY_LIMIT: usize = 2 * 1024 * 1024;

#[derive(Debug, Default, Eq, PartialEq)]
pub enum AccountTouch {
    #[default]
    Noop,
    WarmUp(Address),

    GetNonce(Address, u64),
    GetValue(Address, Word),
    GetCode(Address, Word, Vec<u8>),
    GetState(Address, Word, Word, bool),

    SetNonce(Address, u64, u64),
    SetValue(Address, Word, Word),
    SetState(Address, Word, Word, Word, bool),

    SetTransientState(Address, Word, Word, Word),

    Create(Address, Word, Word, Vec<u8>, Word),
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

    pub logs: Vec<Log>,
    pub touches: Vec<AccountTouch>,

    pub mem_cost: i64,
    pub refund: i64,
}

impl Evm {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn memory_expansion_cost(&mut self) -> i64 {
        let mem_len = self.memory.len().div_ceil(32) as i64;
        let mem_cost = (mem_len * mem_len) / 512 + (3 * mem_len);
        let exp_cost = mem_cost - self.mem_cost;
        self.mem_cost = mem_cost;
        exp_cost
    }

    pub(crate) fn address_access_cost(&mut self, address: &Address, ext: &mut Ext) -> i64 {
        // EIP-2929: Check if address has been accessed during this transaction
        if precompiles::is_precompile(address) {
            return 100;
        }
        let is_warm = ext.is_address_warm(address);
        if !is_warm {
            ext.warm_address(address);
            self.touches.push(AccountTouch::WarmUp(*address));
        }
        if is_warm {
            100 // warm access
        } else {
            2600 // cold access
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
                .map(|_| Word::zero()) // just for typechecker
        }
    }

    // Usage: let [a, b, c] = self.peek(); // no need to provide N
    pub fn peek<const N: usize>(&mut self) -> eyre::Result<[Word; N]> {
        if self.stack.len() < N {
            self.error(ExecutorError::StackUnderflow.into())?;
        }
        let mut ret = [Word::zero(); N];
        let mut i = 0;
        while i < N {
            ret[i] = self.stack[self.stack.len() - 1 - i];
            i += 1;
        }
        Ok(ret)
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
        for t in self.touches.iter().rev() {
            match t {
                AccountTouch::SetState(address, key, val, _, is_warm) => {
                    // Always restore the storage value, regardless of warm/cold status
                    // Directly set storage without calling get() to avoid polluting ext.original
                    ext.state
                        .entry(*address)
                        .or_default()
                        .state
                        .insert(*key, *val);
                    if !*is_warm {
                        ext.accessed_storage.remove(&(*address, *key));
                    }
                }
                AccountTouch::SetTransientState(address, key, val, _) => {
                    ext.transient.insert((*address, *key), *val);
                }
                AccountTouch::GetState(address, key, _, is_warm) => {
                    if !*is_warm {
                        ext.accessed_storage.remove(&(*address, *key));
                    }
                }
                AccountTouch::SetNonce(addr, val, _new) => {
                    ext.account_mut(addr).nonce = (*val).into();
                }
                AccountTouch::SetValue(addr, val, _new) => {
                    ext.account_mut(addr).value = *val;
                }
                AccountTouch::Create(addr, _value, _nonce, _code, _hash) => {
                    *ext.code_mut(addr) = (vec![], Word::from_bytes(&empty()));
                    ext.account_mut(addr).nonce = Word::zero();
                    ext.account_mut(addr).value = Word::zero();
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

pub enum StepResult {
    Ok(i64),
    Halt(i64),
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
        // EIP-2929: Pre-warm sender and target addresses at transaction start
        ext.warm_address(&call.from);
        evm.touches.push(AccountTouch::WarmUp(call.from));
        if !call.to.is_zero() {
            ext.warm_address(&call.to);
            evm.touches.push(AccountTouch::WarmUp(call.to));

            // EIP-7702: If call.to is delegated, also pre-warm the target address
            let (code, _) = ext.code(&call.to).await?;
            if code.len() == 23 && code.starts_with(&[0xef, 0x01, 0x00]) {
                let target = Address::try_from(&code[3..]).expect("must succeed");
                ext.warm_address(&target);
                evm.touches.push(AccountTouch::WarmUp(target));
            }
        }

        // EIP-3651 (Shanghai): Pre-warm coinbase address
        let coinbase = self.header.miner;
        if !coinbase.is_zero() {
            ext.warm_address(&coinbase);
            evm.touches.push(AccountTouch::WarmUp(coinbase));
        }

        // Deduct upfront gas payment before execution
        let gas_prepayment = call.gas * ext.tx_ctx.gas_price;
        let sender_balance = ext.balance(&call.from).await?;
        if !gas_prepayment.is_zero() {
            let updated_balance = sender_balance.saturating_sub(gas_prepayment);
            ext.account_mut(&call.from).value = updated_balance;
            evm.touches.push(AccountTouch::SetValue(
                call.from,
                sender_balance,
                updated_balance,
            ));
        }
        // Note: Gas prepayment is deducted from sender but NOT transferred to coinbase yet.
        // The base fee will be burned (removed from circulation), and only the priority fee
        // will be transferred to the coinbase after execution completes (see below).

        let access_list_cost = ext.tx_ctx.access_list_cost();

        let mut gas = call.gas.as_i64();
        let call_cost = 21000;
        gas -= call_cost;
        gas -= access_list_cost;
        ext.apply_access_list();

        let data_cost = {
            let total_calldata_len = call.data.len();
            let nonzero_bytes_count = call.data.iter().filter(|byte| byte != &&0).count();
            nonzero_bytes_count * 16 + (total_calldata_len - nonzero_bytes_count) * 4
        } as i64;
        gas -= data_cost;

        evm.gas = Gas::new(gas);

        // TODO: sort out value transfer!
        let src = ext.balance(&call.from).await?;
        let dst = ext.balance(&call.to).await?;
        if !call.value.is_zero() && !call.to.is_zero() {
            let value = ext.account_mut(&call.from).value;
            // Avoid strict balance checks for now
            let updated = value.saturating_sub(call.value);
            ext.account_mut(&call.from).value = updated;
            evm.touches
                .push(AccountTouch::SetValue(call.from, src, updated));
            self.tracer.push(Event {
                data: EventData::Account(AccountEvent::SetValue {
                    address: call.from,
                    val: src,
                    new: updated,
                }),
                depth: 1,
                reverted: false,
            });

            ext.account_mut(&call.to).value += call.value;
            evm.touches
                .push(AccountTouch::SetValue(call.to, dst, dst + call.value));
            self.tracer.push(Event {
                data: EventData::Account(AccountEvent::SetValue {
                    address: call.to,
                    val: src,
                    new: src + call.value,
                }),
                depth: 1,
                reverted: false,
            });
        }

        let nonce = ext.nonce(&call.from).await?;
        ext.account_mut(&call.from).nonce = nonce + Word::one();
        evm.touches.push(AccountTouch::SetNonce(
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
            depth: 1,
            reverted: false,
        });

        let is_transfer_only =
            code.bytecode.is_empty() && call.data.is_empty() && !call.to.is_zero();
        if is_transfer_only {
            evm.stopped = true;
            evm.reverted = false;
            // return Ok((self.tracer, vec![]));
        }

        let created = call.from.create(nonce);
        let ctx = Context {
            created,
            origin: call.from,
            depth: 1,
            ..Context::default()
        };

        // Store base_fee and header info for later use (before moving self.header)
        let base_fee = self.header.base_fee;
        let header_clone = self.header.clone();

        let tracer = self.tracer.fork();
        let mut executor = Executor::<T>::with_tracer(tracer).with_header(self.header);
        executor.set_log(self.log);
        let (tracer, ret) = executor
            .execute_with_context(code, call, evm, ext, ctx)
            .await;
        self.tracer.join(tracer, evm.reverted);

        if evm.reverted {
            evm.revert(ext).await?;
        }

        // Calculate gas costs and transaction fee

        let create_cost = 32000i64;
        let init_code_cost = 2 * call.data.len().div_ceil(32) as i64;

        // EIP-7623: Increase calldata cost
        let calldata_tokens = {
            let zero_bytes = call.data.iter().filter(|b| **b == 0).count() as i64;
            let nonzero_bytes = call.data.len() as i64 - zero_bytes;
            zero_bytes + nonzero_bytes * 4
        };
        let gas_floor = call_cost + 10 * calldata_tokens;

        let gas_costs = if !call.to.is_zero() {
            call_cost + data_cost + access_list_cost
        } else {
            let deployed_code_cost = 200 * ret.len() as i64;
            call_cost
                + data_cost
                + create_cost
                + init_code_cost
                + deployed_code_cost
                + access_list_cost
        };

        let gas_final = evm.gas.finalized(gas_costs, evm.reverted).max(gas_floor);
        let gas_used_fee = Word::from(gas_final) * ext.tx_ctx.gas_price;
        let gas_refund = gas_prepayment - gas_used_fee;

        // Refund unused gas to sender
        if !gas_refund.is_zero() {
            let src = ext.balance(&call.from).await?;
            let new_balance = src + gas_refund;
            ext.account_mut(&call.from).value = new_balance;
            evm.touches
                .push(AccountTouch::SetValue(call.from, src, new_balance));
        }

        // Transfer priority fee to coinbase (base fee is burned per EIP-1559)
        // Match revm's calculation: recalculate effective_gas_price from max_fee and max_priority
        // then coinbase_gas_price = effective_gas_price - basefee
        // Note: EIP-4844 blob transactions pay BOTH regular gas fees AND blob fees to coinbase
        // Note: Gas fees are charged regardless of whether the transaction succeeds or reverts
        if !coinbase.is_zero() {
            // Effective priority to coinbase = min(max_priority, max_fee - base_fee) = min(10.01, 30.03 - 0.665752928) ≈ 10.01 Gwei
            let coinbase_gas_price = if ext.tx_ctx.gas_max_priority_fee.is_zero() {
                // Legacy transaction (type 0): gas_price - base_fee goes to miner (base fee is burned)
                // COINBASE BALANCE fix 1/1: legacy tx effective gas price
                ext.tx_ctx.gas_price.saturating_sub(base_fee)
            } else {
                // EIP-1559 transaction (type 2): recalculate effective_gas_price like revm does
                // effective_gas_price = min(max_fee_per_gas, base_fee + max_priority_fee_per_gas)
                let effective_gas_price = {
                    let base_plus_priority = base_fee + ext.tx_ctx.gas_max_priority_fee;
                    Word::min(ext.tx_ctx.gas_max_fee, base_plus_priority)
                };
                // coinbase_gas_price = effective_gas_price - basefee
                effective_gas_price.saturating_sub(base_fee)
            };

            let priority_fee_total = Word::from(gas_final) * coinbase_gas_price;

            if !priority_fee_total.is_zero() {
                // TODO: charge sender with `priority_fee_total`

                // Ensure coinbase is in state (it may not be if no code accessed it during execution)
                let current_coinbase_balance = ext.balance(&coinbase).await?;
                let new_coinbase_balance = current_coinbase_balance + priority_fee_total;
                ext.account_mut(&coinbase).value = new_coinbase_balance;
                // println!("[SOLE] COINBASE (GAS)  : {new_coinbase_balance:#x} *{priority_fee_total:#x}");
                // DO NOT add to evm.touches - fee additions are final and should never be reverted
                // COINBASE BALANCE fix 1/2: do not revert unreversible fee charge
                self.tracer.push(Event {
                    data: EventData::Account(AccountEvent::SetValue {
                        address: coinbase,
                        val: current_coinbase_balance,
                        new: new_coinbase_balance,
                    }),
                    depth: 1,
                    reverted: false,
                });
            }
        }

        // Add blob gas fees (EIP-4844) if this transaction used blob gas
        if ext.tx_ctx.blob_gas_used > 0 && !coinbase.is_zero() {
            let blob_gas_price = header_clone.blob_gas_price().min(ext.tx_ctx.blob_max_fee);
            let blob_fee = Word::from(ext.tx_ctx.blob_gas_used) * blob_gas_price;

            if !blob_fee.is_zero() {
                // TODO: charge sender with `blob_fee`

                // let current_coinbase_balance = ext.balance(&coinbase).await?;
                // println!("[SOLE] COINBASE (BLOB) : {current_coinbase_balance:#x} !{blob_fee:#x}");
                // DO NOT add to evm.touches - blob fees are final and should never be reverted
            }
        }

        // Emit fee event showing gas consumed
        self.tracer.push(Event {
            data: EventData::Fee {
                gas: Word::from(gas_final),
                price: ext.tx_ctx.gas_price,
                total: gas_used_fee,
            },
            depth: 1,
            reverted: false,
        });

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
                .await
            {
                Ok(result) => {
                    let (cost, halt) = match result {
                        StepResult::Ok(cost) => (cost, false),
                        StepResult::Halt(cost) => {
                            // let cost = evm.gas.remaining();
                            // evm.gas(cost).ok();
                            evm.stopped = true;
                            evm.reverted = true;
                            (cost, true)
                        }
                    };

                    let is_sstore = halt && instruction.opcode.name == "SSTORE";
                    if !instruction.is_call() && !is_sstore {
                        // HERE: TODO: remove this label
                        let charged_cost = cost.min(evm.gas.remaining());
                        let refund = evm.gas.refund - evm.refund;
                        evm.refund = evm.gas.refund;

                        self.tracer.push(Event {
                            depth: ctx.depth,
                            reverted: false,
                            data: EventData::OpCode {
                                pc: instruction.offset,
                                op: instruction.opcode.code,
                                name: instruction.opcode.name(),
                                data: instruction.argument.clone().map(Into::into),
                                gas_cost: charged_cost,
                                gas_used: evm.gas.used + charged_cost,
                                gas_back: refund,
                                gas_left: evm.gas.remaining() - charged_cost,
                                stack: evm.stack.clone(),
                                memory: evm.memory.chunks(32).map(Word::from_bytes).collect(),
                                debug: self.debug.take(),
                            },
                        });
                    }
                    if halt || instruction.opcode.code == 0xfe {
                        // INVALID opcode
                        // eprintln!("OPCODE INVALID: depth={} evm.pc={} op={}", ctx.depth, evm.pc, instruction.opcode.name());
                        evm.gas.sub(evm.gas.remaining()).expect("must succeed");
                        evm.stopped = true;
                        evm.reverted = true;
                        return (self.tracer, vec![]);
                    }
                    if evm.gas(cost).is_err() {
                        // out of gas
                        // eprintln!("OUT OF GAS: depth={} evm.pc={} op={}", ctx.depth, evm.pc, instruction.opcode.name());
                        evm.stopped = true;
                        evm.reverted = true;
                        return (self.tracer, vec![]);
                    }
                    if instruction.opcode.code == 0xff {
                        // SELFDESTRUCT opcode
                        return (self.tracer, vec![]);
                    }
                }
                Err(_) => {
                    // opcode failed
                    // eprintln!("OPCODE FAILED: depth={} evm.pc={} op={}", ctx.depth, evm.pc, instruction.opcode.name());
                    evm.stopped = true;
                    evm.reverted = true;

                    self.tracer.push(Event {
                        depth: ctx.depth,
                        reverted: true,
                        data: EventData::OpCode {
                            pc: instruction.offset,
                            op: instruction.opcode.code,
                            name: instruction.opcode.name(),
                            data: instruction.argument.clone().map(Into::into),
                            gas_cost: 0,
                            gas_used: evm.gas.used,
                            gas_back: evm.gas.refund - evm.refund,
                            gas_left: evm.gas.remaining(),
                            stack: evm.stack.clone(),
                            memory: evm.memory.chunks(32).map(Word::from_bytes).collect(),
                            debug: self.debug.take(),
                        },
                    });

                    evm.gas(evm.gas.remaining()).expect("must succeed");
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

        if !evm.stopped && !code.instructions.is_empty() {
            self.tracer.push(Event {
                depth: ctx.depth,
                reverted: false,
                data: EventData::OpCode {
                    pc: evm.pc,
                    op: 0x00,
                    name: "STOP".to_string(),
                    data: None,
                    gas_cost: 0,
                    gas_used: evm.gas.used,
                    gas_back: 0,
                    gas_left: evm.gas.remaining(),
                    stack: evm.stack.clone(),
                    memory: evm.memory.chunks(32).map(Word::from_bytes).collect(),
                    debug: self.debug.take(),
                },
            });
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
    ) -> eyre::Result<StepResult> {
        self.debug = json!({});
        let mut gas = 0i64;
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
                return Ok(StepResult::Ok(0));
            }
            // 0x01..0x0b: Arithmetic Operations
            0x01 => {
                // ADD
                gas = 3;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let a = evm.pop()?;
                let b = evm.pop()?;
                let (res, _) = a.overflowing_add(b);
                evm.push(res)?;
            }
            0x02 => {
                // MUL
                gas = 5;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let a = evm.pop()?;
                let b = evm.pop()?;
                let (res, _) = a.overflowing_mul(b);
                evm.push(res)?;
            }
            0x03 => {
                // SUB
                gas = 3;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let a = evm.pop()?;
                let b = evm.pop()?;
                let (res, _) = a.overflowing_sub(b);
                evm.push(res)?;
            }
            0x04 => {
                // DIV
                gas = 5;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let a = evm.pop()?;
                let b = evm.pop()?;
                if b.is_zero() || a.is_zero() {
                    evm.push(Word::zero())?;
                } else {
                    evm.push(a / b)?;
                }
            }
            0x05 => {
                // SDIV
                gas = 5;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
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
            }
            0x06 => {
                // MOD
                gas = 5;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let a = evm.pop()?;
                let b = evm.pop()?;
                if b.is_zero() {
                    evm.push(Word::zero())?;
                } else {
                    evm.push(a % b)?;
                }
            }
            0x07 => {
                // SMOD
                gas = 5;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
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
            }
            0x08 => {
                // ADDMOD
                gas = 8;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let a = evm.pop()?;
                let b = evm.pop()?;
                let m = evm.pop()?;
                let r = if m.is_zero() {
                    Word::zero()
                } else {
                    a.add_modulo(&b, &m)
                };
                self.debug["ADDMOD"] = json!({
                    "a": a,
                    "b": b,
                    "m": m,
                    "r": r,
                });
                evm.push(r)?;
            }
            0x09 => {
                // MULMOD
                gas = 8;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let a = evm.pop()?;
                let b = evm.pop()?;
                let m = evm.pop()?;
                let res = a.mul_modulo(&b, &m);
                evm.push(res)?;
            }
            0x0a => {
                // EXP
                let base = evm.pop()?;
                let exponent = evm.pop()?;
                let exp_bytes = exponent
                    .into_bytes()
                    .into_iter()
                    .skip_while(|byte| byte == &0)
                    .count() as i64;
                gas = 10 + exp_bytes * 50;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                evm.push(base.pow(exponent))?;
            }
            0x0b => {
                // SIGNEXTEND
                gas = 5;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let x = evm.pop()?.as_usize();
                let b = evm.pop()?;

                let bit = ((x + 1) << 3) - 1;
                let neg = b.bit(bit);

                let mask = Word::max() << (bit + 1);
                let y = if neg { b | mask } else { b & !mask };
                evm.push(y)?;
            }

            // 0x10s: Comparison & Bitwise Logic
            0x10 => {
                // LT
                gas = 3;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let a = evm.pop()?;
                let b = evm.pop()?;
                evm.push(if a < b { Word::one() } else { Word::zero() })?;
            }
            0x11 => {
                // GT
                gas = 3;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let a = evm.pop()?;
                let b = evm.pop()?;
                evm.push(if a > b { Word::one() } else { Word::zero() })?;
            }
            0x12 => {
                // SLT
                gas = 3;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let a = evm.pop()?;
                let b = evm.pop()?;
                let a_signed = I256::from_be_bytes(a.into_bytes());
                let b_signed = I256::from_be_bytes(b.into_bytes());
                evm.push(if a_signed < b_signed {
                    Word::one()
                } else {
                    Word::zero()
                })?;
            }
            0x13 => {
                // SGT
                gas = 3;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let a = evm.pop()?;
                let b = evm.pop()?;
                let a_signed = I256::from_be_bytes(a.into_bytes());
                let b_signed = I256::from_be_bytes(b.into_bytes());
                evm.push(if a_signed > b_signed {
                    Word::one()
                } else {
                    Word::zero()
                })?;
            }
            0x14 => {
                // EQ
                gas = 3;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let a = evm.pop()?;
                let b = evm.pop()?;
                evm.push(if a == b { Word::one() } else { Word::zero() })?;
            }
            0x15 => {
                // ISZERO
                gas = 3;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let a = evm.pop()?;
                evm.push(if a.is_zero() {
                    Word::one()
                } else {
                    Word::zero()
                })?;
            }
            0x16 => {
                // AND
                gas = 3;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let a = evm.pop()?;
                let b = evm.pop()?;
                evm.push(a & b)?;
            }
            0x17 => {
                // OR
                gas = 3;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let a = evm.pop()?;
                let b = evm.pop()?;
                evm.push(a | b)?;
            }
            0x18 => {
                // XOR
                gas = 3;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let a = evm.pop()?;
                let b = evm.pop()?;
                evm.push(a ^ b)?;
            }
            0x19 => {
                // NOT
                gas = 3;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let a = evm.pop()?;
                evm.push(!a)?;
            }
            0x1a => {
                // BYTE
                gas = 3;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let index = evm.pop()?.as_usize();
                let value: Word = evm.pop()?;
                if index < 32 {
                    evm.push(Word::from(value.into_bytes()[index]))?;
                } else {
                    evm.push(Word::zero())?;
                }
            }
            0x1b => {
                // SHL
                gas = 3;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let shift = evm.pop()?.as_usize();
                let value = evm.pop()?;
                let ret = value << shift;
                evm.push(ret)?;
            }
            0x1c => {
                // SHR
                gas = 3;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let shift = evm.pop()?.as_usize();
                let value = evm.pop()?;
                let ret = value >> shift;
                evm.push(ret)?;
            }
            0x1d => {
                // SAR
                gas = 3;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let shift = evm.pop()?.as_usize();
                let value = evm.pop()?;
                let value = I256::from_be_bytes(value.into_bytes());
                let ret = value >> shift;
                let ret = Word::from_bytes(&ret.to_be_bytes());
                evm.push(ret)?;
            }

            0x20 => {
                // SHA3 (KECCAK256)
                let offset = evm.pop()?.as_usize();
                let size = evm.pop()?.as_usize();
                if size > ALLOCATION_SANITY_LIMIT {
                    return Err(ExecutorError::InvalidAllocation(offset + size).into());
                }
                let mut data = vec![0u8; size];
                if offset < evm.memory.len() {
                    let len = size.min(evm.memory.len() - offset);
                    data[0..len].copy_from_slice(&evm.memory[offset..offset + len]);
                };
                let sha3 = keccak256(&data);
                let hash = Word::from_bytes(&sha3);
                self.tracer.push(Event {
                    data: EventData::Hash {
                        data: data.into(),
                        hash: sha3.into(),
                        alg: HashAlg::Keccak256,
                    },
                    depth: ctx.depth,
                    reverted: false,
                });
                gas = 30 + 6 * size.div_ceil(32) as i64;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                evm.push(hash)?;
            }

            // 30-3f
            0x30 => {
                // ADDRESS
                gas = 2;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                evm.push((&this).into())?;
            }
            0x31 => {
                // BALANCE
                let addr = (&evm.pop()?).into();
                // EIP-2929: Use proper address access tracking
                gas = evm.address_access_cost(&addr, ext);
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let value = ext.balance(&addr).await?;
                self.debug["BALANCE"] = json!({
                    "address": addr,
                    "balance": value,
                    "is_coinbase": addr == self.header.miner,
                });
                evm.push(value)?;
            }
            0x32 => {
                // ORIGIN
                gas = 2;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                evm.push((&ctx.origin).into())?;
            }
            0x33 => {
                // CALLER
                gas = 2;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                evm.push((&call.from).into())?;
            }
            0x34 => {
                // CALLVALUE
                gas = 2;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let value = if matches!(ctx.call_type, CallType::Static) {
                    Word::zero()
                } else {
                    call.value
                };
                evm.push(value)?;
            }
            0x35 => {
                // CALLDATALOAD
                gas = 3;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let offset = evm.pop()?.as_usize();
                if offset > call.data.len() {
                    evm.push(Word::zero())?;
                } else {
                    let mut data = [0u8; 32];
                    let copy = call.data.len().min(offset + 32) - offset;
                    data[0..copy].copy_from_slice(&call.data[offset..offset + copy]);
                    evm.push(Word::from_bytes(&data))?;
                }
            }
            0x36 => {
                // CALLDATASIZE
                gas = 2;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                evm.push(Word::from(call.data.len()))?;
            }
            0x37 => {
                // CALLDATACOPY
                let dest_offset = evm.pop()?.as_usize();
                let offset = evm.pop()?.as_usize();
                let size = evm.pop()?.min((usize::MAX >> 1).into()).as_usize();
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
                gas = 3 + 3 * size.div_ceil(32) as i64;
                gas += evm.memory_expansion_cost();
            }
            0x38 => {
                // CODESIZE
                gas = 2;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let len = code.bytecode.len();
                evm.push(len.into())?;
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
                    if offset + size > ALLOCATION_SANITY_LIMIT {
                        return Err(ExecutorError::InvalidAllocation(offset + size).into());
                    }
                    code.resize(offset + size, 0);
                }
                evm.memory[dest_offset..dest_offset + size]
                    .copy_from_slice(&code[offset..offset + size]);
                gas = 3 + 3 * size.div_ceil(32) as i64;
                gas += evm.memory_expansion_cost();
            }
            0x3a => {
                // GASPRICE
                gas = 2;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                evm.push(ext.tx_ctx.gas_price)?;
            }
            0x3b => {
                // EXTCODESIZE
                let address: Address = (&evm.pop()?).into();
                gas = evm.address_access_cost(&address, ext);
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let code_size = ext.code(&address).await?.0.len();
                evm.push(Word::from(code_size))?;
            }
            0x3c => {
                // EXTCODECOPY
                let address: Address = (&evm.pop()?).into();
                let dest_offset = evm.pop()?.as_usize();
                let offset = evm.pop()?.as_usize();
                let size = evm.pop()?.as_usize();

                let (mut code, _) = ext.code(&address).await?;
                if evm.memory.len() < dest_offset + size {
                    if dest_offset + size > ALLOCATION_SANITY_LIMIT {
                        return Err(ExecutorError::InvalidAllocation(dest_offset + size).into());
                    }
                    let padding = 32 - (dest_offset + size) % 32;
                    evm.memory.resize(dest_offset + size + padding % 32, 0);
                }
                if code.len() < offset + size {
                    if offset + size > ALLOCATION_SANITY_LIMIT {
                        return Err(ExecutorError::InvalidAllocation(offset + size).into());
                    }
                    code.resize(offset + size, 0);
                }
                evm.memory[dest_offset..dest_offset + size]
                    .copy_from_slice(&code[offset..offset + size]);
                gas = 3 * size.div_ceil(32) as i64;
                gas += evm.memory_expansion_cost();
                gas += evm.address_access_cost(&address, ext);
            }
            0x3d => {
                // RETURNDATASIZE
                gas = 2;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
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
                gas = 3 + 3 * size.div_ceil(32) as i64;
                gas += evm.memory_expansion_cost();
            }
            0x3f => {
                // EXTCODEHASH
                let address: Address = (&evm.pop()?).into();
                gas = evm.address_access_cost(&address, ext);
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let is_empty = ext.is_empty(&address).await?;
                if is_empty {
                    evm.push(Word::zero())?;
                } else {
                    let (_, hash) = ext.code(&address).await?;
                    evm.push(hash)?;
                }
            }

            // 40-4a
            0x40 => {
                // BLOCKHASH
                gas = 20;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let block_number = evm.pop()?;
                let block_hash = ext.get_block_hash(block_number).await?;
                evm.push(block_hash)?;
            }
            0x41 => {
                // COINBASE
                gas = 2;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                evm.push((&self.header.miner).into())?;
            }
            0x42 => {
                // TIMESTAMP
                gas = 2;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                evm.push(self.header.timestamp)?;
            }
            0x43 => {
                // NUMBER
                gas = 2;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                evm.push(self.header.number)?;
            }
            0x44 => {
                // PREVRANDAO
                gas = 2;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                evm.push(Word::zero())?;
            }
            0x45 => {
                // GASLIMIT
                gas = 2;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                evm.push(self.header.gas_limit)?;
            }
            0x46 => {
                // CHAINID
                gas = 2;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                evm.push(Word::one())?; // TODO: From TX
            }
            0x47 => {
                // SELFBALANCE
                gas = 5;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let balance = ext.balance(&this).await?;

                self.debug["SELFBALANCE"] = json!({
                    "address": this,
                    "balance": balance,
                });

                evm.push(balance)?;
            }
            0x48 => {
                // BASEFEE
                gas = 2;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                evm.push(self.header.base_fee)?;
            }
            0x49 => {
                // BLOBHASH
                gas = 3;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let _index = evm.pop()?;
                // evm.push(self.header.extra_data)?;
                evm.push(Word::zero())?;
                // TODO: make it work properly?
                // > tx.blob_versioned_hashes[index] if index < len(tx.blob_versioned_hashes),
                // > and otherwise with a zeroed bytes32 value."
                // (See: https://www.evm.codes/?fork=prague#49)
            }
            0x4a => {
                // BLOBBASEFEE
                gas = 2;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                // https://eips.ethereum.org/EIPS/eip-4844#gas-accounting
                let word = self.header.blob_gas_price();
                evm.push(word)?;
            }

            // 0x50s: Stack, Memory, Storage and Flow Operations
            0x50 => {
                // POP
                gas = 2;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                evm.pop()?;
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
                gas = 3;
                gas += evm.memory_expansion_cost();
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let value = Word::from_bytes(&evm.memory[offset..end]);
                evm.push(value)?;
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
                gas = 3;
                gas += evm.memory_expansion_cost();
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let bytes = &value.into_bytes();
                evm.memory[offset..end].copy_from_slice(bytes);
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
                gas = 3;
                gas += evm.memory_expansion_cost();
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                evm.memory[offset] = value
                    .into_bytes()
                    .iter()
                    .rev()
                    .nth(0)
                    .copied()
                    .unwrap_or_default();
            }
            0x54 => {
                // SLOAD
                let [key] = evm.peek()?;
                let is_warm = ext.is_storage_warm(&this, &key);
                if !is_warm {
                    ext.warm_storage(&this, &key);
                }
                gas = if is_warm {
                    100.into() // warm
                } else {
                    2100.into() // cold
                };
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }

                evm.pop()?;
                let val = evm.get(ext, &this, &key).await?;
                evm.push(val)?;
                evm.touches
                    .push(AccountTouch::GetState(this, key, val, is_warm));
                self.tracer.push(Event {
                    data: EventData::State(StateEvent::Get {
                        address: this,
                        key,
                        val,
                    }),
                    depth: ctx.depth,
                    reverted: false,
                });

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

                let is_warm = ext.is_storage_warm(&this, &key);
                if !is_warm {
                    ext.warm_storage(&this, &key);
                }

                let val = evm.get(ext, &this, &key).await?;
                let original = ext.original.get(&(this, key)).cloned().unwrap_or_default();

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

                self.debug["SSTORE"] = json!({
                    "is_warm": is_warm,
                    "original": original,
                    "address": this,
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

                if evm.gas.remaining() + gas_refund < 0 {
                    let actual_gas_cost = evm.gas.remaining();

                    self.debug["SSTORE"]["is_oog"] = json!(true);
                    self.debug["SSTORE"]["note"] = json!("edge case with negative gas refund");
                    self.tracer.push(Event {
                        depth: ctx.depth,
                        reverted: true,
                        data: EventData::OpCode {
                            pc: instruction.offset,
                            op: instruction.opcode.code,
                            name: instruction.opcode.name(),
                            data: instruction.argument.clone().map(Into::into),
                            gas_cost: 0,
                            gas_used: evm.gas.used,
                            gas_back: 0,
                            gas_left: actual_gas_cost,
                            stack: evm.stack.clone(),
                            memory: evm.memory.chunks(32).map(Word::from_bytes).collect(),
                            debug: self.debug.take(),
                        },
                    });
                    return Ok(StepResult::Halt(0));
                }

                if evm.gas.remaining() < gas_cost {
                    let actual_gas_cost = evm.gas.remaining();

                    self.debug["SSTORE"]["is_oog"] = json!(true);
                    self.tracer.push(Event {
                        depth: ctx.depth,
                        reverted: true,
                        data: EventData::OpCode {
                            pc: instruction.offset,
                            op: instruction.opcode.code,
                            name: instruction.opcode.name(),
                            data: instruction.argument.clone().map(Into::into),
                            gas_cost: actual_gas_cost,
                            gas_used: evm.gas.used + actual_gas_cost,
                            gas_back: 0,
                            gas_left: 0,
                            stack: evm.stack.clone(),
                            memory: evm.memory.chunks(32).map(Word::from_bytes).collect(),
                            debug: self.debug.take(),
                        },
                    });
                    return Ok(StepResult::Halt(0));
                }

                evm.put(ext, &this, key, new).await?;

                evm.gas.refund(gas_refund);
                gas = gas_cost;
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
                evm.touches
                    .push(AccountTouch::SetState(this, key, val, new, is_warm));
            }
            0x56 => {
                // JUMP
                gas = 8;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let dest = evm.pop()?.as_usize();
                let Some(dest) = code.resolve_jump(dest) else {
                    return Ok(StepResult::Halt(gas));
                };
                if dest >= code.instructions.len() {
                    return Ok(StepResult::Halt(gas));
                }
                if code.instructions[dest].opcode.code != 0x5b {
                    return Ok(StepResult::Halt(gas));
                }
                evm.pc = dest;
                pc_increment = false;
            }
            0x57 => {
                // JUMPI
                gas = 10;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let dest = evm.pop()?.as_usize();
                let cond = evm.pop()?;
                if !cond.is_zero() {
                    let Some(dest) = code.resolve_jump(dest) else {
                        return Ok(StepResult::Halt(gas));
                    };
                    if dest >= code.instructions.len() {
                        return Ok(StepResult::Halt(gas));
                    }
                    if code.instructions[dest].opcode.code != 0x5b && dest != 0 {
                        return Ok(StepResult::Halt(gas));
                    }
                    evm.pc = dest;
                    pc_increment = false;
                }
            }
            0x58 => {
                // PC
                gas = 2;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                evm.push(Word::from(instruction.offset))?;
            }
            0x59 => {
                // MSIZE
                gas = 2;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                evm.push(Word::from(evm.memory.len()))?;
            }
            0x5a => {
                // GAS
                gas = 2;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let val = (evm.gas.remaining() - 2) as u64;
                evm.push(val.into())?;
            }
            0x5b => {
                // JUMPDEST: noop, a valid destination for JUMP/JUMPI
                gas = 1;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
            }
            0x5c => {
                // TLOAD
                gas = 100;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let key = evm.pop()?;
                let val = ext.transient.get(&(this, key)).copied().unwrap_or_default();
                evm.push(val)?;
                self.debug["TLOAD"] = json!({
                    "address": this,
                    "key": key,
                    "val": val,
                });
            }
            0x5d => {
                // TSTORE
                gas = 100;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let key = evm.pop()?;
                let val = evm.pop()?;
                let old = ext.transient.insert((this, key), val).unwrap_or_default();
                evm.touches
                    .push(AccountTouch::SetTransientState(this, key, old, val));
                self.debug["TSTORE"] = json!({
                    "address": this,
                    "key": key,
                    "val": val,
                    "old": old,
                });
            }
            0x5e => {
                // MCOPY
                let dest_offset = evm.pop()?.as_usize();
                let offset = evm.pop()?.as_usize();
                let size = evm.pop()?.as_usize();

                let words = size.div_ceil(32);
                gas = 3 + 3 * words as i64;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }

                if size > 0 {
                    let len = (dest_offset + size).max(offset + size);
                    if len > evm.memory.len() {
                        if dest_offset + size > ALLOCATION_SANITY_LIMIT {
                            return Err(ExecutorError::InvalidAllocation(dest_offset + size).into());
                        }
                        let padding = 32 - len % 32;
                        evm.memory.resize(len + padding % 32, 0);
                    }
                    let mut buffer = vec![0u8; size];
                    if offset + size <= evm.memory.len() {
                        buffer.copy_from_slice(&evm.memory[offset..offset + size]);
                    } else {
                        let copy = evm.memory.len().min(offset + size) - offset;
                        buffer[..copy].copy_from_slice(&evm.memory[offset..offset + copy]);
                        buffer[copy..].copy_from_slice(&vec![0u8; size - copy]);
                    }

                    gas += evm.memory_expansion_cost();
                    if evm.gas.remaining() < gas {
                        return Ok(StepResult::Halt(gas));
                    }
                    evm.memory[dest_offset..dest_offset + size].copy_from_slice(&buffer);
                }

                self.debug["MCOPY"] = json!({
                    "dest_offset": dest_offset,
                    "offset": offset,
                    "size": size,
                    "mem.len": evm.memory.len(),
                });
            }
            0x5f => {
                // PUSH0
                gas = 2;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                evm.push(Word::zero())?;
            }

            0x60..=0x7f => {
                // PUSH1..PUSH32
                gas = 3;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let arg = instruction
                    .argument
                    .as_ref()
                    .ok_or(ExecutorError::MissingData)?;
                evm.push(Word::from_bytes(arg))?;
            }

            0x80..=0x8f => {
                // DUP1..DUP16
                gas = 3;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let n = instruction.opcode.n as usize;
                if evm.stack.len() < n {
                    evm.error(ExecutorError::StackUnderflow.into())?;
                }
                let val = evm.stack[evm.stack.len() - n];
                evm.push(val)?;
            }

            0x90..=0x9f => {
                // SWAP1..SWAP16
                gas = 3;
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }
                let n = instruction.opcode.n as usize;
                if evm.stack.len() <= n {
                    evm.error(ExecutorError::StackUnderflow.into())?;
                }
                let stack_len = evm.stack.len();
                evm.stack.swap(stack_len - 1, stack_len - 1 - n);
            }

            0xa0..=0xa4 => {
                // LOG0..LOG4
                if matches!(ctx.call_type, CallType::Static) {
                    return Err(ExecutorError::StaticCallViolation(opcode).into());
                }
                let n = instruction.opcode.n as usize;
                let offset = evm.pop()?.as_usize();
                let size = evm.pop()?.as_usize();

                gas = 375;
                gas += 375 * n as i64 + 8 * size as i64;
                gas += evm.memory_expansion_cost();
                if evm.gas.remaining() < gas {
                    return Ok(StepResult::Halt(gas));
                }

                let mut topics = Vec::with_capacity(n);
                for _ in 0..n {
                    topics.push(evm.pop()?);
                }
                topics.reverse();

                let data = if offset + size > evm.memory.len() {
                    if offset + size > ALLOCATION_SANITY_LIMIT {
                        return Err(ExecutorError::InvalidAllocation(offset + size).into());
                    }
                    let mut data = evm.memory.clone();
                    data.resize(offset + size, 0);
                    data
                } else {
                    evm.memory[offset..offset + size].to_vec()
                };
                evm.logs.push(Log(this, topics, data));
            }

            0xf0 => {
                // CREATE
                if matches!(ctx.call_type, CallType::Static) {
                    return Err(ExecutorError::StaticCallViolation(opcode).into());
                }
                self.create(instruction, this, &mut gas, evm, ext, ctx)
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
                match self
                    .call(instruction, this, call, &mut gas, evm, ext, ctx)
                    .await
                    .with_context(|| "opcode: CALL")
                {
                    Ok(()) => {} // ignore
                    Err(_) => {
                        return Ok(StepResult::Halt(evm.gas.remaining()));
                    }
                }
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
                match self
                    .call(instruction, this, call, &mut gas, evm, ext, ctx)
                    .await
                {
                    Ok(()) => {} // ignore
                    Err(_) => {
                        return Ok(StepResult::Halt(evm.gas.remaining()));
                    }
                }
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
                match self
                    .call(instruction, this, call, &mut gas, evm, ext, ctx)
                    .await
                {
                    Ok(()) => {} // ignore
                    Err(_) => {
                        return Ok(StepResult::Halt(evm.gas.remaining()));
                    }
                }
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
                self.create(instruction, this, &mut gas, evm, ext, ctx)
                    .await?;
            }
            0xfa => {
                // STATICCALL
                let ctx = Context {
                    call_type: CallType::Static,
                    ..ctx
                };
                match self
                    .call(instruction, this, call, &mut gas, evm, ext, ctx)
                    .await
                {
                    Ok(()) => {} // ignore
                    Err(_) => {
                        return Ok(StepResult::Halt(evm.gas.remaining()));
                    }
                }
            }
            0xfe => {
                // INVALID: handled outside of `execute_instruction`
            }
            0xff => {
                // SELFDESTRUCT
                if matches!(ctx.call_type, CallType::Static) {
                    return Err(ExecutorError::StaticCallViolation(opcode).into());
                }

                let address: Address = (&evm.pop()?).into();

                let opcode_cost = 5000;
                let access_cost = if ext.is_address_warm(&address) {
                    0
                } else {
                    2600
                };
                let create_cost = if ext.is_empty(&address).await? {
                    25000 // account creation cost
                } else {
                    0
                };

                let total_gas_cost = opcode_cost + access_cost + create_cost;
                gas = total_gas_cost.into();

                let balance = ext.balance(&this).await?;
                ext.account_mut(&this).value = Word::zero();
                ext.account_mut(&address).value += balance;
                ext.destroyed_accounts.push(this);

                // TODO: add traces and account/state events
            }
            _ => {
                return Err(ExecutorError::UnknownOpcode(opcode).into());
            }
        }

        if pc_increment {
            evm.pc += 1;
        }

        Ok(StepResult::Ok(gas))
    }

    #[allow(clippy::too_many_arguments)]
    async fn call(
        &mut self,
        instruction: &Instruction,
        this: Address,
        call: &Call,
        gas: &mut i64,
        evm: &mut Evm,
        ext: &mut Ext,
        ctx: Context,
    ) -> eyre::Result<()> {
        let call_gas = evm.pop()?.min(i64::MAX.into()); // avoid possible i64 overflow
        let address: Address = (&evm.pop()?).into();
        let value = if !matches!(ctx.call_type, CallType::Static | CallType::Delegate) {
            evm.pop()?
        } else if matches!(ctx.call_type, CallType::Static) {
            Word::zero() // STATICCALL always has zero value
        } else {
            call.value // DELEGATECALL inherits value from parent
        };
        let args_offset = evm.pop()?.as_usize();
        let args_size = evm.pop()?.as_usize();
        let ret_offset = evm.pop()?.as_usize();
        let ret_size = evm.pop()?.as_usize();

        // Handle memory expansion for arguments and return data
        let args_max = if args_size > 0 {
            args_offset + args_size
        } else {
            0
        };
        let ret_max = if ret_size > 0 {
            ret_offset + ret_size
        } else {
            0
        };
        let size = args_max.max(ret_max);
        if size > evm.memory.len() {
            if size > ALLOCATION_SANITY_LIMIT {
                return Err(ExecutorError::InvalidAllocation(size).into());
            }
            let size = size.div_ceil(32) * 32;
            evm.memory.resize(size, 0);
        }
        let memory_expansion_cost = evm.memory_expansion_cost();

        let mut create_cost = 0;
        let is_empty = !precompiles::is_precompile(&address) && ext.is_empty(&address).await?;
        if !value.is_zero() && is_empty {
            create_cost = 25000; // account creation cost
        }

        // Calculate address access cost (EIP-2929)
        let (code, codehash) = ext.code(&address).await?;
        let mut access_cost = evm.address_access_cost(&address, ext);

        // Check and resolve delegation: CODE = <0xef0100> + <20 bytes address>
        let is_delegated = code.len() == 23 && code.starts_with(&[0xef, 0x01, 0x00]);
        let code = if is_delegated {
            access_cost += 100;
            let target = Address::try_from(&code[3..]).expect("must succeed");
            let (code, _) = ext.code(&target).await?;
            let target_cost = evm.address_access_cost(&target, ext);
            access_cost += target_cost - 100;
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

        // Check if there is enough gas for the base cost (out-of-gas condition)
        if base_gas_cost > evm.gas.remaining() {
            let gas_cost = evm.gas.remaining();
            *gas = gas_cost;

            self.tracer.push(Event {
                depth: ctx.depth,
                reverted: false,
                data: EventData::OpCode {
                    pc: instruction.offset,
                    op: instruction.opcode.code,
                    name: instruction.opcode.name(),
                    data: instruction.argument.clone().map(Into::into),
                    gas_cost,
                    gas_used: evm.gas.used + gas_cost,
                    gas_left: 0,
                    stack: evm.stack.clone(),
                    memory: evm.memory.chunks(32).map(Word::from_bytes).collect(),
                    gas_back: 0,
                    debug: json!({
                        "is_call": true,
                        "evm.gas.used": evm.gas.used,
                        "evm.gas.refund": evm.gas.refund,
                        "call.address": address,
                        "call.input": hex::encode(&evm.memory[args_offset..args_offset + args_size]),
                        "call.result": "OOG",
                        "call.gas": call_gas.as_u64(),
                        "access_cost": access_cost,
                        "memory_expansion_cost": memory_expansion_cost,
                    }),
                },
            });

            // Don't add refunds from reverted calls
            evm.refund = evm.gas.refund;
            evm.push(Word::zero())?;
            return Err(ExecutorError::OutOfGas().into());
        }

        // Calculate available gas for forwarding using "all but one 64th" rule
        let remaining_gas = evm.gas.remaining() - base_gas_cost;
        let all_but_one_64th = remaining_gas - remaining_gas / 64;
        let gas_to_forward = call_gas.as_i64().min(all_but_one_64th) + gas_stipend_adjustment;

        // For EVM accounting: only charge the outer EVM for base cost
        // (forwarded gas was already "spent" by allocating it to inner call)
        *gas = base_gas_cost;

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
            // TODO: check if there is enough gas
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
        let sender_balance = ext.balance(&this).await?;
        let receiver_balance = ext.balance(&address).await?;
        if !value.is_zero() && !matches!(ctx.call_type, CallType::Static | CallType::Delegate) {
            if sender_balance >= value {
                // For self-calls (where sender == receiver), no net balance change
                if this != address {
                    let new_sender_balance = sender_balance - value;
                    ext.account_mut(&this).value = new_sender_balance;
                    evm.touches.push(AccountTouch::SetValue(
                        this,
                        sender_balance,
                        new_sender_balance,
                    ));
                    self.tracer.push(Event {
                        data: EventData::Account(AccountEvent::SetValue {
                            address: this,
                            val: sender_balance,
                            new: new_sender_balance,
                        }),
                        depth: ctx.depth,
                        reverted: false,
                    });
                    let new_receiver_balance = receiver_balance + value;
                    ext.account_mut(&address).value = new_receiver_balance;
                    evm.touches.push(AccountTouch::SetValue(
                        address,
                        receiver_balance,
                        new_receiver_balance,
                    ));
                    self.tracer.push(Event {
                        data: EventData::Account(AccountEvent::SetValue {
                            address,
                            val: receiver_balance,
                            new: new_receiver_balance,
                        }),
                        depth: ctx.depth,
                        reverted: false,
                    });
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
                    "gas_left": evm.gas.remaining() - total_gas_cost_for_tracing,
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
                    "call.is_delegated": is_delegated,
                    "access_cost": access_cost,
                    "inner_evm.reverted": inner_evm.reverted,
                    "is_empty": is_empty,
                    "code.len": code.bytecode.len(),
                    "ret": hex::encode(&self.ret),
                }),
            },
        });
        self.tracer.join(tracer, inner_evm.reverted);

        let copy_len = ret.len().min(ret_size);
        if copy_len > 0 {
            evm.memory[ret_offset..ret_offset + copy_len].copy_from_slice(&ret[..copy_len]);
        }

        // TODO: check if there is enough gas

        evm.gas.used += inner_evm.gas.used;
        evm.gas.used -= gas_stipend_adjustment;

        if inner_evm.reverted {
            // Don't add refunds from reverted calls
            evm.refund = evm.gas.refund;
            self.ret = ret;
            evm.push(Word::zero())?;
            inner_evm.revert(ext).await?;
            return Ok(());
        }

        // Only add refunds if call succeeded
        evm.gas.refund += inner_evm.gas.refund;
        evm.refund = evm.gas.refund;

        evm.touches.extend(inner_evm.touches.into_iter());

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
        gas: &mut i64,
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

        if size > 0 && offset + size > evm.memory.len() {
            if offset + size > ALLOCATION_SANITY_LIMIT {
                return Err(ExecutorError::InvalidAllocation(offset + size).into());
            }
            let padding = 32 - (offset + size) % 32;
            evm.memory.resize(offset + size + padding % 32, 0);
        }

        let memory_expansion_cost = evm.memory_expansion_cost();

        let bytecode = evm.memory[offset..offset + size].to_vec();
        let word_size = bytecode.len().div_ceil(32) as i64;
        let init_code_cost = 2 * word_size
            + if matches!(ctx.call_type, CallType::Create2) {
                6 * word_size
            } else {
                0
            };
        let code = Decoder::decode(bytecode);

        let nonce = ext.nonce(&this).await?;
        let created = if matches!(ctx.call_type, CallType::Create2) {
            this.create2(&salt, &code.bytecode)
        } else {
            this.create(nonce)
        };

        ext.created_accounts.push(created);
        ext.warm_address(&created);
        evm.touches.push(AccountTouch::WarmUp(created));

        let create_cost = 32000;
        let base_gas_cost = memory_expansion_cost + create_cost + init_code_cost;
        let remaining_gas = evm.gas.remaining() - base_gas_cost;

        if remaining_gas <= 0 {
            // TODO: handle this case properly
            panic!("remaining gas <= 0");
        }

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
        let base_cost_without_deploy = memory_expansion_cost + create_cost + init_code_cost;

        // HERE: TODO: remove this label
        let total_gas_cost_for_tracing =
            memory_expansion_cost + create_cost + init_code_cost + gas_to_forward;
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
                    "created": {
                        "opcode": ctx.call_type,
                        "address": created,
                        "creator": this,
                        "nonce": nonce,
                    },
                    "inner_evm.reverted": inner_evm.reverted,
                    "inner_call": inner_call,
                }),
            },
        });

        self.tracer.join(tracer, inner_evm.reverted);

        // Check if there's enough gas left to pay for deployed code
        // gas_to_forward is what was given to inner call
        // After inner execution, we need: inner_evm.gas.used + deployed_code_cost <= gas_to_forward
        if gas_to_forward < inner_evm.gas.used + deployed_code_cost {
            // Not enough gas to deploy the code - creation fails
            evm.gas.used += base_cost_without_deploy + gas_to_forward;
            evm.push(Word::zero())?;
            inner_evm.revert(ext).await?;
            return Ok(());
        }

        evm.gas.used += base_cost_without_deploy + deployed_code_cost + inner_evm.gas.used;
        *gas = 0;

        if inner_evm.reverted {
            evm.push(Word::zero())?;
            inner_evm.revert(ext).await?;
            return Ok(());
        }

        let hash = keccak256(&code);
        let _empty = ext.code(&created).await?;
        *ext.code_mut(&created) = (code.clone(), Word::from_bytes(&hash));

        let sender_balance = ext.balance(&this).await?;
        let receiver_balance = ext.balance(&created).await?;
        if !value.is_zero() && !matches!(ctx.call_type, CallType::Static | CallType::Delegate) {
            if sender_balance >= value {
                let new_sender_balance = sender_balance - value;
                ext.account_mut(&this).value = new_sender_balance;
                evm.touches.push(AccountTouch::SetValue(
                    this,
                    sender_balance,
                    new_sender_balance,
                ));
                self.tracer.push(Event {
                    data: EventData::Account(AccountEvent::SetValue {
                        address: this,
                        val: sender_balance,
                        new: new_sender_balance,
                    }),
                    depth: ctx.depth,
                    reverted: false,
                });
                let new_receiver_balance = receiver_balance + value;
                ext.account_mut(&created).value = new_receiver_balance;
                evm.touches.push(AccountTouch::SetValue(
                    created,
                    receiver_balance,
                    new_receiver_balance,
                ));
                self.tracer.push(Event {
                    data: EventData::Account(AccountEvent::SetValue {
                        address: created,
                        val: receiver_balance,
                        new: new_receiver_balance,
                    }),
                    depth: ctx.depth,
                    reverted: false,
                });
            } else {
                // TODO: insufficient funds to transfer
            }
        }

        let nonce = ext.account_mut(&this).nonce;
        ext.account_mut(&this).nonce += Word::one();
        evm.touches.push(AccountTouch::SetNonce(
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

        evm.touches.push(AccountTouch::Create(
            created,
            value,
            Word::one(),
            code.clone(),
            Word::from_bytes(&hash),
        ));
        self.tracer.push(Event {
            data: EventData::Account(AccountEvent::Create {
                address: created,
                creator: this,
                nonce: Word::one(),
                value,
                codehash: Word::from_bytes(&hash),
                bytecode: code.into(),
            }),
            depth: ctx.depth,
            reverted: false,
        });

        // Accumulate gas refunds from inner execution
        evm.gas.refund += inner_evm.gas.refund;
        evm.refund = evm.gas.refund;

        evm.touches.extend(inner_evm.touches.into_iter());
        evm.push((&created).into())?;
        Ok(())
    }
}
