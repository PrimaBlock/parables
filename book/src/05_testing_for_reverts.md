# Testing for Reverts

Now we will use the contract from the last section, but instead of simply failing, we will change
the assert to expect the revert to happen for some specific input.

Given that `getValue` looks like this (from the last section).

```solidity
function setValue(uint update) public ownerOnly() {
    require(value < 1000000);
    value = update;
}
```

We do that by changing the test case to this:

```rust
tests.test("get and increment value randomly within constraints", pt!{
    |(x in any::<u64>())| {
        let x = U256::from(x);

        let evm = evm.get()?;
        let contract = simple_contract::contract(&evm, simple, call);

        let out = contract.get_value()?.output;
        assert_eq!(U256::from(0), out);

        let result = contract.set_value(x);

        // expect that the transaction is reverted if we try to update the value to a value larger
        // or equal to 1 million.
        let expected = if x >= U256::from(1000000) {
            assert!(result.is_reverted());
            U256::from(0)
        } else {
            assert!(result.is_ok());
            x
        };

        let out = contract.get_value()?.output;
        assert_eq!(expected, out);
    }
});
```

Instead of an error, our test should now pass.

```
cargo run
```

```
get and increment value randomly within constraints in 0.686s: ok
```
