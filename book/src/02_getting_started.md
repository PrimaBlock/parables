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

If you want parables to rebuild your project when you change a contract, add the following
`build.rs`.

```rust
fn main() {
    println!("cargo:rerun-if-changed=contracts/SimpleContract.sol");
    println!("cargo:rerun-if-changed=contracts/SimpleLib.sol");
    println!("cargo:rerun-if-changed=contracts/SimpleLedger.sol");
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

We then load it by adding the `contracts!` macro to the top of our file.

```rust
#[macro_use]
extern crate parables_testing;

use parables_testing::prelude::*;

contracts! {
    simple_contract => "SimpleContract.sol:SimpleContract",
};

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
