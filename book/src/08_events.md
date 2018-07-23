# Testing Events

Parables capatures all events emitted by your contract, and allows you to easily assert that
a specific set of events have been emitted.

A typical test would do something like the following.

```rust
let evm = evm.get();
let contract = simple_contract::contract(&evm, simple, call);

contract.set_value(100)?;
contract.set_value(200)?;

for e in evm.logs(ev::value_updated()).filter(|e| e.filter(Some(100.into()))).iter()? {
    assert_eq!(U256::from(100), e.value);
}

assert_eq!(1, evm.logs(ev::value_updated()).iter()?.count());
assert!(!evm.has_logs(), "there were unprocessed logs");
```

Note that converting the drainer into an iterator through the `iter()` method is a _fallible_
operation since it needs to decode all events.
