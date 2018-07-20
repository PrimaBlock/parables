# Accounts

Parables provides a helper to set up an account.

Accounts have an _address_, a _private_, and a _public_ key.

## Signing payloads

Through the `Account` structure we can sign payloads according to the [ECRecovery scheme].

```rust
let mut crypto = Crypto::new();
let account = Account::new(&mut crypto)?;

let mut sig = account.signer(&mut crypto);

// add things to the signature
sig.input(pool);
sig.input(scenario.owner);
sig.input(code);
sig.input(expiration);

let sig = sig.finish()?;
```

[ECRecovery scheme]: https://github.com/OpenZeppelin/openzeppelin-solidity/blob/master/contracts/ECRecovery.sol
