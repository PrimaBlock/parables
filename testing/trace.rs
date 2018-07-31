use ast;
use ethcore::storage;
use ethcore::trace;
use ethcore::trace::trace::{Call, Create};
use ethereum_types::{Address, H160, U256};
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
    pub variables: BTreeMap<ast::Expr, ast::Value>,
}

impl ErrorInfo {
    /// Create a new root error info.
    pub fn new_root(subs: Vec<ErrorInfo>) -> Self {
        Self {
            kind: ErrorKind::Root,
            line_info: None,
            subs,
            variables: BTreeMap::new(),
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
    pub fn is_failed_with(
        &self,
        location: impl matcher::LocationMatcher + Copy,
        stmt: impl AsRef<str> + Copy,
    ) -> bool {
        let stmt = stmt.as_ref();

        if let Some(ref line_info) = self.line_info {
            let object = line_info.object.as_ref();
            let function = line_info.function.as_ref().map(|s| s.as_str());

            if location.matches_location(object, function)
                && line_info.lines.iter().any(|l| l.trim() == stmt)
            {
                return true;
            }
        }

        self.subs.iter().any(|e| e.is_failed_with(location, stmt))
    }
}

impl fmt::Display for ErrorInfo {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match self.kind {
            ErrorKind::Root => match self.line_info {
                Some(ref line_info) => {
                    writeln!(fmt, "{}: Failed", line_info)?;

                    for (l, line) in (line_info.line..).zip(line_info.lines.iter()) {
                        writeln!(fmt, "{:>3}: {}", l + 1, line)?;
                    }
                }
                None => writeln!(fmt, "?:?: Failed")?,
            },
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
    // Information about the current frame.
    frame_info: FrameInfo,
    // Call stack.
    call_stack: Vec<CallFrame>,
}

impl Shared {
    /// Create a new instance of shared state.
    pub fn new() -> Self {
        Self {
            frame_info: FrameInfo::None,
            call_stack: vec![CallFrame::default()],
        }
    }

    /// Register an expression and the value it evaluated to.
    fn register_expr(
        c: &ast::Ast,
        ctx: &mut ast::Context,
        variables: &mut HashMap<ast::Expr, ast::Value>,
    ) -> Result<(), Error> {
        use ast::Ast::*;

        let (var, ty) = match *c {
            Identifier { ref attributes, .. } => {
                let var = ast::Expr::Identifier {
                    identifier: attributes.value.to_string(),
                };

                (var, attributes.ty.as_str())
            }
            Assignment { ref children, .. } => {
                let mut it = children.iter().map(|a| a.as_ref());

                match it.next() {
                    Some(Identifier { ref attributes, .. }) => {
                        let var = ast::Expr::Identifier {
                            identifier: attributes.value.to_string(),
                        };

                        (var, attributes.ty.as_str())
                    }
                    _ => return Ok(()),
                }
            }
            IndexAccess {
                ref attributes,
                ref children,
                ..
            } => {
                let mut it = children.iter().map(|a| a.as_ref());

                let key = match it.next() {
                    Some(Identifier { ref attributes, .. }) => attributes,
                    _ => return Ok(()),
                };

                let value = match it.next() {
                    Some(Identifier { ref attributes, .. }) => attributes,
                    _ => return Ok(()),
                };

                let var = ast::Expr::IndexAccess {
                    key: key.value.to_string(),
                    value: value.value.to_string(),
                };

                (var, attributes.ty.as_str())
            }
            MemberAccess {
                ref attributes,
                ref children,
                ..
            } => {
                let mut it = children.iter().map(|a| a.as_ref());

                let key = match it.next() {
                    Some(Identifier { ref attributes, .. }) => attributes,
                    _ => return Ok(()),
                };

                let var = ast::Expr::MemberAccess {
                    key: key.value.to_string(),
                    value: attributes.member_name.to_string(),
                };

                (var, attributes.ty.as_str())
            }
            _ => return Ok(()),
        };

        let value = ast::Type::decode(ty).value(ctx)?;

        trace!("Set: {} = {}", var, value);
        variables.insert(var.clone(), value);
        Ok(())
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
        storage: &storage::StorageAccess,
        last_function: &mut Option<Arc<ast::Function>>,
        last: &mut Option<source_map::Mapping>,
        visited_statements: &mut HashSet<ast::Src>,
        force_replace: bool,
    ) -> Result<(), Error> {
        use ast::Ast::*;
        use std::mem;

        let call_info = match self.call_stack.last_mut() {
            Some(call_info) => call_info,
            None => return Ok(()),
        };

        let current = match mapping(call_info.source.as_ref(), pc) {
            Some(current) => current,
            None => return Ok(()),
        };

        let replace = force_replace || match *last {
            None => true,
            // either the statement has changed, or we are reverting.
            Some(ref last) => last != current,
        };

        let ast = match call_info.ast {
            Some(ref ast) => ast,
            None => return Ok(()),
        };

        // TODO: can we use current.is_jump_to_function()?
        if let Some(function) = ast.find_function(&current) {
            // are we in a new function?
            let replace = match last_function.as_ref() {
                Some(last_function) => function.src != last_function.src,
                None => true,
            };

            if replace {
                mem::replace(last_function, Some(Arc::clone(function)));
                call_info.function = Some(Arc::clone(function));

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

        let from = match ast.find(&last) {
            Some(ast) => ast,
            None => return Ok(()),
        };

        let to = match ast.find(&current) {
            Some(ast) => ast,
            None => return Ok(()),
        };

        trace!("AST: {} -> {}", from.kind(), to.kind());

        visited_statements.insert(from.source().clone());

        match *from {
            // Expressions are statements where we register the last set of seen variables to be
            // printed in case of an exception.
            ExpressionStatement { .. } => {
                call_info.variables = mem::replace(&mut call_info.seen_variables, HashMap::new());
            }
            ref ast => {
                let mut ctx = ast::Context::new(stack, memory, storage, &call_info.call_data);
                Self::register_expr(ast, &mut ctx, &mut call_info.seen_variables)?;
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
        function: Option<&Arc<ast::Function>>,
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

        let (lines, line) =
            utils::find_line(file, (m.start as usize, (m.start + m.length) as usize))
                .expect("line from file");

        let object = source.map(|s| s.object.clone());

        // record function name if it is known.
        let function = match function {
            Some(function) => Some(function.name.to_string()),
            None => None,
        };

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

    fn prepare_trace_call(&self, params: &parity_vm::ActionParams) -> Option<Call> {
        // ignore built-in calls since they don't call trace_call correctly:
        // https://github.com/paritytech/parity-ethereum/pull/9236
        if params.code_address == Address::from(0x1) {
            return None;
        }

        let mut shared = self.shared.lock().expect("lock poisoned");

        let mut info = CallFrame::from(self.linker.find_runtime_info(params.code_address));

        info.call_data = params.data.clone().unwrap_or_else(Bytes::default);

        debug!(
            ">> {:03}: Prepare Trace Call: {:?} (address: {:?}, call_type: {:?})",
            self.depth, info.source, params.code_address, params.call_type,
        );

        shared.call_stack.push(info);
        None
    }

    fn prepare_trace_create(&self, params: &parity_vm::ActionParams) -> Option<Create> {
        let mut shared = self.shared.lock().expect("lock poisoned");
        let source = self.entry_source.clone();
        let ast = source
            .as_ref()
            .and_then(|s| self.linker.find_ast_by_object(&s.object));

        let info = CallFrame {
            source,
            ast,
            call_data: params.data.clone().unwrap_or_else(Bytes::default),
            seen_variables: HashMap::new(),
            variables: HashMap::new(),
            function: None,
        };

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

        let variables: BTreeMap<_, _> = match info.as_ref() {
            Some(info) => info.variables.clone().into_iter().collect(),
            None => BTreeMap::new(),
        };

        let function = info.as_ref().and_then(|i| i.function.as_ref());

        match shared.frame_info.clone() {
            FrameInfo::Some(pc) => {
                let line_info = shared.line_info(self.linker, source, pc, function);

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

        let variables: BTreeMap<_, _> = match info.as_ref() {
            Some(info) => info.variables.clone().into_iter().collect(),
            None => BTreeMap::new(),
        };

        let function = info.as_ref().and_then(|i| i.function.as_ref());

        match shared.frame_info.clone() {
            FrameInfo::Some(pc) => {
                let line_info = shared.line_info(self.linker, source, pc, function);

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

    fn subtracer(&self) -> Tracer<'a>
    where
        Self: Sized,
    {
        debug!("!! {:03}: New Sub-Tracer", self.depth);

        Tracer {
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
        let head = shared.call_stack.last();

        let source = head.as_ref().and_then(|s| s.source.as_ref());

        debug!(
            "<< {:03}: Drain: {:?} ({:?})",
            self.depth, source, self.operation
        );

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
    /// Current memory.
    memory: Vec<u8>,
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
            memory: Vec::new(),
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

    fn trace_executed(
        &mut self,
        _gas_used: U256,
        stack_push: &[U256],
        mem_diff: Option<(usize, &[u8])>,
        _store_diff: Option<(U256, U256)>,
        storage: &storage::StorageAccess,
    ) {
        let mut shared = self.shared.lock().expect("poisoned lock");

        if let Err(e) = shared.decode_instruction(
            self.pc,
            &self.stack,
            &self.memory,
            storage,
            &mut self.last_function,
            &mut self.last,
            &mut self.visited_statements,
            false,
        ) {
            warn!("Failed to decode: {}", e);
        }

        let inst = self.instruction.expect("illegal instruction");
        trace!(
            "I {:<4x}: {:<16}: {:?}",
            self.pc,
            inst.info().name,
            self.stack
        );

        let len = self.stack.len();
        shared.frame_info = FrameInfo::Some(self.pc);

        let info = inst.info();

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
    }

    fn trace_done(&mut self, storage: &storage::StorageAccess) {
        let mut shared = self.shared.lock().expect("poisoned lock");

        if let Err(e) = shared.decode_instruction(
            self.pc,
            &self.stack,
            &self.memory,
            storage,
            &mut self.last_function,
            &mut self.last,
            &mut self.visited_statements,
            true,
        ) {
            warn!("Failed to decode: {}", e);
        }
    }

    fn prepare_subtrace(&self, _code: &[u8]) -> Self
    where
        Self: Sized,
    {
        VmTracer::new(self.linker, self.entry_source.clone(), self.shared)
    }

    fn done_subtrace(&mut self, sub: Self) {
        if let Some(s) = sub.drain() {
            self.visited_statements.extend(s.visited_statements);
        }
    }

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
    /// Source associated with an address.
    pub source: Option<Arc<linker::Source>>,
    /// AST associated with an address.
    pub ast: Option<Arc<ast::Registry>>,
    /// Input data for the current call frame.
    pub call_data: Bytes,
    // named variables and their stack offsets.
    variables: HashMap<ast::Expr, ast::Value>,
    // Last set of variables seen up until an expression.
    seen_variables: HashMap<ast::Expr, ast::Value>,
    // Function call stack.
    function: Option<Arc<ast::Function>>,
}

impl From<linker::AddressInfo> for CallFrame {
    fn from(info: linker::AddressInfo) -> Self {
        Self {
            source: info.source,
            ast: info.ast,
            call_data: Bytes::default(),
            variables: HashMap::new(),
            seen_variables: HashMap::new(),
            function: None,
        }
    }
}
