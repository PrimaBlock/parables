# parables

`parables` is a framework to perform advanced and ergonomic solidity contract testing in Rust.

Note: this project is in _active development_. We track the master version of parity, where things
are still changing rapidly.
We won't release stable versions until parity has settled a bit.

## Getting started

See our [user guide](https://primablock.github.io/parables/) on how to get started with Parables.

You can also use our [example project](example) as a reference.

# Highlighted Features

This section contains highlights of the features available with Parables.

For a complete set of features, see the [user guide](https://primablock.github.io/parables/).

## State Dumping

When a contract throws an exception it can be hard to troubleshoot.

Parables integrates with Parity to give the context for all state used in the expression that
caused the exception to happen.

![Expression Dumping](images/expression-dump.gif)

## Parallel Test Runner

Using the provided `TestRunner`, each test case will run in parallel, sharing any state that is
possible between them to run as fast as possible.

![Parallel Tests](images/paralell-tests.gif)
