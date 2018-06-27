# Getting Started

Parables runs as a regular application using a custom testing framework.
To run this you should set up a regular rust application. This can easiest be done using `cargo init`.

```bash
cargo init --bin my-contract-tests
```

Modify the `Cargo.toml` to add a dependency to parables:

```toml
[dependencies]
parables-testing = {git = "https://github.com/primablock/parables"}

[build-dependencies]
parables-build = {git = "https://github.com/primablock/parables"}
```

If you want parables to try to build your contracts automatically, add the following `build.rs`.

```rust
extern crate parables_build;

fn main() {
    if let Err(e) = parables_build::compile(concat!(env!("CARGO_MANIFEST_DIR"), "/contracts")) {
        panic!("failed to compile contracts: {:?}", e);
    }
}
```

Finally you should set up a main method that uses `TestRunner` to schedule tests in `src/main.rs`.

```rust
#[macro_use]
extern crate parables_testing;

use parables_testing::prelude::*;

fn main() -> Result<()> {
    let mut tests = TestRunner::new();

    tests.test("something cool", || {
        assert_eq!(1, 2);
    });

    let reporter = StdoutReporter::new();
    tests.run(&reporter)?;

    Ok(())
}
```

At this stage you can test that everything works with `cargo run`.

```bash
cargo run
```

```
something cool in 0s: failed at src/main.rs:9:9
assertion failed: `(left == right)`
  left: `1`,
 right: `2`
```

Now it's time to add a smart contract.

Create the contracts directory, and write the `SimpleContract` code below into
`contracts/SimpleContract.sol`.

```bash
mkdir contracts
```

```solidity
/// contracts/SimpleContract.sol

pragma solidity 0.4.24;

contract SimpleContract {
    uint value;
    address owner;

    event ValueUpdated(uint);

    constructor(uint initial) public {
        value = initial;
        owner = msg.sender;
    }

    modifier ownerOnly() {
        require(msg.sender == owner);
        _;
    }

    function getValue() public view returns(uint) {
        return value;
    }

    function setValue(uint update) public ownerOnly() {
        value = update;
        emit ValueUpdated(update);
    }
}
```

Compile the contract using `solcjs`.

```bash
(cd contracts && solcjs *.sol --bin --abi)
```

We then load it by adding the `contracts!` macro to the top of our file.

```rust
#[macro_use]
extern crate parables_testing;

use parables_testing::prelude::*;

contracts! {
    simple_contract {
        "contracts/SimpleContract_sol_SimpleContract.abi",
        "contracts/SimpleContract_sol_SimpleContract.bin"
    },
}

fn main() -> Result<()> {
    let mut tests = TestRunner::new();

    tests.test("something cool", || {
        assert_eq!(1, 2);
    });

    let reporter = StdoutReporter::new();
    tests.run(&reporter)?;

    Ok(())
}
```

In the next section we will walk you through how to write your first contract test.
