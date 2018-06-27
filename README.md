# parables

`parables` is a framework to perform advanced and ergonomic solidity contract testing in Rust.

Note: this project is in _active development_. We track the master version of parity, where things
are still changing rapidly. We will release new versions once parity has settled a bit.

## getting started

See our [user guide](https://primablock.github.io/parables/).

You can also use our [exampe project](example) as a reference.

## comparison to sol-rs

This is a reimplementation of [`sol-rs`](https://github.com/paritytech/sol-rs), with the
intent to improve ergonomics and simplify how tests are written.

The `sol-rs` project and their devs deserve all the credit for showing that this kind of testing on
top of parity is possible.

For now we need to freedom to move forward in a different direction to support our needs, but
hopefully we will be able to join forces in the future.

`parables` does the following things which are not supported in `sol-rs`:

* `sol-rs` [doesn't have a license yet](https://github.com/paritytech/sol-rs/issues/35).
* we include a [linker](testing/linker.rs), so that we can link libraries for testing.
* dependencies have been bumped and [patched to work with our contracts](https://github.com/paritytech/ethabi/compare/master...PrimaBlock:next?expand=1).
* interactions have been made more ergonomic, specifically around how log testing works.
  Additional work around this will probably be done in the future by contributing them to [ethabi-derive](https://github.com/paritytech/ethabi/tree/master/derive) to make it easier to work with.
* we have an ergonomic way to call the [fallback function](https://github.com/PrimaBlock/parables/blob/master/testing/evm.rs#L158) of a contract.
* dependencies are [re-exported through `parables-testing`](testing/prelude.rs), so that testing projects are easier to set up.
