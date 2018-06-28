# Account Balances

Every address has an implicit _account_ associated with it.
The account acts like a ledger, keeping track of of the balance in ether that any given address
has.

Accounts are always in use, any transaction takes into account the amount of ether being attached
to it and any gas being used.

To make use of balances, we first need to provide an address with a balance.

```rust
let foundation = Spec::new_null();
let mut evm = Evm::new(&foundation);

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
