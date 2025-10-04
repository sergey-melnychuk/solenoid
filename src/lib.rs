pub mod common;
pub mod decoder;
pub mod eth;
pub mod executor;
pub mod ext;
pub mod opcodes;
pub mod precompiles;
pub mod solenoid;
pub mod tracer;

use std::{pin::Pin, sync::Arc};

use evm_tracer::{OpcodeTrace, run::TxResult};
use tokio::sync::Mutex;

use crate::{
    common::block::{Header, Tx},
    ext::Ext,
    solenoid::{Builder as _, CallResult, Solenoid},
    tracer::{EventTracer as _, LoggingTracer},
};

impl From<CallResult<LoggingTracer>> for TxResult {
    fn from(value: CallResult<LoggingTracer>) -> Self {
        Self {
            gas: value.evm.gas.used as u64, // TODO: use finalized gas
            ret: value.ret,
            rev: value.evm.reverted,
        }
    }
}

pub fn runner(
    header: Header,
    ext: Ext,
) -> impl FnMut(Tx) -> Pin<Box<dyn Future<Output = eyre::Result<(TxResult, Vec<OpcodeTrace>)>>>> {
    let ext = Arc::new(Mutex::new(ext));
    move |tx| {
        let header = header.clone();
        let ext = ext.clone();
        Box::pin(async move {
            let mut result = tokio::spawn(async move {
                let mut guard = ext.lock().await;
                let result = Solenoid::new()
                    .execute(tx.to.unwrap_or_default(), "", tx.input.as_ref())
                    .with_header(header.clone())
                    .with_sender(tx.from)
                    .with_gas(tx.gas)
                    .with_value(tx.value)
                    .ready()
                    .apply(&mut *guard)
                    .await?;
                Ok::<_, eyre::Report>(result)
            }).await??;

            let traces = result
                .tracer
                .take()
                .into_iter()
                .filter_map(|event| evm_tracer::OpcodeTrace::try_from(event).ok())
                .collect::<Vec<_>>();

            Ok((result.into(), traces))
        })
    }
}
