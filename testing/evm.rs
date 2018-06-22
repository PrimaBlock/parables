use error::{CallError, Error, NonceError};
use ethabi;
use ethcore::db;
use ethcore::engines;
use ethcore::executive;
use ethcore::log_entry::LogEntry;
use ethcore::receipt::TransactionOutcome;
use ethcore::spec;
use ethcore::state;
use ethcore::state_db;
use ethcore_transaction::{Action, SignedTransaction, Transaction};
use ethereum_types::{Address, H256, U256};
use kvdb::KeyValueDB;
use parity_vm;
use std::collections::{hash_map, HashMap};
use std::fmt;
use std::mem;
use std::sync::Arc;
use trace;
use {journaldb, kvdb, kvdb_memorydb};

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

#[derive(Clone)]
pub struct Evm {
    env_info: parity_vm::EnvInfo,
    state: state::State<state_db::StateDB>,
    engine: Arc<engines::EthEngine>,
    /// Logs collected by topic.
    logs: HashMap<ethabi::Hash, Vec<LogEntry>>,
}

impl fmt::Debug for Evm {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("Evm").finish()
    }
}

impl Evm {
    /// Create a new ethereum virtual machine abstraction.
    pub fn new(spec: &spec::Spec) -> Result<Self, Error> {
        let env_info = Self::env_info(Address::random());
        let engine = Arc::clone(&spec.engine);
        let state = Self::state_from_spec(spec)?;

        let evm = Evm {
            env_info,
            state,
            engine,
            logs: HashMap::new(),
        };

        Ok(evm)
    }

    /// Convert the spec into a state.
    /// Converted from parity:
    /// https://github.com/paritytech/parity/blob/98b7c07171cd320f32877dfa5aa528f585dc9a72/ethcore/src/client/evm_test_client.rs#L136
    fn state_from_spec(spec: &spec::Spec) -> Result<state::State<state_db::StateDB>, Error> {
        let factories = Default::default();

        let db = Arc::new(kvdb_memorydb::create(
            db::NUM_COLUMNS.expect("We use column-based DB; qed"),
        ));

        let journal_db =
            journaldb::new(db.clone(), journaldb::Algorithm::EarlyMerge, db::COL_STATE);

        let mut state_db = state_db::StateDB::new(journal_db, 5 * 1024 * 1024);

        state_db = spec.ensure_db_good(state_db, &factories)
            .map_err(|e| format!("bad database state: {}", e))?;

        let genesis = spec.genesis_header();

        // Write DB
        {
            let mut batch = kvdb::DBTransaction::new();

            state_db
                .journal_under(&mut batch, 0, &genesis.hash())
                .map_err(|e| format!("failed to execute transaction: {}", e))?;

            db.write(batch)
                .map_err(|e| format!("failed to set up database: {}", e))?;
        }

        let state = state::State::from_existing(
            state_db,
            *genesis.state_root(),
            spec.engine.account_start_nonce(0),
            factories,
        ).map_err(|e| format!("error setting up state: {}", e))?;

        Ok(state)
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
    pub fn deploy<F>(&mut self, f: F, call: Call) -> Result<Address, CallError>
    where
        F: ethabi::ContractFunction<Output = Address>,
    {
        self.deploy_code(f.encoded(), call)
    }

    /// Deploy the contract with the given code.
    pub fn deploy_code(&mut self, code: Vec<u8>, call: Call) -> Result<Address, CallError> {
        let result = self.action(Action::Create, code, call)?;
        self.add_logs(result.logs)?;

        let address = result
            .contract_address
            .ok_or_else(|| format!("no address for deployed contract"))?;

        Ok(address)
    }

    /// Perform a call against the given contract function.
    pub fn call<F>(&mut self, address: Address, f: F, call: Call) -> Result<F::Output, CallError>
    where
        F: ethabi::ContractFunction,
    {
        let result = self.action(Action::Call(address), f.encoded(), call)?;
        self.add_logs(result.logs)?;

        let output = f.output(result.output.to_vec())
            .map_err(|e| format!("VM output conversion failed: {}", e))?;

        Ok(output)
    }

    /// Perform a call against the given contracts' fallback function.
    pub fn fallback(&mut self, address: Address, call: Call) -> Result<(), CallError> {
        let result = self.action(Action::Call(address), Vec::new(), call)?;
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
    pub fn drain_logs<P>(&mut self, filter: Filter<P>) -> Result<Vec<P::Log>, Error>
    where
        P: ethabi::ParseLog,
    {
        self.drain_logs_with(filter, |_, log| log)
    }

    /// Drain logs matching the given filter that has been registered so far.
    ///
    /// Include who sent the logs in the result.
    pub fn drain_logs_with_sender<P>(
        &mut self,
        filter: Filter<P>,
    ) -> Result<Vec<(Address, P::Log)>, Error>
    where
        P: ethabi::ParseLog,
    {
        self.drain_logs_with(filter, |sender, log| (sender, log))
    }

    /// Drain logs matching the given filter that has been registered so far.
    fn drain_logs_with<P, M, O>(&mut self, filter: Filter<P>, map: M) -> Result<Vec<O>, Error>
    where
        P: ethabi::ParseLog,
        M: Fn(Address, P::Log) -> O,
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
                            .map_err(|e| format!("failed to pase log: {}", e))?;

                        out.push(map(sender, log));
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

    /// Execute the given action.
    fn action(
        &mut self,
        action: Action,
        data: Vec<u8>,
        call: Call,
    ) -> Result<Execution, CallError> {
        let nonce = self.state.nonce(&call.sender).map_err(|_| NonceError)?;

        let tx = Transaction {
            nonce,
            gas_price: call.gas_price,
            gas: call.gas,
            action: action,
            value: call.value,
            data: data,
        };

        let tx = tx.fake_sign(call.sender.into());
        self.run_transaction(tx)
    }

    /// Run the specified transaction.
    fn run_transaction(&mut self, tx: SignedTransaction) -> Result<Execution, CallError> {
        let initial_gas = tx.gas;

        // Verify transaction
        let is_ok = tx.verify_basic(
            true,
            None,
            self.env_info.number >= self.engine.params().eip86_transition,
        );

        if let Err(error) = is_ok {
            return Err(format!("VM Error: {}", error).into());
        }

        // Apply transaction
        let result = self.state.apply_with_tracing(
            &self.env_info,
            self.engine.machine(),
            &tx,
            trace::TxTracer::new(),
            trace::TxVmTracer::default(),
        );

        let scheme = self.engine
            .machine()
            .create_address_scheme(self.env_info.number);

        match result {
            Ok(result) => {
                self.state.commit().ok();

                let state_root = *self.state.root();
                let gas_left = initial_gas - result.receipt.gas_used;
                let outcome = result.receipt.outcome;
                let output = result.output;
                let trace = result.trace;
                let vm_trace = result.vm_trace;
                let logs = result.receipt.logs;

                let contract_address = if let Action::Create = tx.action {
                    Some(executive::contract_address(scheme, &tx.sender(), &tx.nonce, &tx.data).0)
                } else {
                    None
                };

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
            Err(error) => {
                return Err(format!("VM Error: {}", error).into());
            }
        }
    }

    /// Add logs, partitioned by topic.
    fn add_logs(&mut self, logs: Vec<LogEntry>) -> Result<(), CallError> {
        for log in logs {
            let topic = match log.topics.iter().next() {
                Some(first) => *first,
                None => return Err("expected at least one topic".into()),
            };

            self.logs.entry(topic).or_insert_with(Vec::new).push(log);
        }

        Ok(())
    }
}

#[derive(Debug)]
pub struct Filter<P> {
    parse_log: P,
    topic: ethabi::Hash,
    filter: ethabi::TopicFilter,
}

impl<P> Filter<P> {
    pub fn new(parse_log: P) -> Result<Self, Error>
    where
        P: ethabi::LogFilter,
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

impl<P> ethabi::LogFilter for Filter<P>
where
    P: ethabi::LogFilter,
{
    fn match_any(&self) -> ethabi::TopicFilter {
        self.parse_log.match_any()
    }
}

pub trait InternalTryFrom<T>: Sized {
    type Error;

    /// TryFrom until std::convert::TryFrom is stable.
    fn internal_try_from(value: T) -> Result<Self, Self::Error>;
}

/// Blanket conversion trait so that we don't have to create a Filter instance of anything
/// implementing LogFilter.
impl<P> InternalTryFrom<P> for Filter<P>
where
    P: ethabi::LogFilter,
{
    type Error = Error;

    fn internal_try_from(value: P) -> Result<Filter<P>, Error> {
        Filter::new(value)
    }
}

/// Extract the exact topic or fail.
pub fn extract_this_topic(topic: &ethabi::Topic<ethabi::Hash>) -> Result<ethabi::Hash, Error> {
    match *topic {
        ethabi::Topic::This(ref id) => Ok(*id),
        ref other => return Err(format!("not an exact topic: {:?}", other).into()),
    }
}
