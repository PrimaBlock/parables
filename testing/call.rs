use ethereum_types::{Address, U256};

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

    /// Access the sender of the call.
    pub fn get_sender(&self) -> Address {
        self.sender
    }

    /// Modify sender of call.
    pub fn sender<S: Into<Address>>(self, sender: S) -> Self {
        Self {
            sender: sender.into(),
            ..self
        }
    }

    /// Access the gas of the call.
    pub fn get_gas(&self) -> U256 {
        self.gas
    }

    /// Set the call to have the specified amount of gas.
    pub fn gas<E: Into<U256>>(self, gas: E) -> Self {
        Self {
            gas: gas.into(),
            ..self
        }
    }

    /// Access the gas price of the call.
    pub fn get_gas_price(&self) -> U256 {
        self.gas_price
    }

    /// Set the call to have the specified gas price.
    pub fn gas_price<E: Into<U256>>(self, gas_price: E) -> Self {
        Self {
            gas_price: gas_price.into(),
            ..self
        }
    }

    /// Access the value of the call.
    pub fn get_value(&self) -> U256 {
        self.value
    }

    /// Set the call to have the specified value.
    pub fn value<E: Into<U256>>(self, value: E) -> Self {
        Self {
            value: value.into(),
            ..self
        }
    }
}
