# Introduction to Parables

Parables is a system for testing smart contracts.

It was built at [PrimaBlock] to support rapid and thorough testing of contracts.
We wanted to make use of [property testing](https://en.wikipedia.org/wiki/Property_testing), but
we found that conventional testing frameworks like [Truffle](https://truffleframework.com/) were
not fast enough to support rapid executions of a contract.

Smart contracts are _hard_ to get right, but our hopes is that Parables can help you on that
journey.

In this book we will walk you though how to write tests using Parables.
All the way from [setting up a new project], to performing full-scale [property testing].

So sit down, buckle your seat belt, and enjoy the trip!

[PrimaBlock]: https://github.com/primablock
[setting up a new project]: ./02_getting_started.html
[property testing]: ./04_property_testing.html
