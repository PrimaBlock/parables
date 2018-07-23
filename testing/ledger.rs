//! A ledger is used to keep track of the books for multiple accounts.
//!
//! For testing, this permits us to perform a kind of double booking.

use ethereum_types::{Address, U256};
use std::collections::{hash_map, HashMap};
use {error, evm};

#[derive(Debug)]
pub struct Ledger<'a, S>
where
    S: LedgerState,
{
    evm: &'a evm::Evm,
    state: S,
    instances: HashMap<Address, S::Instance>,
    balances: HashMap<Address, U256>,
}

impl<'a> Ledger<'a, ()> {
    /// Construct a new empty ledger that doesn't have any specialized state.
    pub fn empty(evm: &'a evm::Evm) -> Ledger<'a, ()> {
        Self::new(evm, ())
    }
}

impl<'a, S> Ledger<'a, S>
where
    S: LedgerState,
{
    /// Construct a new ledger.
    ///
    /// To construct a ledger without state, use `Ledger::empty()`.
    pub fn new(evm: &'a evm::Evm, state: S) -> Ledger<S> {
        Ledger {
            evm,
            state,
            instances: HashMap::new(),
            balances: HashMap::new(),
        }
    }

    /// Synchronize the ledger against the current state of the virtual machine.
    pub fn sync(&mut self, address: Address) -> Result<(), error::Error> {
        let balance = self.balances.entry(address).or_insert_with(U256::default);
        *balance = self.evm.balance(address)?;

        match self.instances.entry(address) {
            hash_map::Entry::Vacant(entry) => {
                let mut state = self.state.new_instance();
                self.state.sync(self.evm, address, &mut state)?;
                entry.insert(state);
            }
            hash_map::Entry::Occupied(entry) => {
                self.state.sync(self.evm, address, entry.into_mut())?;
            }
        }

        Ok(())
    }

    /// Go through each registered account, and verify their invariants.
    pub fn verify(self) -> Result<(), error::Error> {
        use std::fmt::Write;

        let mut errors = Vec::new();

        let state = self.state;

        for (address, expected_balance) in self.balances {
            let actual_balance = self.evm.balance(address)?;

            if expected_balance != actual_balance {
                errors.push((
                    address,
                    error::Error::from(format!(
                        "expected account wei balance {}, but was {}",
                        expected_balance, actual_balance
                    )),
                ));
            }
        }

        // Check that all verifiable states and balances are matching expectations.
        for (address, s) in self.instances {
            if let Err(e) = state.verify(self.evm, address, s) {
                errors.push((address, e));
            }
        }

        if !errors.is_empty() {
            let mut msg = String::new();

            writeln!(msg, "Errors in ledger:")?;

            for (address, e) in errors {
                writeln!(msg, "{}: {}", address, e)?;
            }

            return Err(msg.into());
        }

        Ok(())
    }

    /// Add to the balance for the given address.
    pub fn add<V>(&mut self, address: Address, value: V)
    where
        V: Into<U256>,
    {
        let current = self.balances.entry(address).or_insert_with(U256::default);
        let value = value.into();

        match current.checked_add(value) {
            None => {
                panic!(
                    "{}: adding {} to the account would overflow the balance",
                    address, value
                );
            }
            Some(update) => {
                *current = update;
            }
        }
    }

    /// Subtract from the balance for the given address.
    pub fn sub<V>(&mut self, address: Address, value: V)
    where
        V: Into<U256>,
    {
        let current = self.balances.entry(address).or_insert_with(U256::default);
        let value = value.into();

        match current.checked_sub(value) {
            None => {
                panic!(
                    "{}: subtracting {} would set account to negative balance",
                    address, value
                );
            }
            Some(update) => {
                *current = update;
            }
        }
    }

    /// Access the mutable state for the given address.
    pub fn state(&mut self, address: Address) -> &mut S::Instance {
        match self.instances.entry(address) {
            hash_map::Entry::Vacant(entry) => {
                let mut state = self.state.new_instance();
                entry.insert(state)
            }
            hash_map::Entry::Occupied(entry) => entry.into_mut(),
        }
    }
}

/// A state that can be verified with a virtual machine.
pub trait LedgerState {
    type Instance;

    /// Construct a new instance.
    fn new_instance(&self) -> Self::Instance;

    /// Verify the given state.
    fn verify(
        &self,
        evm: &evm::Evm,
        address: Address,
        instance: Self::Instance,
    ) -> Result<(), error::Error>;

    /// Synchronize the given state.
    fn sync(
        &self,
        evm: &evm::Evm,
        address: Address,
        instance: &mut Self::Instance,
    ) -> Result<(), error::Error>;
}

impl LedgerState for () {
    type Instance = ();

    fn new_instance(&self) -> () {
        ()
    }

    fn verify(
        &self,
        _evm: &evm::Evm,
        _address: Address,
        _instance: Self::Instance,
    ) -> Result<(), error::Error> {
        Ok(())
    }

    fn sync(
        &self,
        _evm: &evm::Evm,
        _address: Address,
        _instance: &mut Self::Instance,
    ) -> Result<(), error::Error> {
        Ok(())
    }
}
