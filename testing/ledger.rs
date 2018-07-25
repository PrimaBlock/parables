//! A ledger is used to keep track of the books for multiple accounts.
//!
//! For testing, this permits us to perform a kind of double booking.

use ethereum_types::{Address, U256};
use evm;
use failure::Error;
use std::collections::{hash_map, HashMap};

#[derive(Debug)]
pub struct Ledger<S>
where
    S: LedgerState,
{
    state: S,
    entries: HashMap<Address, S::Entry>,
}

impl<'a> Ledger<AccountBalance<'a>> {
    /// Construct a new empty ledger that doesn't have any specialized state.
    pub fn account_balance(evm: &'a evm::Evm) -> Ledger<AccountBalance<'a>> {
        Self::new(AccountBalance(evm))
    }
}

impl<S> Ledger<S>
where
    S: LedgerState,
{
    /// Construct a new ledger.
    ///
    /// To construct a ledger without state, use `Ledger::empty()`.
    pub fn new(state: S) -> Ledger<S> {
        Ledger {
            state,
            entries: HashMap::new(),
        }
    }

    /// Synchronize the ledger against the current state of the virtual machine.
    pub fn sync(&mut self, address: Address) -> Result<(), Error> {
        match self.entries.entry(address) {
            hash_map::Entry::Vacant(entry) => {
                let mut state = self.state.new_instance();
                self.state.sync(address, &mut state)?;
                entry.insert(state);
            }
            hash_map::Entry::Occupied(entry) => {
                self.state.sync(address, entry.into_mut())?;
            }
        }

        Ok(())
    }

    /// Sync multiple addresses.
    pub fn sync_all(&mut self, addresses: impl IntoIterator<Item = Address>) -> Result<(), Error> {
        for a in addresses {
            self.sync(a)?;
        }

        Ok(())
    }

    /// Go through each registered account, and verify their invariants.
    pub fn verify(self) -> Result<(), Error> {
        use std::fmt::Write;

        let mut errors = Vec::new();

        let state = self.state;

        // Check that all verifiable entries are matching expectations.
        for (address, s) in self.entries {
            if let Err(e) = state.verify(address, s) {
                errors.push((address, e));
            }
        }

        if !errors.is_empty() {
            let mut msg = String::new();

            writeln!(msg, "Errors in ledger:")?;

            for (address, e) in errors {
                writeln!(msg, "{}: {}", address, e)?;
            }

            bail!("{}", msg);
        }

        Ok(())
    }

    /// Access the mutable state for the given address.
    pub fn entry(&mut self, address: Address) -> &mut S::Entry {
        match self.entries.entry(address) {
            hash_map::Entry::Vacant(entry) => {
                let mut state = self.state.new_instance();
                entry.insert(state)
            }
            hash_map::Entry::Occupied(entry) => entry.into_mut(),
        }
    }
}

impl<S> Ledger<S>
where
    S: LedgerState<Entry = U256>,
{
    /// Add to the balance for the given address.
    pub fn add<V>(&mut self, address: Address, value: V)
    where
        V: Into<U256>,
    {
        let current = self.entries.entry(address).or_insert_with(U256::default);
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
        let current = self.entries.entry(address).or_insert_with(U256::default);
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
}

/// A state that can be verified with a virtual machine.
pub trait LedgerState {
    type Entry;

    /// Construct a new instance.
    fn new_instance(&self) -> Self::Entry;

    /// Verify the given state.
    fn verify(&self, address: Address, instance: Self::Entry) -> Result<(), Error>;

    /// Synchronize the given state.
    fn sync(&self, address: Address, instance: &mut Self::Entry) -> Result<(), Error>;
}

/// A ledger state checking account balances against the EVM.
pub struct AccountBalance<'a>(&'a evm::Evm);

impl<'a> LedgerState for AccountBalance<'a> {
    type Entry = U256;

    fn new_instance(&self) -> U256 {
        U256::default()
    }

    fn verify(&self, address: Address, expected_balance: Self::Entry) -> Result<(), Error> {
        let actual_balance = self.0.balance(address)?;

        if expected_balance != actual_balance {
            bail!(
                "expected account wei balance {}, but was {}",
                expected_balance,
                actual_balance
            );
        }

        Ok(())
    }

    fn sync(&self, address: Address, balance: &mut Self::Entry) -> Result<(), Error> {
        *balance = self.0.balance(address)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Ledger, LedgerState};
    use error;
    use ethereum_types::{Address, U256};

    #[test]
    fn simple_u256_ledger() {
        let mut ledger = Ledger::new(Simple(0.into(), 42.into()));

        let a = Address::random();

        ledger.sync(a).expect("bad sync");

        ledger.add(a, 10);
        ledger.add(a, 32);

        ledger.verify().expect("ledger not balanced");

        pub struct Simple(U256, U256);

        impl LedgerState for Simple {
            type Entry = U256;

            fn new_instance(&self) -> U256 {
                U256::default()
            }

            fn verify(
                &self,
                _address: Address,
                expected_balance: Self::Entry,
            ) -> Result<(), Error> {
                let actual_balance = self.1;

                if expected_balance != actual_balance {
                    return Err(format!(
                        "expected account wei balance {}, but was {}",
                        expected_balance, actual_balance
                    ).into());
                }

                Ok(())
            }

            fn sync(&self, _address: Address, balance: &mut Self::Entry) -> Result<(), Error> {
                *balance = self.0;
                Ok(())
            }
        }
    }
}
