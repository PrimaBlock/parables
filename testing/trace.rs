use ethcore::trace;
use ethcore::trace::trace::{Call, Create};
use ethereum_types::{H160, U256};
use parity_bytes::Bytes;
use parity_evm;
use parity_vm;

#[derive(Debug, PartialEq, Eq)]
pub enum TxEvent {
    /// Generic trace error.
    TraceError(String),
}

pub struct TxTracer {
    traces: Vec<TxEvent>,
}

impl TxTracer {
    pub fn new() -> Self {
        Self { traces: Vec::new() }
    }
}

impl trace::Tracer for TxTracer {
    type Output = TxEvent;

    fn prepare_trace_call(&self, _params: &parity_vm::ActionParams) -> Option<Call> {
        // println!("call: {:?} -> {:?} (data: {:?})", _params.sender, _params.address, _params.data.as_ref().map(|d| d.len()));
        None
    }

    fn prepare_trace_create(&self, _params: &parity_vm::ActionParams) -> Option<Create> {
        // println!("create: {:?} -> {:?} (data: {:?})", _params.sender, _params.address, _params.data.as_ref().map(|d| d.len()));
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

    fn subtracer(&self) -> Self
    where
        Self: Sized,
    {
        Self::new()
    }

    fn drain(self) -> Vec<Self::Output> {
        self.traces
    }
}

#[derive(Debug)]
pub struct TxVmTracer {
    depth: usize,
    pc: usize,
    instruction: u8,
    stack: Vec<U256>,
    state: Option<TxVmState>,
}

impl Default for TxVmTracer {
    fn default() -> Self {
        TxVmTracer {
            depth: 0,
            pc: 0,
            instruction: 0,
            stack: Vec::new(),
            state: None,
        }
    }
}

/// A non-success, terminal state of a transaction.
#[derive(Debug, PartialEq, Eq)]
pub enum TxVmState {
    /// The transaction was reverted, all funds except the ones used until we hit the REVERT are
    /// returned to the caller.
    ///
    /// This typically happens when a `require(...)` statement fires.
    Reverted,
}

impl TxVmTracer {
    fn stack(&self) -> String {
        let items = self.stack.iter().map(u256_as_str).collect::<Vec<_>>();
        return format!("[{}]", items.join(","));

        fn u256_as_str(v: &U256) -> String {
            if v.is_zero() {
                "0x0".into()
            } else {
                format!("{:x}", v)
            }
        }
    }

    fn depth(&self) -> String {
        let mut s = String::new();

        for _ in 0..self.depth {
            s.push(' ');
        }

        s
    }
}

impl trace::VMTracer for TxVmTracer {
    type Output = TxVmState;

    fn trace_next_instruction(&mut self, pc: usize, instruction: u8, _current_gas: U256) -> bool {
        self.pc = pc;
        self.instruction = instruction;
        true
    }

    // Parts borrowed from: https://github.com/paritytech/sol-rs/blob/master/solaris/src/trace.rs
    fn trace_executed(
        &mut self,
        gas_used: U256,
        stack_push: &[U256],
        _mem_diff: Option<(usize, &[u8])>,
        _store_diff: Option<(U256, U256)>,
    ) {
        let info = parity_evm::Instruction::from_u8(self.instruction)
            .expect("legal instruction")
            .info();

        if info.name == "REVERT" {
            self.state = Some(TxVmState::Reverted);
        }

        if true {
            return;
        }

        let len = self.stack.len();

        self.stack
            .truncate(if len > info.args { len - info.args } else { 0 });

        self.stack.extend_from_slice(stack_push);

        println!(
            "{}[{}] {}({:x}) stack_after: {}, gas_left: {}",
            self.depth(),
            self.pc,
            info.name,
            self.instruction,
            self.stack(),
            gas_used,
        );
    }

    fn prepare_subtrace(&self, _code: &[u8]) -> Self
    where
        Self: Sized,
    {
        let mut vm = TxVmTracer::default();
        vm.depth = self.depth + 1;
        vm
    }

    fn done_subtrace(&mut self, sub: Self) {
        if sub.state.is_some() {
            self.state = sub.state;
        }
    }

    fn drain(self) -> Option<Self::Output> {
        self.state
    }
}
