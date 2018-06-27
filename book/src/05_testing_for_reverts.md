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
tests.test("get and increment value randomly within constraints", || {
    proptest!(|(x in any::<u64>())| {
        use simple_contract::functions as f;

        let x = U256::from(x);

        let mut evm = evm.get()?;

        let out = evm.call(simple, f::get_value(), call)?.output;
        assert_eq!(U256::from(0), out);

        let result = evm.call(simple, f::set_value(x), call);

        // expect that the transaction is reverted if we try to update the value to a value larger
        // or equal to 1 million.
        let expected = if x >= U256::from(1000000) {
            assert!(result.is_reverted());
            U256::from(0)
        } else {
            assert!(result.is_ok());
            x
        };

        let out = evm.call(simple, f::get_value(), call)?.output;
        assert_eq!(expected, out);
    });
});
```

Instead of an error, our test should now pass.

```
cargo run
```

```
get and increment value randomly within constraints in 0.686s: ok
```
