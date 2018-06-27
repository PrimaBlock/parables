# Property Testing

Parables provides the necessary _speed_ to perform property testing of smart contracts.

We make use of the excellent [`proptest`] framework to accomplish this.

Let's rewrite our example from the last chapter to instead of testing that we can get and set some
well-defined numeric values, we test a wide range of values.

```rust
tests.test("get and increment value randomly", || {
    proptest!(|(x in any::<u64>())| {
        use simple_contract::functions as f;

        let x = U256::from(x);

        let mut evm = evm.get()?;

        let out = evm.call(simple, f::get_value(), call)?;
        assert_eq!(U256::from(0), out);

        evm.call(simple, f::set_value(x), call)?;
        let out = evm.call(simple, f::get_value(), call)?;
        assert_eq!(x, out);
    });
});
```

For the heck of it, let's introduce a require that will prevent us from setting the field to
a value larger or equal to `1000000`.

```solidity
function setValue(uint update) public ownerOnly() {
    require(value < 1000000);
    value = update;
    emit ValueUpdated(value);
}
```

What does our test case say?

```bash
cargo run
```

```
get and increment value randomly in 0.144s: failed at src/main.rs:36:9
Test failed: call was reverted; minimal failing input: x = 1000000
        successes: 0
        local rejects: 0
        global rejects: 0
```

What's happening here is actually quite remarkable.
When proptest notices a failing, random, input, it tries to _reduce_ the value to minimal failing
test.
The exact strategy is determined by the type being mutated, but for numeric values it performs
a binary search through all the inputs.

For more information on property testing, please read the [proptest README].

In the next section we will discuss how to _expect_ that a transaction is reverted.

[`proptest`]: https://github.com/AltSysrq/proptest
[proptest README]: https://github.com/AltSysrq/proptest
