# Account Balances

Every address has an implicit _account_ associated with it.
The account acts like a ledger, keeping track of of the balance in ether that any given address
has.

Accounts are always in use, any transaction takes into account the amount of ether being attached
to it and any gas being used.

To make use of balances, we first need to provide an address with a balance.

```rust
let foundation = Spec::new_null();
let evm = Evm::new(&foundation, new_context());

let a = Address::random();
let b = Address::random();

evm.add_balance(a, wei::from_ether(100));
```

The first way we can change the balance of an account is to transfer ether from one account to
another using a default call.

```rust
let call = Call::new(a).gas(21000).gas_price(10);
let res = evm.call_default(b, call)?;
```

We can now check the balance for each account to make sure it's been modified.

Note that account `a` doesn't have `90` ether, we have to take the gas subtracted into account!

```rust
assert_ne!(evm.balance(a), wei::from_ether(90));
assert_eq!(evm.balance(a), wei::from_ether(90) - res.gas_total());
assert_eq!(evm.balance(b), wei::from_ether(10));
```

## Ledger

If you want a more streamlined way of testing that a various set of account balances are as you
expect, you can use a `Ledger`.

The above would then be written as:

```rust
let foundation = Spec::new_null();
let evm = Evm::new(&foundation, new_context());
let mut ledger = Ledger::empty(&evm);

let a = Address::random();
let b = Address::random();

// sync the initial state of the accounts we are interested in.
ledger.sync(a)?;
ledger.sync(b)?;

evm.add_balance(a, wei::from_ether(100));
ledger.add(a, wei::from_ether(100));

let call = Call::new(a).gas(21000).gas_price(10);
let res = evm.call_default(b, call)?;
// we expect the bas price to be deducted.
ledger.sub(a, res.gas_total());

// consume the ledger and verify all expected stated.
ledger.verify()?;
```

## Advanced bookkeeping with the Ledger

Suppose we have the following contract:

```solidity
pragma solidity 0.4.24;

contract SimpleLedger {
    mapping(address => uint) ledger;

    function add(address account) payable {
        ledger[account] += msg.value;
    }

    // used for testing
    function get(address account) returns(uint) {
        return ledger[account];
    }
}
```

The contract has a state which is stored per address.

A `Ledger` can be taught how to use this using a custom `LedgerState`.

```rust
use simple_ledger::simple_ledger;

let a = Address::random();
let b = Address::random();

let call = call.sender(a);

let evm = evm.get()?;

let simple = evm.deploy(simple_ledger::constructor(), call)?.address;
let simple = simple_ledger::contract(&evm, simple, call.gas_price(10));

let mut ledger = Ledger::new(&evm, State(simple.address));

evm.add_balance(a, wei!(100 eth))?;

ledger.sync(a)?;
ledger.sync(b)?;
ledger.sync(simple.address)?;

// add to a
let res = simple.value(wei!(42 eth)).add(a)?;
ledger.sub(a, res.gas_total() + wei!(42 eth));
ledger.add(simple.address, wei!(42 eth));
*ledger.state(a) = wei!(42 eth);

// add to b
let res = simple.value(wei!(12 eth)).add(b)?;
ledger.sub(a, res.gas_total() + wei!(12 eth));
ledger.add(simple.address, wei!(12 eth));
*ledger.state(b) = wei!(12 eth);

ledger.verify()?;

return Ok(());

pub struct State(Address);

impl State {
    /// Helper to get the current value stored on the blockchain.
    fn get_value(&self, evm: &Evm, address: Address) -> Result<U256> {
        use simple_ledger::simple_ledger::functions as f;
        let call = Call::new(Address::random()).gas(10_000_000).gas_price(0);
        Ok(evm.call(self.0, f::get(address), call)?.output)
    }
}

impl LedgerState for State {
    type Instance = U256;

    fn new_instance(&self) -> Self::Instance {
        U256::default()
    }

    fn sync(&self, evm: &Evm, address: Address, instance: &mut U256) -> Result<()> {
        *instance = self.get_value(evm, address)?;
        Ok(())
    }

    fn verify(&self, evm: &Evm, address: Address, expected: U256) -> Result<()> {
        let value = self.get_value(evm, address)?;

        if value != expected {
            return Err(format!("value: expected {} but got {}", expected, value).into());
        }

        Ok(())
    }
}
```
