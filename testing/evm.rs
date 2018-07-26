use ethabi;
use ethcore::db;
use ethcore::engines;
use ethcore::executive;
use ethcore::log_entry::LogEntry;
use ethcore::receipt;
use ethcore::spec;
use ethcore::state;
use ethcore::state_db;
use ethcore_transaction::{Action, SignedTransaction, Transaction};
use ethereum_types::{Address, U256};
use failure::Error;
use kvdb::KeyValueDB;
use parity_vm;
use std::cell::{Ref, RefCell, RefMut};
use std::collections::{hash_map, HashMap};
use std::fmt;
use std::mem;
use std::sync::{Arc, Mutex};
use trace;
use {abi, account, call, crypto, journaldb, kvdb, kvdb_memorydb, linker};

/// The outcome of a transaction.
///
/// Even when transactions fail, they might be using gas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome<T> {
    Ok(T),
    Reverted { error_info: trace::ErrorInfo },
    Errored { error_info: trace::ErrorInfo },
    Status { status: u8 },
}

impl<T> Outcome<T> {
    /// Check if the outcome is OK.
    pub fn is_ok(&self) -> bool {
        use self::Outcome::*;

        match *self {
            Ok(..) => true,
            _ => false,
        }
    }

    /// Check if outcome is errored.
    pub fn is_err(&self) -> bool {
        !self.is_ok()
    }

    /// Check if the outcome is reverted.
    pub fn is_reverted(&self) -> bool {
        use self::Outcome::*;

        match *self {
            Reverted { .. } => true,
            _ => false,
        }
    }
}

/// The result of executing a call transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct Call<T> {
    /// The outcome of a call.
    pub outcome: Outcome<T>,
    /// Gas used to perform call.
    pub gas_used: U256,
    /// The price payed for each gas.
    pub gas_price: U256,
    /// Value transmitted during the call.
    pub value: U256,
    /// The sender of the transaction.
    pub sender: Address,
}

impl<T> Call<T> {
    /// Total amount of wei transferred in the transaction.
    pub fn total(&self) -> U256 {
        match self.outcome {
            Outcome::Ok(..) => self.value + self.gas(),
            _ => self.gas(),
        }
    }

    /// Access the total amount of gas used in wei.
    pub fn gas(&self) -> U256 {
        self.gas_used * self.gas_price
    }

    /// Check if the outcome is OK.
    pub fn is_ok(&self) -> bool {
        self.outcome.is_ok()
    }

    /// Check if outcome is errored.
    pub fn is_err(&self) -> bool {
        self.outcome.is_err()
    }

    /// Check if the outcome is reverted.
    pub fn is_reverted(&self) -> bool {
        self.outcome.is_reverted()
    }

    /// Check that a revert happened with the specified statement.
    pub fn is_reverted_with(&self, stmt: impl AsRef<str> + Copy) -> bool {
        use self::Outcome::*;

        match self.outcome {
            Reverted { ref error_info } => {
                error_info.is_reverted() && error_info.is_failed_with(stmt)
            }
            _ => false,
        }
    }

    /// Convert the outcome into a result.
    pub fn ok(self) -> Result<T, Error> {
        use self::Outcome::*;

        match self.outcome {
            Ok(value) => Result::Ok(value),
            Reverted { error_info } => Err(format_err!("call reverted: {}", error_info)),
            Errored { error_info } => Err(format_err!("call errored: {}", error_info)),
            Status { status } => Err(format_err!("call returned status: {}", status)),
        }
    }
}

impl<T> fmt::Display for Call<T>
where
    T: fmt::Debug,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(self, fmt)
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
            .map_err(|e| format_err!("failed to setup account: {}", e))
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
            .map_err(|e| format_err!("bad database state: {}", e))?;

        let genesis = spec.genesis_header();

        // Write DB
        {
            let mut batch = kvdb::DBTransaction::new();

            state_db
                .journal_under(&mut batch, 0, &genesis.hash())
                .map_err(|e| format_err!("failed to execute transaction: {}", e))?;

            db.write(batch)
                .map_err(|e| format_err!("failed to set up database: {}", e))?;
        }

        let state = state::State::from_existing(
            state_db,
            *genesis.state_root(),
            spec.engine.account_start_nonce(0),
            factories,
        ).map_err(|e| format_err!("error setting up state: {}", e))?;

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
    pub fn deploy<C>(&self, constructor: C, call: call::Call) -> Result<Call<Address>, Error>
    where
        C: abi::ContractFunction<Output = Address> + abi::Constructor,
    {
        let mut linker = self.borrow_mut_linker()?;

        let code = constructor
            .encoded(&linker)
            .map_err(|e| format_err!("{}: failed to encode deployment: {}", C::ITEM, e))?;

        // when deploying, special source information should be used.
        let entry_source = match (C::BIN.clone(), C::SOURCE_MAP.clone()) {
            (bin, Some(source_map)) => {
                let source = linker
                    .source(bin, source_map)
                    .map_err(|e| format_err!("{}: {}", C::ITEM, e))?;

                Some(Arc::new(source))
            }
            _ => None,
        };

        let result = self.deploy_code(code, call, entry_source, &linker)?;

        // Register all linker information used for debugging.
        if let Outcome::Ok(ref address) = result.outcome {
            linker.register_item(C::ITEM.to_string(), *address);

            if let (Some(bin), Some(source_map)) =
                (C::RUNTIME_BIN.clone(), C::RUNTIME_SOURCE_MAP.clone())
            {
                let source = linker
                    .source(bin, source_map)
                    .map_err(|e| format_err!("{}: {}", C::ITEM, e))?;

                linker.register_runtime_source(C::ITEM.to_string(), source);
            }
        }

        Ok(result)
    }

    /// Deploy the contract with the given code.
    pub fn deploy_code(
        &self,
        code: Vec<u8>,
        call: call::Call,
        entry_source: Option<Arc<linker::Source>>,
        linker: &linker::Linker,
    ) -> Result<Call<Address>, Error> {
        self.action(
            Action::Create,
            code,
            call,
            entry_source,
            linker,
            |evm, tx, _| {
                let scheme = evm.engine
                    .machine()
                    .create_address_scheme(evm.env_info.number);

                let address =
                    executive::contract_address(scheme, &tx.sender(), &tx.nonce, &tx.data).0;
                Ok(address)
            },
        )
    }

    /// Perform a call against the given address' fallback function.
    ///
    /// This is the same as a straight up transfer.
    pub fn call_default(&self, address: Address, call: call::Call) -> Result<Call<()>, Error> {
        let linker = self.borrow_linker()?;

        self.action(
            Action::Call(address),
            Vec::new(),
            call,
            None,
            &linker,
            |_evm, _tx, _output| Ok(()),
        )
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
        Ok(state
            .balance(&address)
            .map_err(|_| format_err!("failed to access balance"))?)
    }

    /// Add the given number of wei to the provided account.
    pub fn add_balance<W: Into<U256>>(&self, address: Address, wei: W) -> Result<(), Error> {
        let mut state = self.borrow_mut_state()?;

        Ok(state
            .add_balance(&address, &wei.into(), state::CleanupMode::ForceCreate)
            .map_err(|_| format_err!("failed to modify balance"))?)
    }

    /// Execute the given action.
    fn action<T>(
        &self,
        action: Action,
        data: Vec<u8>,
        call: call::Call,
        entry_source: Option<Arc<linker::Source>>,
        linker: &linker::Linker,
        decode: impl FnOnce(&Evm, &SignedTransaction, Vec<u8>) -> Result<T, Error>,
    ) -> Result<Call<T>, Error> {
        let mut state = self.borrow_mut_state()?;

        let nonce = state
            .nonce(&call.sender)
            .map_err(|_| format_err!("error building nonce"))?;

        let tx = Transaction {
            nonce,
            gas_price: call.gas_price,
            gas: call.gas,
            action: action,
            value: call.value,
            data: data,
        };

        let tx = tx.fake_sign(call.sender.into());
        self.run_transaction(&mut state, tx, entry_source, linker, decode)
    }

    /// Run the specified transaction.
    fn run_transaction<T>(
        &self,
        state: &mut state::State<state_db::StateDB>,
        tx: SignedTransaction,
        entry_source: Option<Arc<linker::Source>>,
        linker: &linker::Linker,
        decode: impl FnOnce(&Evm, &SignedTransaction, Vec<u8>) -> Result<T, Error>,
    ) -> Result<Call<T>, Error> {
        // Verify transaction
        tx.verify_basic(
            true,
            None,
            self.env_info.number >= self.engine.params().eip86_transition,
        ).map_err(|e| format_err!("verify failed: {}", e))?;

        let frame_info = Mutex::new(trace::FrameInfo::None);

        // Apply transaction
        let result = state.apply_with_tracing(
            &self.env_info,
            self.engine.machine(),
            &tx,
            trace::TxTracer::new(linker, entry_source.clone(), &frame_info),
            trace::TxVmTracer::new(linker, entry_source.clone(), &frame_info),
        );

        let mut result = result.map_err(|e| format_err!("vm: {}", e))?;

        state.commit().ok();
        self.add_logs(result.receipt.logs.drain(..))?;

        let gas_used = result.receipt.gas_used;
        let gas_price = tx.gas_price;
        let value = tx.value;
        let sender = tx.sender();

        let outcome = self.outcome(result, tx, decode)?;

        Ok(Call {
            outcome,
            gas_used,
            gas_price,
            value,
            sender,
        })
    }

    /// Convert into an outcome.
    fn outcome<T>(
        &self,
        result: state::ApplyOutcome<trace::ErrorInfo, ()>,
        tx: SignedTransaction,
        decode: impl FnOnce(&Evm, &SignedTransaction, Vec<u8>) -> Result<T, Error>,
    ) -> Result<Outcome<T>, Error> {
        if !result.trace.is_empty() {
            let reverted = result.trace.iter().any(|e| e.is_reverted());

            if reverted {
                return Ok(Outcome::Reverted {
                    error_info: trace::ErrorInfo::new_root(result.trace),
                });
            } else {
                return Ok(Outcome::Errored {
                    error_info: trace::ErrorInfo::new_root(result.trace),
                });
            }
        }

        if let receipt::TransactionOutcome::StatusCode(status) = result.receipt.outcome {
            if status != 1 {
                return Ok(Outcome::Status { status });
            }
        }

        let output = decode(&self, &tx, result.output)?;
        Ok(Outcome::Ok(output))
    }

    /// Add logs, partitioned by topic.
    fn add_logs(&self, new_logs: impl Iterator<Item = LogEntry>) -> Result<(), Error> {
        let mut logs = self.borrow_mut_logs()?;

        for log in new_logs {
            let topic = match log.topics.iter().next() {
                Some(first) => *first,
                None => return Err(format_err!("expected at least one topic")),
            };

            logs.entry(topic).or_insert_with(Vec::new).push(log);
        }

        Ok(())
    }

    /// Access all raw logs.
    fn borrow_logs(&self) -> Result<Ref<HashMap<ethabi::Hash, Vec<LogEntry>>>, Error> {
        self.logs
            .try_borrow()
            .map_err(|e| format_err!("cannot borrow logs: {}", e))
    }

    /// Mutably access all raw logs.
    fn borrow_mut_logs(&self) -> Result<RefMut<HashMap<ethabi::Hash, Vec<LogEntry>>>, Error> {
        self.logs
            .try_borrow_mut()
            .map_err(|e| format_err!("cannot borrow logs mutably: {}", e))
    }

    /// Access linker.
    fn borrow_linker(&self) -> Result<Ref<linker::Linker>, Error> {
        self.linker
            .try_borrow()
            .map_err(|e| format_err!("cannot borrow linker: {}", e))
    }

    /// Mutably access linker.
    fn borrow_mut_linker(&self) -> Result<RefMut<linker::Linker>, Error> {
        self.linker
            .try_borrow_mut()
            .map_err(|e| format_err!("cannot borrow linker mutably: {}", e))
    }

    /// Access underlying state.
    fn borrow_state(&self) -> Result<Ref<state::State<state_db::StateDB>>, Error> {
        self.state
            .try_borrow()
            .map_err(|e| format_err!("cannot borrow state: {}", e))
    }

    /// Mutably access underlying state.
    fn borrow_mut_state(&self) -> Result<RefMut<state::State<state_db::StateDB>>, Error> {
        self.state
            .try_borrow_mut()
            .map_err(|e| format_err!("cannot borrow state mutably: {}", e))
    }
}

impl abi::Vm for Evm {
    fn call<F>(&self, address: Address, f: F, call: call::Call) -> Result<Call<F::Output>, Error>
    where
        F: abi::ContractFunction,
    {
        let linker = self.borrow_linker()?;

        let params = f.encoded(&linker)
            .map_err(|e| format_err!("failed to encode input: {}", e))?;

        self.action(
            Action::Call(address),
            params,
            call,
            None,
            &linker,
            move |_evm, _tx, output| {
                f.output(output)
                    .map_err(|e| format_err!("VM output conversion failed: {}", e))
            },
        )
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
                            .map_err(|e| format_err!("failed to parse log entry: {}", e))?;

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
        ref other => return Err(format_err!("not an exact topic: {:?}", other)),
    }
}
