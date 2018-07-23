use error::{BalanceError, CallError, Error, NonceError};
use ethabi;
use ethcore::db;
use ethcore::engines;
use ethcore::executive;
use ethcore::log_entry::LogEntry;
use ethcore::receipt;
use ethcore::receipt::TransactionOutcome;
use ethcore::spec;
use ethcore::state;
use ethcore::state_db;
use ethcore_transaction::{Action, SignedTransaction, Transaction};
use ethereum_types::{Address, U256};
use kvdb::KeyValueDB;
use parity_vm;
use std::cell::{Ref, RefCell, RefMut};
use std::collections::{hash_map, HashMap};
use std::fmt;
use std::mem;
use std::sync::{Arc, Mutex};
use trace;
use {abi, account, call, crypto, journaldb, kvdb, kvdb_memorydb, linker};

/// The result of executing a call transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallResult {
    /// Gas used to perform call.
    pub gas_used: U256,
    /// The price payed for each gas.
    pub gas_price: U256,
    /// The sender of the transaction.
    pub sender: Address,
}

impl CallResult {
    /// Access the total amount of gas used.
    pub fn gas_total(&self) -> U256 {
        self.gas_used * self.gas_price
    }
}

impl fmt::Display for CallResult {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(self, fmt)
    }
}

/// The result of executing a create transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateResult {
    /// Address the code was created on.
    pub address: Address,
    /// Gas used to create contract.
    pub gas_used: U256,
    /// The price payed for each gas.
    pub gas_price: U256,
    /// The sender of the transaction.
    pub sender: Address,
}

impl CreateResult {
    /// Access the total amount of gas used.
    pub fn gas_total(&self) -> U256 {
        self.gas_used * self.gas_price
    }
}

impl fmt::Display for CreateResult {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(self, fmt)
    }
}

/// Decoded output and call result in one.
#[derive(Debug, Clone)]
pub struct CallOutput<T> {
    pub output: T,
    pub result: CallResult,
}

impl<T> CallOutput<T> {
    /// Access the total amount of gas used.
    pub fn gas_total(&self) -> U256 {
        self.result.gas_total()
    }
}

// Primary EVM abstraction.
//
// Most state is guarded by runtime checks (e.g. RefCell) to simplify how we can interact with the
// Evm.
#[derive(Clone)]
pub struct Evm {
    env_info: parity_vm::EnvInfo,
    state: RefCell<state::State<state_db::StateDB>>,
    engine: Arc<engines::EthEngine>,
    /// Logs collected by topic.
    logs: RefCell<HashMap<ethabi::Hash, Vec<LogEntry>>>,
    /// Linker used, if available it can be used to perform source-map lookups.
    linker: RefCell<linker::Linker>,
    /// Default crypto implementation.
    crypto: RefCell<crypto::Crypto>,
}

impl fmt::Debug for Evm {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("Evm").finish()
    }
}

impl Evm {
    /// Create a new ethereum virtual machine abstraction.
    pub fn new(spec: &spec::Spec, context: abi::ContractContext) -> Result<Self, Error> {
        let env_info = Self::env_info(Address::random());
        let engine = Arc::clone(&spec.engine);
        let state = Self::state_from_spec(spec)?;

        let mut linker = linker::Linker::new();

        if let Some(source_list) = context.source_list {
            linker.register_source_list(source_list);
        }

        let evm = Evm {
            env_info,
            state: RefCell::new(state),
            engine,
            logs: RefCell::new(HashMap::new()),
            linker: RefCell::new(linker),
            crypto: RefCell::new(crypto::Crypto::new()),
        };

        Ok(evm)
    }

    /// Create a new account.
    pub fn account(&self) -> Result<account::Account, Error> {
        account::Account::new(&self.crypto)
            .map_err(|e| format!("failed to setup account: {}", e).into())
    }

    /// Get the current block number.
    pub fn get_block_number(&self) -> u64 {
        self.env_info.number
    }

    /// Set the current block number.
    pub fn set_block_number(&mut self, number: u64) {
        self.env_info.number = number;
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
    pub fn deploy<C>(
        &self,
        constructor: C,
        call: call::Call,
    ) -> Result<CreateResult, CallError<CreateResult>>
    where
        C: abi::ContractFunction<Output = Address> + abi::Constructor,
    {
        let mut linker = self.borrow_mut_linker()?;

        let code = constructor
            .encoded(&linker)
            .map_err(|e| format!("{}: failed to encode deployment: {}", C::ITEM, e))?;

        // when deploying, special source information should be used.
        let entry_source = match (C::BIN.clone(), C::SOURCE_MAP.clone()) {
            (bin, Some(source_map)) => {
                let source = linker
                    .source(bin, source_map)
                    .map_err(|e| format!("{}: {}", C::ITEM, e))?;

                Some(Arc::new(source))
            }
            _ => None,
        };

        let result = self.deploy_code(code, call, entry_source, &linker);

        // Register all linker information used for debugging.
        match result.as_ref() {
            Ok(result) => {
                linker.register_item(C::ITEM.to_string(), result.address);

                if let (Some(bin), Some(source_map)) =
                    (C::RUNTIME_BIN.clone(), C::RUNTIME_SOURCE_MAP.clone())
                {
                    let source = linker
                        .source(bin, source_map)
                        .map_err(|e| format!("{}: {}", C::ITEM, e))?;

                    linker.register_runtime_source(C::ITEM.to_string(), source);
                }
            }
            // ignore
            Err(_) => {}
        }

        result
    }

    /// Deploy the contract with the given code.
    pub fn deploy_code(
        &self,
        code: Vec<u8>,
        call: call::Call,
        entry_source: Option<Arc<linker::Source>>,
        linker: &linker::Linker,
    ) -> Result<CreateResult, CallError<CreateResult>> {
        self.action(
            Action::Create,
            code,
            call,
            entry_source,
            linker,
            Self::create_result,
        ).map(|(_, result)| result)
    }

    /// Perform a call against the given address' fallback function.
    ///
    /// This is the same as a straight up transfer.
    pub fn call_default(
        &self,
        address: Address,
        call: call::Call,
    ) -> Result<CallResult, CallError<CallResult>> {
        let linker = self.borrow_linker()?;

        self.action(
            Action::Call(address),
            Vec::new(),
            call,
            None,
            &linker,
            Self::call_result,
        ).map(|(_, result)| result)
    }

    /// Setup a log drainer that drains the specified logs.
    pub fn logs<'a, P>(&'a self, log: P) -> LogDrainer<'a, P>
    where
        P: abi::ParseLog + abi::LogFilter,
    {
        LogDrainer::new(self, log)
    }

    /// Access raw underlying logs.
    ///
    /// Note: it is important that the Ref is released as soon as possible since this would
    /// otherwise cause borrowing issues for other operations.
    pub fn raw_logs(&self) -> Result<Ref<HashMap<ethabi::Hash, Vec<LogEntry>>>, Error> {
        self.borrow_logs()
    }

    /// Check if we still have unclaimed logs.
    pub fn has_logs(&self) -> Result<bool, Error> {
        let logs = self.borrow_logs()?;
        Ok(logs.values().any(|v| !v.is_empty()))
    }

    /// Query the balance of the given account.
    pub fn balance(&self, address: Address) -> Result<U256, Error> {
        let state = self.borrow_state()?;
        Ok(state.balance(&address).map_err(|_| BalanceError)?)
    }

    /// Add the given number of wei to the provided account.
    pub fn add_balance<W: Into<U256>>(&self, address: Address, wei: W) -> Result<(), Error> {
        let mut state = self.borrow_mut_state()?;

        Ok(state
            .add_balance(&address, &wei.into(), state::CleanupMode::ForceCreate)
            .map_err(|_| BalanceError)?)
    }

    /// Execute the given action.
    fn action<F, E>(
        &self,
        action: Action,
        data: Vec<u8>,
        call: call::Call,
        entry_source: Option<Arc<linker::Source>>,
        linker: &linker::Linker,
        map: F,
    ) -> Result<(Vec<u8>, E), CallError<E>>
    where
        F: FnOnce(&Evm, &SignedTransaction, &receipt::Receipt) -> E,
    {
        let mut state = self.borrow_mut_state()?;

        let nonce = state.nonce(&call.sender).map_err(|_| NonceError)?;

        let tx = Transaction {
            nonce,
            gas_price: call.gas_price,
            gas: call.gas,
            action: action,
            value: call.value,
            data: data,
        };

        let tx = tx.fake_sign(call.sender.into());
        self.run_transaction(&mut state, tx, entry_source, linker, map)
    }

    fn call_result(_: &Evm, tx: &SignedTransaction, receipt: &receipt::Receipt) -> CallResult {
        let gas_used = receipt.gas_used;
        let gas_price = tx.gas_price;

        CallResult {
            gas_used,
            gas_price,
            sender: tx.sender(),
        }
    }

    fn create_result(
        evm: &Evm,
        tx: &SignedTransaction,
        receipt: &receipt::Receipt,
    ) -> CreateResult {
        let scheme = evm.engine
            .machine()
            .create_address_scheme(evm.env_info.number);

        let address = executive::contract_address(scheme, &tx.sender(), &tx.nonce, &tx.data).0;
        let gas_used = receipt.gas_used;
        let gas_price = tx.gas_price;

        CreateResult {
            address,
            gas_used,
            gas_price,
            sender: tx.sender(),
        }
    }

    /// Run the specified transaction.
    fn run_transaction<F, E>(
        &self,
        state: &mut state::State<state_db::StateDB>,
        tx: SignedTransaction,
        entry_source: Option<Arc<linker::Source>>,
        linker: &linker::Linker,
        map: F,
    ) -> Result<(Vec<u8>, E), CallError<E>>
    where
        F: FnOnce(&Evm, &SignedTransaction, &receipt::Receipt) -> E,
    {
        // Verify transaction
        tx.verify_basic(
            true,
            None,
            self.env_info.number >= self.engine.params().eip86_transition,
        ).map_err(|e| format!("verify failed: {}", e))?;

        let frame_info = Mutex::new(trace::FrameInfo::None);

        // Apply transaction
        let result = state.apply_with_tracing(
            &self.env_info,
            self.engine.machine(),
            &tx,
            trace::TxTracer::new(linker, entry_source.clone(), &frame_info),
            trace::TxVmTracer::new(linker, entry_source.clone(), &frame_info),
        );

        let result = result.map_err(|e| format!("vm: {}", e))?;

        state.commit().ok();

        let execution = map(self, &tx, &result.receipt);

        self.add_logs(result.receipt.logs)?;

        if !result.trace.is_empty() {
            let reverted = result.trace.iter().any(|e| e.is_reverted());

            if reverted {
                return Err(CallError::Reverted {
                    execution,
                    error_info: trace::ErrorInfo::new_root(result.trace),
                });
            } else {
                return Err(CallError::Errored {
                    execution,
                    error_info: trace::ErrorInfo::new_root(result.trace),
                });
            }
        }

        match result.receipt.outcome {
            TransactionOutcome::Unknown | TransactionOutcome::StateRoot(_) => {
                // OK
            }
            TransactionOutcome::StatusCode(status) => {
                if status != 1 {
                    return Err(CallError::Status { execution, status });
                }
            }
        }

        Ok((result.output, execution))
    }

    /// Add logs, partitioned by topic.
    fn add_logs(&self, new_logs: Vec<LogEntry>) -> Result<(), Error> {
        let mut logs = self.borrow_mut_logs()?;

        for log in new_logs {
            let topic = match log.topics.iter().next() {
                Some(first) => *first,
                None => return Err("expected at least one topic".into()),
            };

            logs.entry(topic).or_insert_with(Vec::new).push(log);
        }

        Ok(())
    }

    /// Access all raw logs.
    fn borrow_logs(&self) -> Result<Ref<HashMap<ethabi::Hash, Vec<LogEntry>>>, Error> {
        self.logs
            .try_borrow()
            .map_err(|e| format!("cannot borrow logs: {}", e).into())
    }

    /// Mutably access all raw logs.
    fn borrow_mut_logs(&self) -> Result<RefMut<HashMap<ethabi::Hash, Vec<LogEntry>>>, Error> {
        self.logs
            .try_borrow_mut()
            .map_err(|e| format!("cannot borrow logs mutably: {}", e).into())
    }

    /// Access linker.
    fn borrow_linker(&self) -> Result<Ref<linker::Linker>, Error> {
        self.linker
            .try_borrow()
            .map_err(|e| format!("cannot borrow linker: {}", e).into())
    }

    /// Mutably access linker.
    fn borrow_mut_linker(&self) -> Result<RefMut<linker::Linker>, Error> {
        self.linker
            .try_borrow_mut()
            .map_err(|e| format!("cannot borrow linker mutably: {}", e).into())
    }

    /// Access underlying state.
    fn borrow_state(&self) -> Result<Ref<state::State<state_db::StateDB>>, Error> {
        self.state
            .try_borrow()
            .map_err(|e| format!("cannot borrow state: {}", e).into())
    }

    /// Mutably access underlying state.
    fn borrow_mut_state(&self) -> Result<RefMut<state::State<state_db::StateDB>>, Error> {
        self.state
            .try_borrow_mut()
            .map_err(|e| format!("cannot borrow state mutably: {}", e).into())
    }
}

impl abi::Vm for Evm {
    fn call<F>(
        &self,
        address: Address,
        f: F,
        call: call::Call,
    ) -> Result<CallOutput<F::Output>, CallError<CallResult>>
    where
        F: abi::ContractFunction,
    {
        let linker = self.borrow_linker()?;

        let params = f.encoded(&linker)
            .map_err(|e| format!("failed to encode input: {}", e))?;

        let (output, result) = self.action(
            Action::Call(address),
            params,
            call,
            None,
            &linker,
            Self::call_result,
        )?;

        let output = f.output(output)
            .map_err(|e| format!("VM output conversion failed: {}", e))?;

        Ok(CallOutput { output, result })
    }
}

#[derive(Debug)]
pub struct LogDrainer<'a, P> {
    evm: &'a Evm,
    log: P,
    filter: ethabi::TopicFilter,
}

impl<'a, P> LogDrainer<'a, P>
where
    P: abi::ParseLog + abi::LogFilter,
{
    pub fn new(evm: &'a Evm, log: P) -> Self {
        let filter = log.wildcard_filter();

        Self { evm, log, filter }
    }

    /// Modify the current drainer with a new filter.
    pub fn filter<M>(self, map: M) -> Self
    where
        M: FnOnce(&P) -> ethabi::TopicFilter,
    {
        Self {
            filter: map(&self.log),
            ..self
        }
    }

    /// Consumer the drainer and build an interator out of it.
    pub fn iter(self) -> Result<impl Iterator<Item = P::Log>, Error>
    where
        P: abi::ParseLog,
    {
        Ok(self.drain()?.into_iter())
    }

    /// Consumer the drainer without processing elements.
    pub fn drop(self) -> Result<(), Error> {
        let _ = self.drain()?;
        Ok(())
    }

    /// Drain logs matching the given filter that has been registered so far.
    pub fn drain(self) -> Result<Vec<P::Log>, Error>
    where
        P: abi::ParseLog,
    {
        self.drain_with(|_, log| log)
    }

    /// Drain logs matching the given filter that has been registered so far.
    ///
    /// Include who sent the logs in the result.
    pub fn drain_with_sender(self) -> Result<Vec<(Address, P::Log)>, Error> {
        self.drain_with(|sender, log| (sender, log))
    }

    /// Drain logs matching the given filter that has been registered so far.
    fn drain_with<M, O>(self, map: M) -> Result<Vec<O>, Error>
    where
        M: Fn(Address, P::Log) -> O,
    {
        let mut out = Vec::new();

        let LogDrainer { evm, log, filter } = self;

        let topic = extract_this_topic(&filter.topic0)?;

        let matches = move |log: &LogEntry| {
            let mut top = log.topics.iter();

            // topics to match in order.
            let mut mat = vec![
                &filter.topic0,
                &filter.topic1,
                &filter.topic2,
                &filter.topic3,
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
        };

        let mut logs = evm.borrow_mut_logs()?;

        match logs.entry(topic) {
            hash_map::Entry::Vacant(_) => return Ok(out),
            hash_map::Entry::Occupied(mut e) => {
                let remove = {
                    let mut keep = Vec::new();
                    let logs = e.get_mut();

                    for entry in logs.drain(..) {
                        if !matches(&entry) {
                            keep.push(entry);
                            continue;
                        }

                        let sender = entry.address;

                        let entry = log.parse_log((entry.topics, entry.data).into())
                            .map_err(|e| format!("failed to parse log entry: {}", e))?;

                        out.push(map(sender, entry));
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
}

/// Extract the exact topic or fail.
pub fn extract_this_topic(topic: &ethabi::Topic<ethabi::Hash>) -> Result<ethabi::Hash, Error> {
    match *topic {
        ethabi::Topic::This(ref id) => Ok(*id),
        ref other => return Err(format!("not an exact topic: {:?}", other).into()),
    }
}
