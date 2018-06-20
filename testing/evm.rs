use error::{CallError, Result};
use ethabi;
use ethcore::client::{EvmTestClient, TransactResult};
use ethcore::log_entry::LogEntry;
use ethcore::receipt::TransactionOutcome;
use ethcore_transaction::{Action, SignedTransaction, Transaction};
use ethereum_types::{Address, H256, U256};
use parity_vm;
use std::collections::{hash_map, HashMap};
use std::mem;
use std::result;
use std::sync::Arc;
use trace;

/// The result of executing a transaction.
#[derive(Debug, Clone)]
pub struct Execution {
    state_root: H256,
    gas_left: U256,
    output: Vec<u8>,
    contract_address: Option<Address>,
    logs: Vec<LogEntry>,
    outcome: TransactionOutcome,
}

#[derive(Debug, Clone, Copy)]
pub struct Call {
    /// The sender of the call.
    sender: Address,
    /// The amount of gas to include in the call.
    gas: U256,
    /// The price willing to pay for gas during the call (in WEI).
    gas_price: U256,
    /// The amount of ethereum attached to the call (in WEI).
    value: U256,
}

impl Call {
    /// Build a new call with the given sender.
    pub fn new(sender: Address) -> Self {
        Self {
            sender,
            gas: 0.into(),
            gas_price: 0.into(),
            value: 0.into(),
        }
    }

    /// Modify sender of call.
    pub fn sender<S: Into<Address>>(self, sender: S) -> Self {
        Self {
            sender: sender.into(),
            ..self
        }
    }

    /// Set the call to have the specified amount of gas.
    pub fn gas<E: Into<U256>>(self, gas: E) -> Self {
        Self {
            gas: gas.into(),
            ..self
        }
    }
}

#[derive(Debug)]
pub struct Evm<'a> {
    env_info: parity_vm::EnvInfo,
    client: EvmTestClient<'a>,
    /// Logs collected by topic.
    logs: HashMap<ethabi::Hash, Vec<LogEntry>>,
}

impl<'a> Evm<'a> {
    /// Create a new ethereum virtual machine abstraction.
    pub fn new(client: EvmTestClient<'a>) -> Self {
        let author = Self::random_address();
        let env_info = Self::env_info(author);

        Evm {
            env_info,
            client,
            logs: HashMap::new(),
        }
    }

    /// Create a random address.
    pub fn random_address() -> Address {
        Address::random()
    }

    /// Create a static info structure of the environment.
    pub fn env_info(author: Address) -> parity_vm::EnvInfo {
        parity_vm::EnvInfo {
            number: 10_000_000u64,
            author: author,
            timestamp: 1u64,
            difficulty: 1.into(),
            gas_limit: 10_000_000.into(),
            gas_used: 0.into(),
            last_hashes: Arc::new(vec![0.into(); 256]),
        }
    }

    /// Deploy the contract with the given code.
    pub fn deploy<F>(&mut self, f: F, call: Call) -> Result<Address>
    where
        F: ethabi::ContractFunction<Output = Address>,
    {
        self.deploy_code(f.encoded(), call)
    }

    /// Deploy the contract with the given code.
    pub fn deploy_code(&mut self, code: Vec<u8>, call: Call) -> Result<Address> {
        let nonce = self.client.state().nonce(&call.sender)?;

        let tx = Transaction {
            nonce,
            gas_price: call.gas_price,
            gas: call.gas,
            action: Action::Create,
            value: call.value,
            data: code,
        };

        let tx = tx.fake_sign(call.sender.into());

        let result = self.run_transaction(tx)?;
        self.add_logs(result.logs)?;

        let address = result
            .contract_address
            .ok_or_else(|| format_err!("no address for deployed contract"))?;

        Ok(address)
    }

    /// Perform a call against the given contract function.
    pub fn call<F>(
        &mut self,
        address: Address,
        f: F,
        call: Call,
    ) -> result::Result<F::Output, CallError>
    where
        F: ethabi::ContractFunction,
    {
        let tx = self.call_transaction(address, Some(f.encoded()), call)?;
        let result = self.run_transaction(tx)?;
        self.add_logs(result.logs)?;

        let output = f.output(result.output.to_vec())
            .map_err(|e| format!("VM output conversion failed: {}", e))?;
        Ok(output)
    }

    /// Perform a call against the given contracts' fallback function.
    pub fn fallback(&mut self, address: Address, call: Call) -> result::Result<(), CallError> {
        let tx = self.call_transaction(address, None, call)?;
        let result = self.run_transaction(tx)?;
        self.add_logs(result.logs)?;
        Ok(())
    }

    /// Access all logs.
    pub fn logs(&self) -> &HashMap<ethabi::Hash, Vec<LogEntry>> {
        &self.logs
    }

    /// Check if we still have unclaimed logs.
    pub fn has_logs(&self) -> bool {
        self.logs.values().any(|v| !v.is_empty())
    }

    /// Drain logs matching the given filter that has been registered so far.
    pub fn drain_logs<P>(&mut self, filter: &Filter<P>) -> Result<Vec<P::Log>>
    where
        P: ethabi::ParseLog,
    {
        self.drain_logs_with(filter, |_, log| log)
    }

    /// Drain logs matching the given filter that has been registered so far.
    ///
    /// Include who sent the logs in the result.
    pub fn drain_logs_with_sender<P>(&mut self, filter: &Filter<P>) -> Result<Vec<(Address, P::Log)>>
    where
        P: ethabi::ParseLog,
    {
        self.drain_logs_with(filter, |sender, log| (sender, log))
    }

    /// Drain logs matching the given filter that has been registered so far.
    fn drain_logs_with<P, F, O>(&mut self, filter: &Filter<P>, f: F) -> Result<Vec<O>>
    where
        P: ethabi::ParseLog,
        F: Fn(Address, P::Log) -> O
    {
        let mut out = Vec::new();

        match self.logs.entry(filter.topic) {
            hash_map::Entry::Vacant(_) => return Ok(out),
            hash_map::Entry::Occupied(mut e) => {
                let remove = {
                    let mut keep = Vec::new();
                    let logs = e.get_mut();

                    for log in logs.drain(..) {
                        if !filter.matches(&log) {
                            keep.push(log);
                            continue;
                        }

                        let sender = log.address;

                        let log = filter
                            .parse_log
                            .parse_log((log.topics, log.data).into())
                            .map_err(|e| format_err!("failed to pase log: {}", e))?;

                        out.push(f(sender, log));
                    }

                    if !keep.is_empty() {
                        mem::replace(logs, keep);
                        false
                    } else {
                        true
                    }
                };

                if remove {
                    e.remove_entry();
                }
            }
        }

        Ok(out)
    }

    /// Add logs, partitioned by topic.
    fn add_logs(&mut self, logs: Vec<LogEntry>) -> result::Result<(), CallError> {
        for log in logs {
            let topic = match log.topics.iter().next() {
                Some(first) => *first,
                None => return Err("expected at least one topic".into()),
            };

            self.logs.entry(topic).or_insert_with(Vec::new).push(log);
        }

        Ok(())
    }

    /// Set up a call transaction.
    fn call_transaction(
        &mut self,
        address: Address,
        data: Option<Vec<u8>>,
        call: Call,
    ) -> result::Result<SignedTransaction, CallError> {
        let nonce = self.client
            .state()
            .nonce(&call.sender)
            .map_err(|e| format!("failed to set up nonce: {}", e))?;

        let tx = Transaction {
            nonce,
            gas_price: call.gas_price,
            gas: call.gas,
            action: Action::Call(address),
            value: call.value,
            data: data.unwrap_or_else(Vec::new),
        };

        Ok(tx.fake_sign(call.sender))
    }

    /// Run the specified transaction.
    fn run_transaction(&mut self, tx: SignedTransaction) -> result::Result<Execution, CallError> {
        let res = self.client.transact(
            &self.env_info,
            tx,
            trace::TxTracer::new(),
            trace::TxVmTracer::default(),
        );

        match res {
            TransactResult::Ok {
                state_root,
                gas_left,
                output,
                contract_address,
                logs,
                outcome,
                trace,
                vm_trace,
                ..
            } => {
                match vm_trace {
                    Some(trace::TxVmState::Reverted) => {
                        return Err(CallError::Reverted);
                    }
                    _ => {}
                }

                if !trace.is_empty() {
                    return Err(format!("errors in call: {:?}", trace).into());
                }

                match outcome {
                    TransactionOutcome::Unknown | TransactionOutcome::StateRoot(_) => {
                        // OK
                    }
                    TransactionOutcome::StatusCode(status) => {
                        if status != 1 {
                            return Err(format!("call failed with status code: {}", status).into());
                        }
                    }
                }

                Ok(Execution {
                    state_root,
                    gas_left,
                    output,
                    contract_address,
                    logs,
                    outcome,
                })
            }
            TransactResult::Err { error, .. } => {
                return Err(format!("VM Error: {}", error).into());
            }
        }
    }
}

#[derive(Debug)]
pub struct Filter<P> {
    parse_log: P,
    topic: ethabi::Hash,
    filter: ethabi::TopicFilter,
}

impl<P> Filter<P> {
    pub fn new(parse_log: P) -> Result<Self>
    where
        P: ethabi::LogFilter
    {
        let filter = parse_log.match_any();
        let topic = extract_this_topic(&filter.topic0)?;

        Ok(Self {
            parse_log,
            topic,
            filter,
        })
    }

    /// Build a new filter, which has a custom filter enabled.
    pub fn with_filter<M>(self, map: M) -> Self
    where
        M: FnOnce(&P) -> ethabi::TopicFilter,
    {
        Self {
            filter: map(&self.parse_log),
            ..self
        }
    }

    pub fn matches(&self, log: &LogEntry) -> bool {
        let mut top = log.topics.iter();

        // topics to match in order.
        let mut mat = vec![
            &self.filter.topic0,
            &self.filter.topic1,
            &self.filter.topic2,
            &self.filter.topic3,
        ].into_iter();

        while let Some(t) = top.next() {
            let m = match mat.next() {
                Some(m) => m,
                None => return false,
            };

            match m {
                ethabi::Topic::Any => continue,
                ethabi::Topic::OneOf(ids) => {
                    if ids.contains(t) {
                        continue;
                    }
                }
                ethabi::Topic::This(id) => {
                    if id == t {
                        continue;
                    }
                }
            }

            return false;
        }

        // rest must match any
        mat.all(|m| *m == ethabi::Topic::Any)
    }
}

/// Extract the exact topic or fail.
pub fn extract_this_topic(topic: &ethabi::Topic<ethabi::Hash>) -> Result<ethabi::Hash> {
    match *topic {
        ethabi::Topic::This(ref id) => Ok(*id),
        ref other => bail!("not an exact topic: {:?}", other),
    }
}
