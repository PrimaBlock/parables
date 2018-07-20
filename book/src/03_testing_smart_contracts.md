# Testing Smart Contracts

To test a smart contract, we run it on top of the Ethereum Virtual Machine (EVM) built by [parity].

Parables provides a wrapper for this using the `Evm` type.

But before we start testing our contracts, we need to make sure that they are _compiled_ into
bytecode and abi.

To do this, we can use solcjs.

```bash
(cd contracts && solcjs *.sol --bin --abi)
```

After this, you can put the following in `src/main.rs`.
Don't worry, we will walk you through line-by-line what it is below.

```rust
#[macro_use]
extern crate parables_testing;

use parables_testing::prelude::*;

contracts!();

fn main() -> Result<()> {
    // Set up a template call with a default amount of gas.
    let owner = Address::random();
    let call = Call::new(owner).gas(1_000_000);

    // Initialize a new linker.
    let mut linker = Linker::new();

    // Set up a new virtual machine with a default (null) foundation.
    let foundation = Spec::new_null();
    let mut evm = Evm::new(&foundation)?;

    // Deploy the SimpleContract.
    let code = simple_contract::bin(&linker)?;
    let simple = evm.deploy(simple_contract::constructor(code, 0), call)?.address;

    // Wrap the virtual machine in a Snapshot type so that it can be shared as a snapshot across
    // threads.
    let evm = Snapshot::new(evm);

    let mut tests = TestRunner::new();

    tests.test("get and increment value a couple of times", || {
        use simple_contract::events as ev;
        use simple_contract::functions as f;

        let mut evm = evm.get()?;

        let mut expected = U256::from(0);

        let out = evm.call(simple, f::get_value(), call)?.output;
        assert_eq!(expected, out);

        // change value
        expected = 1.into();

        evm.call(simple, f::set_value(expected), call)?;
        let out = evm.call(simple, f::get_value(), call)?.output;
        assert_eq!(expected, out);

        Ok(())
    });

    let reporter = StdoutReporter::new();
    tests.run(&reporter)?;

    Ok(())
}
```

We will now walk through this line-by-line and explain what it is.

```rust
use parables_testing::prelude::*;
```

This imports everything necessary to write parables test into the current scope.

Check out the [prelude documentation] as a reference for what is imported.

```rust
contracts!();
```

This makes use of ethabi's derive module to build a type-safe model for the contract that we can
use through the `simple_contract` module.

Through this we can import `functions`, `events`, and the contract's `constructor`.

```rust
let owner = Address::random();
let call = Call::new(owner).gas(1_000_000);
```

In main, we start by creating a random `owner`, and set up the template model we will be using for
our calls.

```rust
let mut linker = Linker::new();
```

Next we set up our linker. There is currently no configuration necessary since our contract doesn't
link to any other contracts. But we need one anyways.

```rust
let foundation = Spec::new_null();
let mut evm = Evm::new(&foundation)?;
```

Time to set up our _foundation_. A foundation determines the parameters of the blockchain.
The `null` foundation is the default foundation, which makes it operate like your modern Ethereum
blockchain.
But we also have access to older foundations like [`morden`].

The currently available foundations are:

* `Spec::new_null` - The most default foundation which doesn't have a consensus engine.
* `Spec::new_instant` - A default foundation which has an InstantSeal consensus engine.
* `Spec::new_test` - Morden without a consensus engine.

For more details, you'll currently have to reference the [Spec source code].

```rust
let code = simple_lib::bin(&linker)?;
let simple = evm.deploy(simple_contract::constructor(code, 0), call)?.address;
```

For the next line we link our contract, and deploy it to our virtual machine by calling its
constructor.

Note that the first argument of the constructor is the code to deploy.

```rust
let evm = Snapshot::new(evm);
```

Finally we want to wrap our virtual machine in the `Snapshot` container.
The virtual machine has some state that needs to be synchronized when shared across threads, but it
is clonable.
The Snapshot class provides us with a convenient `get()` function that handles the cloning for us.

Next we enter the code for the test case.

```rust
use simple_contract::events as ev;
use simple_contract::functions as f;
```

We start out by importing all relevant functions into scope for the test.

Using these we can access all events being emitted by the contract through `ev::SomeEvent`, and all
the functions being exposed as `f::some_function`.
Note that the function names are converted from lower camel (solidity standard) to lower snake case
(rust standard).

```rust
let mut evm = evm.get()?;
```

This line takes a snapshot of the virtual machine.
The snapshot is guaranteed to be isolated from all other snapshots, letting us run many tests in
isolation without worrying about trampling on each others feets.

```rust
let mut expected = 0;

let out = evm.call(simple, f::get_value(), call)?.output;
assert_eq!(expected, out);

// change value
expected = 1;

evm.call(simple, f::set_value(expected), call)?;
let out = evm.call(simple, f::get_value(), call)?.output;
assert_eq!(expected, out);
```

This final snippet is the complete test case.
We call the `getValue()` solidity function and compare its `output`, set it using `setValue(uint)`,
and make sure that it has been set as expected by getting it again.

So it's finally time to run your test!
You do this by calling `cargo run`.

```bash
cargo run
```

[prelude documentation]: ./doc/parables_testing/prelude/index.html
[parity]: https://github.com/paritytech/parity
[`mordem`]: https://blog.ethereum.org/2016/11/20/from-morden-to-ropsten/
[Spec source code]: https://github.com/paritytech/parity/blob/master/ethcore/src/spec/spec.rs
