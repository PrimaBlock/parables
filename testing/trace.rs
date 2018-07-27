use ast;
use ethcore::trace;
use ethcore::trace::trace::{Call, Create};
use ethereum_types::{Address, H160, U256};
use linker;
use parity_bytes::Bytes;
use parity_evm;
use parity_vm;
use source_map;
use std::cmp;
use std::collections::HashMap;
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
    /// Local variables and their corresponding values at the time of error.
    pub variables: HashMap<String, Option<U256>>,
}

impl ErrorInfo {
    /// Create a new root error info.
    pub fn new_root(subs: Vec<ErrorInfo>) -> Self {
        Self {
            kind: ErrorKind::Root,
            line_info: None,
            subs,
            variables: HashMap::new(),
        }
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
        match self.kind {
            ErrorKind::Root => match self.line_info {
                Some(ref line_info) => writeln!(fmt, "failed at {}", line_info)?,
                None => writeln!(fmt, "failed")?,
            },
            ErrorKind::Error(ref e) => match self.line_info {
                Some(ref line_info) => writeln!(fmt, "{} at {}", e, line_info)?,
                None => writeln!(fmt, "{} at unknown location", e)?,
            },
        }

        if !self.variables.is_empty() {
            writeln!(fmt, "  Variables:")?;

            let mut it = self.variables.iter();

            while let Some((name, value)) = it.next() {
                let value = value
                    .as_ref()
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "*unknown*".to_string());

                writeln!(fmt, "    {} = {}", name, value)?;
            }
        }

        for sub in &self.subs {
            sub.fmt(fmt)?;
        }

        Ok(())
    }
}

#[derive(Debug)]
pub enum Operation {
    None,
    Create,
    Call,
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
pub struct Shared {
    // Information about the current frame.
    frame_info: FrameInfo,
    // Call stack.
    call_stack: Vec<CallFrame>,
    // Last set of identifiers.
    identifiers: Vec<String>,
    // named variables and their stack offsets.
    variables: HashMap<String, U256>,
}

impl Shared {
    /// Create a new instance of shared state.
    pub fn new() -> Self {
        Self {
            frame_info: FrameInfo::None,
            call_stack: vec![CallFrame::default()],
            identifiers: Vec::new(),
            variables: HashMap::new(),
        }
    }

    // Decode the current statement according to its AST.
    //
    // This will try to decode any variable assignments.
    //
    // NOTE: AST searching is currently not indexed correctly making it rather slow.
    fn decode(
        &mut self,
        pc: usize,
        i: &parity_evm::Instruction,
        stack: &Vec<U256>,
        last_mapping: &mut Option<source_map::Mapping>,
        refs: &mut HashMap<u32, String>,
    ) {
        use std::mem;

        let info = match self.call_stack.last() {
            Some(info) => info,
            None => return,
        };

        let mapping = match mapping(info.source.as_ref(), pc) {
            Some(mapping) => mapping,
            None => return,
        };

        let replace = match *last_mapping {
            None => true,
            Some(ref last_mapping) => {
                // either the statement has changed, or we are reverting.
                last_mapping != mapping || parity_evm::Instruction::REVERT == *i
            }
        };

        // No change in AST.
        if !replace {
            return;
        }

        self.identifiers = Vec::new();

        let last_mapping = match mem::replace(last_mapping, Some(mapping.clone())) {
            Some(last_mapping) => last_mapping,
            // initial statement
            None => return,
        };

        let ast = match info.ast {
            Some(ref ast) => ast,
            None => return,
        };

        let ast = match ast.find(last_mapping.start, last_mapping.length) {
            Some(ast) => ast,
            None => return,
        };

        match *ast {
            ast::Ast::Identifier { ref attributes, .. } => {
                debug!("Identifier: {:?}", attributes);
            }
            ref ast => {
                trace!("AST: {}", ast.kind());
            }
        }

        match *ast {
            ast::Ast::FunctionDefinition { ref children, .. } => {
                let children = match children.iter().next() {
                    Some(ast::Ast::ParameterList { ref children, .. }) => children,
                    _ => return,
                };

                let mut parameters = Vec::new();

                for c in children {
                    let (id, attributes) = match *c {
                        ast::Ast::VariableDeclaration {
                            ref id,
                            ref attributes,
                            ..
                        } => (id, attributes),
                        _ => return,
                    };

                    parameters.push(attributes.name.to_string());
                }

                trace!("Function Parameters: {:?}: {:?}", parameters, stack);
            }
            ast::Ast::VariableDeclarationStatement { ref children, .. } => {
                let mut it = children.iter();

                let (id, attributes) = match it.next() {
                    Some(ast::Ast::VariableDeclaration {
                        ref id,
                        ref attributes,
                        ..
                    }) => (id, attributes),
                    _ => return,
                };

                let value = match stack.last() {
                    Some(value) => *value,
                    None => return,
                };

                debug!("Declare: {} = {}", attributes.name, value);

                self.variables.insert(attributes.name.to_string(), value);
                refs.insert(*id, attributes.name.to_string());
            }
            ast::Ast::ExpressionStatement { ref children, .. } => {
                // update dumped identifiers on expressions.
                {
                    let mut idents = Vec::new();

                    for c in children {
                        idents.extend(c.identifiers().map(|s| s.to_string()));
                    }

                    debug!("Register Identifiers: {} {:?}", ast.kind(), idents);
                    self.identifiers = idents;
                }

                let first = match children.first() {
                    Some(first) => first,
                    None => return,
                };

                match *first {
                    ast::Ast::Assignment { ref children, .. } => {
                        let mut it = children.iter();

                        let var = match it.next() {
                            Some(ast::Ast::Identifier { ref attributes, .. }) => attributes,
                            _ => return,
                        };

                        let value = match stack.last() {
                            Some(value) => *value,
                            None => return,
                        };

                        debug!("Assignment: {} = {}", var.value, value);

                        self.variables.insert(var.value.to_string(), value);
                    }
                    _ => return,
                }
            }
            _ => return,
        }
    }

    /// Get all variables for the last expression evaluated.
    fn variables(&self) -> HashMap<String, Option<U256>> {
        let mut out = HashMap::new();

        for i in self.identifiers.iter() {
            out.insert(i.to_string(), self.variables.get(i).map(|v| *v));
        }

        out
    }

    /// Get line info from the current program counter.
    fn line_info(
        &self,
        linker: &linker::Linker,
        source: Option<&Arc<linker::Source>>,
        pc: usize,
    ) -> Option<LineInfo> {
        let m = match mapping(source, pc) {
            Some(m) => m,
            None => return None,
        };

        let path = match m.file_index.and_then(|index| linker.find_file(index)) {
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

/// Call tracer.
pub struct TxTracer<'a> {
    linker: &'a linker::Linker,
    // if present, the source used when creating a contract.
    entry_source: Option<Arc<linker::Source>>,
    // Information about a revert.
    errors: Vec<ErrorInfo>,
    // operation prepare.
    operation: Operation,
    // depth of the tracer.
    depth: usize,
    // shared state between tracers.
    shared: &'a Mutex<Shared>,
}

impl<'a> TxTracer<'a> {
    pub fn new(
        linker: &'a linker::Linker,
        entry_source: Option<Arc<linker::Source>>,
        shared: &'a Mutex<Shared>,
    ) -> Self {
        Self {
            linker,
            entry_source,
            errors: Vec::new(),
            operation: Operation::None,
            depth: 0,
            shared,
        }
    }
}

impl<'a> trace::Tracer for TxTracer<'a> {
    type Output = ErrorInfo;

    fn prepare_trace_call(&self, params: &parity_vm::ActionParams) -> Option<Call> {
        // ignore built-in calls since they don't call trace_call correctly:
        // https://github.com/paritytech/parity-ethereum/pull/9236
        if params.code_address == Address::from(0x1) {
            return None;
        }

        let mut shared = self.shared.lock().expect("lock poisoned");

        let info = CallFrame::from(self.linker.find_runtime_info(params.code_address));

        debug!(
            ">> {:03}: Prepare Trace Call: {:?} (address: {:?}, call_type: {:?})",
            self.depth, info.source, params.code_address, params.call_type,
        );

        shared.call_stack.push(info);
        None
    }

    fn prepare_trace_create(&self, _: &parity_vm::ActionParams) -> Option<Create> {
        let mut shared = self.shared.lock().expect("lock poisoned");
        let source = self.entry_source.clone();
        let ast = source
            .as_ref()
            .and_then(|s| self.linker.find_ast_by_object(&s.object));

        let info = CallFrame { source, ast };

        debug!(
            ">> {:03}: Prepare Trace Create: {:?}",
            self.depth, info.source
        );

        shared.call_stack.push(info);
        None
    }

    fn prepare_trace_output(&self) -> Option<Bytes> {
        let shared = self.shared.lock().expect("lock poisoned");
        let source = shared.call_stack.last().and_then(|s| s.source.as_ref());

        debug!("!! {:03}: Prepare Trace Output: {:?}", self.depth, source);
        None
    }

    fn trace_call(
        &mut self,
        _call: Option<Call>,
        _gas_used: U256,
        _output: Option<Bytes>,
        subs: Vec<Self::Output>,
    ) {
        let mut shared = self.shared.lock().expect("lock poisoned");
        let info = shared.call_stack.pop();
        let source = info.as_ref().and_then(|s| s.source.as_ref());

        debug!("!! {:03}: Trace Call: {:?}", self.depth, source);

        if !subs.is_empty() {
            self.errors.extend(subs);
        }

        self.operation = Operation::Call;
    }

    fn trace_create(
        &mut self,
        _create: Option<Create>,
        _gas_used: U256,
        _code: Option<Bytes>,
        address: H160,
        subs: Vec<Self::Output>,
    ) {
        let mut shared = self.shared.lock().expect("lock poisoned");
        let info = shared.call_stack.pop();
        let source = info.as_ref().and_then(|s| s.source.as_ref());

        debug!(
            "!! {:03}: Trace Create: {:?} ({})",
            self.depth, source, address
        );

        if !subs.is_empty() {
            self.errors.extend(subs);
        }

        self.operation = Operation::Create;
    }

    fn trace_failed_call(
        &mut self,
        _call: Option<Call>,
        subs: Vec<Self::Output>,
        error: trace::TraceError,
    ) {
        let mut shared = self.shared.lock().expect("lock poisoned");
        let info = shared.call_stack.pop();
        let source = info.as_ref().and_then(|s| s.source.as_ref());

        debug!(
            "!! {:03}: Trace Failed Call: {:?} ({})",
            self.depth, source, error
        );

        let variables = shared.variables();

        match shared.frame_info.clone() {
            FrameInfo::Some(pc) => {
                let line_info = shared.line_info(self.linker, source, pc);

                self.errors.push(ErrorInfo {
                    kind: ErrorKind::Error(error),
                    line_info,
                    subs,
                    variables,
                })
            }
            FrameInfo::None => self.errors.push(ErrorInfo {
                kind: ErrorKind::Error(error),
                line_info: None,
                subs,
                variables,
            }),
        }
    }

    fn trace_failed_create(
        &mut self,
        _create: Option<Create>,
        subs: Vec<Self::Output>,
        error: trace::TraceError,
    ) {
        let mut shared = self.shared.lock().expect("lock poisoned");
        let info = shared.call_stack.pop();
        let source = info.as_ref().and_then(|s| s.source.as_ref());

        debug!(
            "!! {:03}: Trace Failed Create: {:?} ({})",
            self.depth, source, error
        );

        let variables = shared.variables();

        match shared.frame_info.clone() {
            FrameInfo::Some(pc) => {
                let line_info = shared.line_info(self.linker, source, pc);

                self.errors.push(ErrorInfo {
                    kind: ErrorKind::Error(error),
                    line_info,
                    subs,
                    variables,
                })
            }
            FrameInfo::None => self.errors.push(ErrorInfo {
                kind: ErrorKind::Error(error),
                line_info: None,
                subs,
                variables,
            }),
        }
    }

    fn trace_suicide(&mut self, _address: H160, _balance: U256, _refund_address: H160) {
        let shared = self.shared.lock().expect("lock poisoned");
        let source = shared.call_stack.last().and_then(|s| s.source.as_ref());

        debug!("!! {:03}: Trace Suicide: {:?}", self.depth, source,);
    }

    fn trace_reward(&mut self, _author: H160, _value: U256, _reward_type: trace::RewardType) {
        let shared = self.shared.lock().expect("lock poisoned");
        let source = shared.call_stack.last().and_then(|s| s.source.as_ref());

        debug!("!! {:03}: Trace Reward: {:?}", self.depth, source,);
    }

    fn subtracer(&self) -> TxTracer<'a>
    where
        Self: Sized,
    {
        debug!("!! {:03}: New Sub-Tracer", self.depth);

        TxTracer {
            linker: self.linker,
            entry_source: self.entry_source.clone(),
            errors: Vec::new(),
            operation: Operation::None,
            depth: self.depth + 1,
            shared: self.shared,
        }
    }

    fn drain(self) -> Vec<ErrorInfo> {
        let shared = self.shared.lock().expect("lock poisoned");
        let source = shared
            .call_stack
            .last()
            .as_ref()
            .and_then(|s| s.source.as_ref());

        debug!(
            "<< {:03}: Drain: {:?} ({:?})",
            self.depth, source, self.operation
        );

        self.errors
    }
}

/// Instruction tracer.
#[derive(Debug)]
pub struct TxVmTracer<'a> {
    linker: &'a linker::Linker,
    /// If present, the source used to create a contract.
    entry_source: Option<Arc<linker::Source>>,
    /// Current program counter.
    pc: usize,
    /// Current instruction.
    instruction: Option<parity_evm::Instruction>,
    /// Current stack.
    stack: Vec<U256>,
    /// Current memory.
    memory: Vec<u8>,
    /// Last evaluated mapping.
    last_mapping: Option<source_map::Mapping>,
    /// All locally references variable declarations.
    refs: HashMap<u32, String>,
    /// Shared state between tracers.
    shared: &'a Mutex<Shared>,
}

impl<'a> TxVmTracer<'a> {
    pub fn new(
        linker: &'a linker::Linker,
        entry_source: Option<Arc<linker::Source>>,
        shared: &'a Mutex<Shared>,
    ) -> Self {
        TxVmTracer {
            linker,
            entry_source,
            pc: 0,
            instruction: None,
            stack: Vec::new(),
            memory: Vec::new(),
            last_mapping: None,
            refs: HashMap::new(),
            shared,
        }
    }
}

impl<'a> trace::VMTracer for TxVmTracer<'a> {
    type Output = ();

    fn trace_next_instruction(&mut self, pc: usize, instruction: u8, _current_gas: U256) -> bool {
        self.pc = pc;
        self.instruction = parity_evm::Instruction::from_u8(instruction);
        let i = self.instruction.expect("illegal instruction");
        trace!("I {:<4x}: {:<16}: {:?}", pc, i.info().name, self.stack);
        true
    }

    fn trace_executed(
        &mut self,
        _gas_used: U256,
        stack_push: &[U256],
        mem_diff: Option<(usize, &[u8])>,
        _store_diff: Option<(U256, U256)>,
    ) {
        let mut shared = self.shared.lock().expect("poisoned lock");
        let i = self.instruction.expect("illegal instruction");

        let len = self.stack.len();
        shared.frame_info = FrameInfo::Some(self.pc);

        let info = i.info();

        self.stack.truncate(if len >= info.args {
            len - info.args
        } else {
            0usize
        });

        self.stack.extend_from_slice(stack_push);

        if let Some((pos, slice)) = mem_diff {
            let len = pos + slice.len();

            if self.memory.len() < len {
                let rest = len - self.memory.len();
                self.memory.extend(::std::iter::repeat(0u8).take(rest));
            }

            self.memory[pos..(pos + slice.len())].copy_from_slice(slice);
            trace!("M {:<4x} length:{}", pos, slice.len());
        }

        // print post-stack manipulation state.
        trace!("{:<24}= {:?}", "", self.stack);

        shared.decode(
            self.pc,
            &i,
            &self.stack,
            &mut self.last_mapping,
            &mut self.refs,
        );
    }

    fn prepare_subtrace(&self, _code: &[u8]) -> Self
    where
        Self: Sized,
    {
        TxVmTracer::new(self.linker, self.entry_source.clone(), self.shared)
    }

    fn done_subtrace(&mut self, _sub: Self) {}

    fn drain(self) -> Option<Self::Output> {
        None
    }
}

/// Find the corresponding mapping for a source and program counter.
fn mapping<'a>(
    source: Option<&'a Arc<linker::Source>>,
    pc: usize,
) -> Option<&'a source_map::Mapping> {
    let linker::Source {
        ref source_map,
        ref offsets,
        ..
    } = match source {
        Some(source) => source.as_ref(),
        None => return None,
    };

    let offset = match offsets.get(&pc) {
        Some(offset) => *offset,
        None => return None,
    };

    source_map.find_mapping(offset)
}

/// Information about the current call.
#[derive(Debug, Default)]
pub struct CallFrame {
    /// Source associated with an address.
    pub source: Option<Arc<linker::Source>>,
    /// AST associated with an address.
    pub ast: Option<Arc<ast::Ast>>,
}

impl From<linker::AddressInfo> for CallFrame {
    fn from(info: linker::AddressInfo) -> Self {
        Self {
            source: info.source,
            ast: info.ast,
        }
    }
}
