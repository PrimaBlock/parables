use ethcore::trace;
use ethcore::trace::trace::{Call, Create};
use ethereum_types::{H160, U256};
use linker;
use parity_bytes::Bytes;
use parity_evm;
use parity_vm;
use std::cmp;
use std::fmt;
use std::fs::File;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use utils;

/// Last known frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameInfo {
    Some(usize),
    None,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ErrorKind {
    Root,
    Error(trace::TraceError),
}

impl cmp::Eq for ErrorKind {}

impl ErrorKind {
    /// Check if kind is reverted.
    pub fn is_reverted(&self) -> bool {
        match *self {
            ErrorKind::Root => false,
            ErrorKind::Error(ref e) => *e == trace::TraceError::Reverted,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorInfo {
    pub kind: ErrorKind,
    pub line_info: Option<LineInfo>,
    pub subs: Vec<ErrorInfo>,
}

impl ErrorInfo {
    /// Create a new root error info.
    pub fn new_root(subs: Vec<ErrorInfo>) -> Self {
        Self {
            kind: ErrorKind::Root,
            line_info: None,
            subs: subs,
        }
    }

    fn fmt_sub(&self, fmt: &mut fmt::Formatter, level: usize) -> fmt::Result {
        let prefix = (0..level).map(|_| "  ").collect::<String>();

        match self.kind {
            ErrorKind::Root => match self.line_info {
                Some(ref line_info) => writeln!(fmt, "{}failed at {}", prefix, line_info)?,
                None => writeln!(fmt, "{}failed", prefix)?,
            },
            ErrorKind::Error(ref e) => match self.line_info {
                Some(ref line_info) => writeln!(fmt, "{}{} at {}", prefix, e, line_info)?,
                None => writeln!(fmt, "{}{} at unknown location", prefix, e)?,
            },
        }

        for sub in &self.subs {
            sub.fmt_sub(fmt, level + 1)?;
        }

        Ok(())
    }

    /// Check if kind is reverted.
    pub fn is_reverted(&self) -> bool {
        if self.kind.is_reverted() {
            return true;
        }

        self.subs.iter().any(|e| e.is_reverted())
    }

    /// Check if error info contains a line that caused it to be reverted.
    ///
    /// This recursively looks through all sub-traces to find a match.
    pub fn is_failed_with(&self, stmt: impl AsRef<str> + Copy) -> bool {
        let stmt = stmt.as_ref();

        if let Some(ref line_info) = self.line_info {
            if line_info.line_string.trim() == stmt {
                return true;
            }
        }

        self.subs.iter().any(|e| e.is_failed_with(stmt))
    }
}

impl fmt::Display for ErrorInfo {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        self.fmt_sub(fmt, 0)
    }
}

pub struct TxTracer<'a> {
    linker: &'a linker::Linker,
    // if present, the source used when creating a contract.
    entry_source: Option<Arc<linker::Source>>,
    // program counter of last revert.
    frame_info: &'a Mutex<FrameInfo>,
    // Information about a revert.
    errors: Vec<ErrorInfo>,
}

impl<'a> TxTracer<'a> {
    pub fn new(
        linker: &'a linker::Linker,
        entry_source: Option<Arc<linker::Source>>,
        frame_info: &'a Mutex<FrameInfo>,
    ) -> Self {
        Self {
            linker,
            entry_source,
            frame_info,
            errors: Vec::new(),
        }
    }

    /// Get line info from the current program counter.
    fn line_info(&self, source: Option<&Arc<linker::Source>>, pc: usize) -> Option<LineInfo> {
        let linker::Source {
            ref source_map,
            ref offsets,
        } = match source {
            Some(source) => source.as_ref(),
            None => return None,
        };

        let offset = match offsets.get(&pc) {
            Some(offset) => *offset,
            None => return None,
        };

        let m = match source_map.find_mapping(offset) {
            Some(m) => m,
            None => return None,
        };

        let path = match m.file_index.and_then(|index| self.linker.find_file(index)) {
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

impl<'a> trace::Tracer for TxTracer<'a> {
    type Output = ErrorInfo;

    fn prepare_trace_call(&self, params: &parity_vm::ActionParams) -> Option<Call> {
        Some(Call::from(params.clone()))
    }

    fn prepare_trace_create(&self, params: &parity_vm::ActionParams) -> Option<Create> {
        Some(Create::from(params.clone()))
    }

    fn prepare_trace_output(&self) -> Option<Bytes> {
        None
    }

    fn trace_call(
        &mut self,
        _call: Option<Call>,
        _gas_used: U256,
        _output: Option<Bytes>,
        subs: Vec<Self::Output>,
    ) {
        if !subs.is_empty() {
            self.errors.extend(subs);
        }
    }

    fn trace_create(
        &mut self,
        _create: Option<Create>,
        _gas_used: U256,
        _code: Option<Bytes>,
        _address: H160,
        subs: Vec<Self::Output>,
    ) {
        if !subs.is_empty() {
            self.errors.extend(subs);
        }
    }

    fn trace_failed_call(
        &mut self,
        call: Option<Call>,
        subs: Vec<Self::Output>,
        error: trace::TraceError,
    ) {
        let call = call.expect("no call");

        let frame_info: FrameInfo = self.frame_info.lock().expect("poisoned lock").clone();

        match frame_info {
            FrameInfo::Some(pc) => {
                let source = self.linker.find_runtime_source(call.to);
                let line_info = self.line_info(source.as_ref(), pc);

                self.errors.push(ErrorInfo {
                    kind: ErrorKind::Error(error),
                    line_info,
                    subs,
                })
            }
            FrameInfo::None => self.errors.push(ErrorInfo {
                kind: ErrorKind::Error(error),
                line_info: None,
                subs: subs,
            }),
        }
    }

    fn trace_failed_create(
        &mut self,
        _create: Option<Create>,
        subs: Vec<Self::Output>,
        error: trace::TraceError,
    ) {
        let frame_info: FrameInfo = self.frame_info.lock().expect("poisoned lock").clone();

        match frame_info {
            FrameInfo::Some(pc) => {
                let line_info = self.line_info(self.entry_source.as_ref(), pc);

                self.errors.push(ErrorInfo {
                    kind: ErrorKind::Error(error),
                    line_info,
                    subs,
                })
            }
            FrameInfo::None => self.errors.push(ErrorInfo {
                kind: ErrorKind::Error(error),
                line_info: None,
                subs: subs,
            }),
        }
    }

    fn trace_suicide(&mut self, _address: H160, _balance: U256, _refund_address: H160) {}

    fn trace_reward(&mut self, _author: H160, _value: U256, _reward_type: trace::RewardType) {}

    fn subtracer(&self) -> TxTracer<'a>
    where
        Self: Sized,
    {
        TxTracer::new(self.linker, self.entry_source.clone(), self.frame_info)
    }

    fn drain(self) -> Vec<ErrorInfo> {
        self.errors
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug)]
pub struct TxVmTracer<'a> {
    linker: &'a linker::Linker,
    // if present, the source used to create a contract.
    entry_source: Option<Arc<linker::Source>>,
    // current sources.
    frame_info: &'a Mutex<FrameInfo>,
    depth: usize,
    pc: usize,
    instruction: u8,
    stack: Vec<U256>,
}

impl<'a> TxVmTracer<'a> {
    pub fn new(
        linker: &'a linker::Linker,
        entry_source: Option<Arc<linker::Source>>,
        frame_info: &'a Mutex<FrameInfo>,
    ) -> Self {
        TxVmTracer {
            linker,
            entry_source,
            frame_info,
            depth: 0,
            pc: 0,
            instruction: 0,
            stack: Vec::new(),
        }
    }
}

impl<'a> trace::VMTracer for TxVmTracer<'a> {
    type Output = ();

    fn trace_next_instruction(&mut self, pc: usize, instruction: u8, _current_gas: U256) -> bool {
        self.pc = pc;
        self.instruction = instruction;
        true
    }

    fn trace_executed(
        &mut self,
        _gas_used: U256,
        stack_push: &[U256],
        _mem_diff: Option<(usize, &[u8])>,
        _store_diff: Option<(U256, U256)>,
    ) {
        let i = parity_evm::Instruction::from_u8(self.instruction).expect("legal instruction");

        let len = self.stack.len();

        {
            let mut frame_info = self.frame_info.lock().expect("poisoned lock");
            *frame_info = FrameInfo::Some(self.pc);
        }

        let info = i.info();

        self.stack.truncate(if len >= info.args {
            len - info.args
        } else {
            0usize
        });
        self.stack.extend_from_slice(stack_push);
    }

    fn prepare_subtrace(&self, _code: &[u8]) -> Self
    where
        Self: Sized,
    {
        let mut vm = TxVmTracer::new(self.linker, self.entry_source.clone(), self.frame_info);
        vm.depth = self.depth + 1;
        vm
    }

    fn done_subtrace(&mut self, _sub: Self) {}

    fn drain(self) -> Option<Self::Output> {
        None
    }
}
