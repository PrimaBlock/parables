use ethcore::trace;
use ethcore::trace::trace::{Call, Create};
use ethereum_types::{H160, U256};
use linker;
use parity_bytes::Bytes;
use parity_evm;
use parity_vm;
use source_map;
use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use utils;

#[derive(Debug, PartialEq, Eq)]
pub enum TxEvent {
    /// Generic trace error.
    TraceError(String),
}

pub struct TxTracer<'a> {
    linker: Option<Arc<linker::Linker>>,
    // current source.
    source: &'a Mutex<Option<Arc<(source_map::SourceMap, HashMap<usize, usize>)>>>,
    traces: Vec<TxEvent>,
}

impl<'a> TxTracer<'a> {
    pub fn new(
        linker: Option<Arc<linker::Linker>>,
        source: &'a Mutex<Option<Arc<(source_map::SourceMap, HashMap<usize, usize>)>>>,
    ) -> Self {
        Self {
            linker,
            source,
            traces: Vec::new(),
        }
    }
}

impl<'a> trace::Tracer for TxTracer<'a> {
    type Output = TxEvent;

    fn prepare_trace_call(&self, params: &parity_vm::ActionParams) -> Option<Call> {
        if let Some(runtime_map) = self.linker
            .as_ref()
            .and_then(|linker| linker.find_runtime_source(params.address))
        {
            let mut source = self.source.lock().expect("not poisoned lock");
            *source = Some(runtime_map);
        }

        None
    }

    fn prepare_trace_create(&self, _params: &parity_vm::ActionParams) -> Option<Create> {
        None
    }

    fn prepare_trace_output(&self) -> Option<Bytes> {
        None
    }

    fn trace_call(
        &mut self,
        _call: Option<Call>,
        _gas_used: U256,
        _output: Option<Bytes>,
        _subs: Vec<Self::Output>,
    ) {
    }

    /// Stores trace create info.
    fn trace_create(
        &mut self,
        _create: Option<Create>,
        _gas_used: U256,
        _code: Option<Bytes>,
        _address: H160,
        _subs: Vec<Self::Output>,
    ) {
    }

    fn trace_failed_call(
        &mut self,
        _call: Option<Call>,
        _subs: Vec<Self::Output>,
        error: trace::TraceError,
    ) {
        self.traces.push(TxEvent::TraceError(error.to_string()));
    }

    fn trace_failed_create(
        &mut self,
        _create: Option<Create>,
        _subs: Vec<Self::Output>,
        error: trace::TraceError,
    ) {
        self.traces.push(TxEvent::TraceError(error.to_string()));
    }

    fn trace_suicide(&mut self, _address: H160, _balance: U256, _refund_address: H160) {}

    fn trace_reward(&mut self, _author: H160, _value: U256, _reward_type: trace::RewardType) {}

    fn subtracer(&self) -> TxTracer<'a>
    where
        Self: Sized,
    {
        TxTracer::new(self.linker.clone(), self.source)
    }

    fn drain(self) -> Vec<Self::Output> {
        self.traces
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct LineInfo {
    path: PathBuf,
    line_string: String,
    line: usize,
}

impl fmt::Display for LineInfo {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(
            fmt,
            "{}:{}: {}",
            self.path.display(),
            self.line,
            self.line_string
        )
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Revert {
    line_info: Option<LineInfo>,
}

impl fmt::Display for Revert {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match self.line_info {
            Some(ref line_info) => write!(fmt, "reverted at {}", line_info),
            None => write!(fmt, "reverted at unknown location"),
        }
    }
}

#[derive(Debug)]
pub struct TxVmOutput {
    pub revert: Option<Revert>,
}

#[derive(Debug)]
pub struct TxVmTracer<'a> {
    linker: Option<Arc<linker::Linker>>,
    // current source.
    source: &'a Mutex<Option<Arc<(source_map::SourceMap, HashMap<usize, usize>)>>>,
    depth: usize,
    pc: usize,
    instruction: u8,
    stack: Vec<U256>,
    revert: Option<Revert>,
}

impl<'a> TxVmTracer<'a> {
    pub fn new(
        linker: Option<Arc<linker::Linker>>,
        source: &'a Mutex<Option<Arc<(source_map::SourceMap, HashMap<usize, usize>)>>>,
    ) -> Self {
        TxVmTracer {
            linker,
            source,
            depth: 0,
            pc: 0,
            instruction: 0,
            stack: Vec::new(),
            revert: None,
        }
    }
}

impl<'a> TxVmTracer<'a> {
    /// Get line info from the current program counter.
    fn line_info(&self) -> Option<LineInfo> {
        let source = self.source.lock().expect("not poisoned lock");

        let (ref source, ref offsets) = match *source {
            Some(ref source) => source.as_ref(),
            None => return None,
        };

        let offset = match offsets.get(&self.pc) {
            Some(offset) => *offset,
            None => return None,
        };

        let m = match source.find_mapping(offset) {
            Some(m) => m,
            None => return None,
        };

        let path = match m.file_index.and_then(|index| {
            self.linker
                .as_ref()
                .and_then(|linker| linker.find_file(index))
        }) {
            Some(path) => path,
            None => return None,
        };

        let file = File::open(path).expect("bad file");

        let (line_string, line, _) =
            utils::find_line(file, (m.start as usize, (m.start + m.length) as usize))
                .expect("line from file");

        Some(LineInfo {
            path: path.to_owned(),
            line_string,
            line,
        })
    }
}

impl<'a> trace::VMTracer for TxVmTracer<'a> {
    type Output = TxVmOutput;

    fn trace_next_instruction(&mut self, pc: usize, instruction: u8, _current_gas: U256) -> bool {
        self.pc = pc;
        self.instruction = instruction;
        true
    }

    // Parts borrowed from: https://github.com/paritytech/sol-rs/blob/master/solaris/src/trace.rs
    fn trace_executed(
        &mut self,
        _gas_used: U256,
        _stack_push: &[U256],
        _mem_diff: Option<(usize, &[u8])>,
        _store_diff: Option<(U256, U256)>,
    ) {
        let info = parity_evm::Instruction::from_u8(self.instruction)
            .expect("legal instruction")
            .info();

        if info.name == "REVERT" {
            let line_info = self.line_info();
            self.revert = Some(Revert { line_info });
        }
    }

    fn prepare_subtrace(&self, _code: &[u8]) -> Self
    where
        Self: Sized,
    {
        let mut vm = TxVmTracer::new(self.linker.clone(), self.source);
        vm.depth = self.depth + 1;
        vm
    }

    fn done_subtrace(&mut self, sub: Self) {
        if sub.revert.is_some() {
            self.revert = sub.revert;
        }
    }

    fn drain(self) -> Option<Self::Output> {
        Some(TxVmOutput {
            revert: self.revert,
        })
    }
}
