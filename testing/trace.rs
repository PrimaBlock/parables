use ast;
use ethcore::trace;
use ethereum_types::{H160, U256};
use failure::Error;
use linker;
use matcher;
use parity_bytes::Bytes;
use parity_evm;
use parity_vm;
use source_map;
use std::cmp;
use std::collections::{BTreeMap, HashMap, HashSet};
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

impl Default for FrameInfo {
    fn default() -> Self {
        FrameInfo::None
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ErrorKind {
    Error(parity_vm::Error),
}

impl cmp::Eq for ErrorKind {}

impl ErrorKind {
    /// Check if kind is reverted.
    pub fn is_reverted(&self) -> bool {
        match *self {
            ErrorKind::Error(ref e) => *e == parity_vm::Error::Reverted,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Errors {
    errors: Vec<ErrorInfo>,
}

impl Errors {
    /// Create a new root error info.
    pub fn new(errors: Vec<ErrorInfo>) -> Self {
        Self { errors }
    }

    /// Check if kind is reverted.
    pub fn is_reverted(&self) -> bool {
        self.errors.iter().any(ErrorInfo::is_reverted)
    }

    /// Check if error info contains a line that caused it to be reverted.
    ///
    /// This looks through all errors to find a match.
    pub fn is_failed_with(
        &self,
        location: impl matcher::LocationMatcher,
        stmt: impl matcher::StatementMatcher,
    ) -> bool {
        for e in &self.errors {
            if let Some(ref line_info) = e.line_info {
                let object = line_info.object.as_ref();
                let function = line_info.function.as_ref().map(|s| s.as_str());

                if location.matches_location(object, function)
                    && stmt.matches_lines(&line_info.lines)
                {
                    return true;
                }
            }
        }

        false
    }
}

impl fmt::Display for Errors {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        for (i, e) in self.errors.iter().rev().enumerate() {
            write!(fmt, "Frame #{}: {}", i, e)?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorInfo {
    pub kind: ErrorKind,
    pub line_info: Option<LineInfo>,
    /// Local variables and their corresponding values at the time of error.
    pub variables: BTreeMap<ast::Expr, ast::Value>,
}

impl ErrorInfo {
    /// Check if kind is reverted.
    pub fn is_reverted(&self) -> bool {
        self.kind.is_reverted()
    }
}

impl fmt::Display for ErrorInfo {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match self.kind {
            ErrorKind::Error(ref e) => match self.line_info {
                Some(ref line_info) => {
                    writeln!(fmt, "{}: {}", line_info, e)?;

                    for (l, line) in (line_info.line..).zip(line_info.lines.iter()) {
                        writeln!(fmt, " {:>3}: {}", l + 1, line)?;
                    }
                }
                None => writeln!(fmt, "?:?: {}", e)?,
            },
        }

        if !self.variables.is_empty() {
            writeln!(fmt, "Expressions:")?;

            let mut it = self.variables.iter();

            while let Some((var, value)) = it.next() {
                writeln!(fmt, "  {} = {}", var, value)?;
            }
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
    object: Option<linker::Object>,
    function: Option<String>,
    line: usize,
    lines: Vec<String>,
}

impl fmt::Display for LineInfo {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "{}:{}", self.path.display(), self.line + 1)?;

        if let Some(ref name) = self.function {
            write!(fmt, ":{}", name)?;
        } else {
            write!(fmt, ":?")?;
        }

        Ok(())
    }
}

#[derive(Debug)]
pub struct Shared {
    // Call stack.
    call_stack: Vec<CallFrame>,
}

impl Shared {
    /// Create a new instance of shared state.
    pub fn new() -> Self {
        Self { call_stack: vec![] }
    }

    // Decode the current statement according to its AST.
    //
    // `pc` - the current program counter.
    //
    // This will try to decode any variable assignments.
    //
    // NOTE: AST searching is currently not indexed correctly making it rather slow.
    fn decode_instruction(
        &mut self,
        pc: usize,
        stack: &[U256],
        memory: &[u8],
        last_function: &mut Option<Arc<ast::Function>>,
        last: &mut Option<source_map::Mapping>,
        visited_statements: &mut HashSet<ast::Src>,
        force_replace: bool,
    ) -> Result<(), Error> {
        use ast::Ast::*;
        use std::mem;

        let frame = match self.call_stack.last_mut() {
            Some(frame) => frame,
            None => return Ok(()),
        };

        frame.frame_info = FrameInfo::Some(pc);

        let current = match mapping(frame.source.as_ref(), pc) {
            Some(current) => current,
            None => return Ok(()),
        };

        let replace = force_replace || match *last {
            None => true,
            // either the statement has changed, or we are reverting.
            Some(ref last) => last != current,
        };

        let registry = match frame.ast {
            Some(ref registry) => registry,
            None => return Ok(()),
        };

        // TODO: can we use current.is_jump_to_function()?
        if let Some(function) = registry.find_function(&current) {
            // are we in a new function?
            let replace = match last_function.as_ref() {
                Some(last_function) => function.src != last_function.src,
                None => true,
            };

            if replace {
                mem::replace(last_function, Some(Arc::clone(function)));
                frame.function = Some(Arc::clone(function));

                debug!(
                    "In Function: {}: {:?} from {:?}",
                    function.name, function.src, current
                );
            }
        }

        // No change in AST.
        if !replace {
            return Ok(());
        }

        let last = match mem::replace(last, Some(current.clone())) {
            Some(last) => last,
            // initial statement
            None => return Ok(()),
        };

        let from = match registry.find(&last) {
            Some(ast) => ast,
            None => return Ok(()),
        };

        let to = match registry.find(&current) {
            Some(ast) => ast,
            None => return Ok(()),
        };

        trace!("AST: {} -> {}", from.kind(), to.kind());

        visited_statements.insert(from.source().clone());

        match *from {
            // Expressions are statements where we register the last set of seen variables to be
            // printed in case of an exception.
            ExpressionStatement { .. } => {
                frame.variables.clear();
            }
            ref c => {
                let mut ctx = ast::Context::new(stack, memory, &frame.call_data);

                // Register an expression and the value it evaluated to.
                if let Some((var, ty)) = registry.decode_ast(c) {
                    let value = registry.decode_type(ty).value(&mut ctx)?;
                    trace!("Set: {} = {}", var, value);
                    frame.variables.insert(var, value);
                }
            }
        }

        Ok(())
    }

    /// Get line info from the current program counter.
    fn line_info(
        &self,
        linker: &linker::Linker,
        source: Option<&Arc<linker::Source>>,
        pc: usize,
        function: Option<&ast::Function>,
    ) -> Option<LineInfo> {
        let m = match mapping(source, pc) {
            Some(m) => m,
            None => return None,
        };

        let path = match m.file_index.and_then(|index| linker.find_file(index)) {
            Some(path) => path,
            None => return None,
        };

        let function = function.map(|f| f.name.to_string());
        let file = File::open(path).expect("bad file");

        let (lines, line) =
            utils::find_line(file, (m.start as usize, (m.start + m.length) as usize))
                .expect("line from file");

        let object = source.map(|s| s.object.clone());

        Some(LineInfo {
            path: path.to_owned(),
            object,
            function,
            line,
            lines,
        })
    }
}

/// Call tracer.
pub struct Tracer<'a> {
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

impl<'a> Tracer<'a> {
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

impl<'a> trace::Tracer for Tracer<'a> {
    type Output = ErrorInfo;

    fn prepare_trace_call(
        &mut self,
        params: &parity_vm::ActionParams,
        _depth: usize,
        _is_builtin: bool,
    ) {
        let mut shared = self.shared.lock().expect("lock poisoned");

        let mut frame = CallFrame::from(self.linker.find_runtime_info(params.code_address));
        frame.call_data = params.data.clone().unwrap_or_else(Bytes::default);

        debug!(
            ">> {:03}: Prepare Trace Call: {:?} (address: {:?}, call_type: {:?})",
            self.depth, frame.source, params.code_address, params.call_type,
        );

        shared.call_stack.push(frame);
    }

    fn prepare_trace_create(&mut self, params: &parity_vm::ActionParams) {
        let mut shared = self.shared.lock().expect("lock poisoned");
        let source = self.entry_source.clone();
        let ast = source
            .as_ref()
            .and_then(|s| self.linker.find_ast_by_object(&s.object));

        let info = CallFrame {
            frame_info: FrameInfo::None,
            source,
            ast,
            call_data: params.data.clone().unwrap_or_else(Bytes::default),
            variables: HashMap::new(),
            function: None,
        };

        debug!(
            ">> {:03}: Prepare Trace Create: {:?}",
            self.depth, info.source
        );

        shared.call_stack.push(info);
    }

    fn done_trace_call(&mut self, _gas_used: U256, _output: &[u8]) {
        let mut shared = self.shared.lock().expect("lock poisoned");
        let info = shared.call_stack.pop();
        let source = info.as_ref().and_then(|s| s.source.as_ref());

        debug!("!! {:03}: Trace Call: {:?}", self.depth, source);
        self.operation = Operation::Call;
    }

    fn done_trace_create(&mut self, _gas_used: U256, _code: &[u8], address: H160) {
        let mut shared = self.shared.lock().expect("lock poisoned");
        let info = shared.call_stack.pop();
        let source = info.as_ref().and_then(|s| s.source.as_ref());

        debug!(
            "!! {:03}: Trace Create: {:?} ({})",
            self.depth, source, address
        );

        self.operation = Operation::Create;
    }

    fn done_trace_failed(&mut self, error: &parity_vm::Error) {
        let mut shared = self.shared.lock().expect("lock poisoned");

        let CallFrame {
            source,
            variables,
            function,
            frame_info,
            ..
        } = shared.call_stack.pop().expect("call frame missing");

        debug!(
            "!! {:03}: Trace Failed Call: {:?} ({})",
            self.depth, source, error
        );

        let variables: BTreeMap<_, _> = variables.into_iter().collect();

        match frame_info {
            FrameInfo::Some(pc) => {
                let function = function.as_ref().map(|f| f.as_ref());
                let line_info = shared.line_info(self.linker, source.as_ref(), pc, function);

                self.errors.push(ErrorInfo {
                    kind: ErrorKind::Error(error.clone()),
                    line_info,
                    variables,
                })
            }
            FrameInfo::None => self.errors.push(ErrorInfo {
                kind: ErrorKind::Error(error.clone()),
                line_info: None,
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

    fn drain(self) -> Vec<ErrorInfo> {
        self.errors
    }
}

#[derive(Debug)]
pub struct VmTracerOutput {
    /// Statements which have been visited.
    pub visited_statements: HashSet<ast::Src>,
}

/// Instruction tracer.
#[derive(Debug)]
pub struct VmTracer<'a> {
    linker: &'a linker::Linker,
    /// If present, the source used to create a contract.
    entry_source: Option<Arc<linker::Source>>,
    /// Current program counter.
    pc: usize,
    /// Current instruction.
    instruction: Option<parity_evm::Instruction>,
    /// Current stack.
    stack: Vec<U256>,
    /// Last evaluated function.
    last_function: Option<Arc<ast::Function>>,
    /// Last evaluated mapping.
    last: Option<source_map::Mapping>,
    /// Shared state between tracers.
    shared: &'a Mutex<Shared>,
    /// Statements which have been visited.
    visited_statements: HashSet<ast::Src>,
}

impl<'a> VmTracer<'a> {
    pub fn new(
        linker: &'a linker::Linker,
        entry_source: Option<Arc<linker::Source>>,
        shared: &'a Mutex<Shared>,
    ) -> Self {
        VmTracer {
            linker,
            entry_source,
            pc: 0,
            instruction: None,
            stack: Vec::new(),
            last_function: None,
            last: None,
            shared,
            visited_statements: HashSet::new(),
        }
    }
}

impl<'a> trace::VMTracer for VmTracer<'a> {
    type Output = VmTracerOutput;

    fn trace_next_instruction(&mut self, pc: usize, instruction: u8, _current_gas: U256) -> bool {
        self.pc = pc;
        self.instruction = parity_evm::Instruction::from_u8(instruction);
        true
    }

    fn trace_executed(&mut self, _gas_used: U256, stack_push: &[U256], mem: &[u8]) {
        let mut shared = self.shared.lock().expect("poisoned lock");

        if let Err(e) = shared.decode_instruction(
            self.pc,
            &self.stack,
            mem,
            &mut self.last_function,
            &mut self.last,
            &mut self.visited_statements,
            false,
        ) {
            warn!("Failed to decode: {}", e);
        }

        let inst = self.instruction.expect("illegal instruction");
        trace!("I {:<4x}: {:<16}", self.pc, inst.info().name,);

        let len = self.stack.len();

        let info = inst.info();

        self.stack.truncate(if len >= info.args {
            len - info.args
        } else {
            0usize
        });

        self.stack.extend_from_slice(stack_push);

        // print post-stack manipulation state.
        // trace!("{:<24}= {:?}", "", self.stack);
    }

    fn prepare_subtrace(&mut self, _code: &[u8]) {}
    fn done_subtrace(&mut self) {}

    fn drain(self) -> Option<Self::Output> {
        let visited_statements = self.visited_statements;
        Some(VmTracerOutput { visited_statements })
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
    /// Information about the current frame.
    frame_info: FrameInfo,
    /// Source associated with an address.
    pub source: Option<Arc<linker::Source>>,
    /// AST associated with an address.
    pub ast: Option<Arc<ast::Registry>>,
    /// Input data for the current call frame.
    pub call_data: Bytes,
    // named variables and their stack offsets.
    variables: HashMap<ast::Expr, ast::Value>,
    // Function call stack.
    function: Option<Arc<ast::Function>>,
}

impl From<linker::AddressInfo> for CallFrame {
    fn from(info: linker::AddressInfo) -> Self {
        Self {
            frame_info: FrameInfo::None,
            source: info.source,
            ast: info.ast,
            call_data: Bytes::default(),
            variables: HashMap::new(),
            function: None,
        }
    }
}
