# Introduction to Parables

Parables is a system for testing smart contracts.
Smart contracts are _hard_ to get right and the cost of making mistakes is high.
The way we avoid mistakes is by being exceptional at testing.

Parables was built at [PrimaBlock] to support thorough testing of contracts.
We wanted to make use of [property testing](https://en.wikipedia.org/wiki/Property_testing),
but found that conventional testing frameworks like [Truffle](https://truffleframework.com/) were
too slow to support that.

Property testing typically requires that the thing under test is executed hundreds of times with
different valued, randomized parameters.
For this reason, individual test cases must be _fast_.
Parables is able to execute complex contract interactions in microseconds since we do it directly
on top of the [parity virtual machine]. We also intend to make testing a first-class citizen of
parity by extending the necessary primitives to get it done the right way.

## About this book

This book is a user guide, suitable for people who want to learn how to do testing on top of
parables.

It requires an understanding of Rust, and that you have the `cargo` toolchain installed.
If you don't have it, you can get it through [rustup.rs].

We will guide you all the way from [setting up a new project], to performing full-scale
[property testing].

So sit down, buckle your seat belt, and enjoy the trip!

[PrimaBlock]: https://github.com/primablock
[rustup.rs]: https://rustup.rs
[setting up a new project]: ./02_getting_started.html
[property testing]: ./04_property_testing.html
[parity virtual machine]: https://github.com/paritytech/parity
