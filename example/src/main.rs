#[macro_use]
extern crate parables_testing;
extern crate pretty_env_logger;

use parables_testing::prelude::*;

contracts!{
    simple_contract => "SimpleContract.sol:SimpleContract",
    simple_lib => "SimpleLib.sol:SimpleLib",
    simple_ledger => "SimpleLedger.sol:SimpleLedger",
}

fn main() -> Result<()> {
    pretty_env_logger::init();

    let owner = Address::random();
    // template call
    let call = Call::new(owner).gas(1_000_000);

    let foundation = Spec::new_null();
    let evm = Evm::new(&foundation, new_context())?;
    evm.add_balance(owner, wei::from_ether(1000))?;

    // set up simple lib
    evm.deploy(simple_lib::constructor(), call)?.ok()?;
    let simple = evm.deploy(simple_contract::constructor(42), call)?.ok()?;

    let evm = Snapshot::new(evm);

    let mut runner = TestRunner::new();

    runner.test(
        "any set value",
        pt!{
            |(x in any::<u64>())| {
                use simple_contract::events as ev;

                let evm = evm.get()?;

                let contract = simple_contract::contract(&evm, simple, call);

                let out = contract.get_value()?.ok()?;
                assert_eq!(out, 42.into());

                contract.set_value(x)?.ok()?;

                let out = contract.get_value()?.ok()?;
                assert_eq!(out, x.into());

                for e in evm.logs(ev::value_updated()).filter(|e| e.filter(Some(100.into()))).iter()? {
                    assert_eq!(U256::from(100), e.value);
                }

                assert_eq!(1, evm.logs(ev::value_updated()).iter()?.count());
                assert!(!evm.has_logs()?, "there were unprocessed logs");
            }
        },
    );

    runner.test("simple contract", || {
        let evm = evm.get()?;
        let mut current = 42u64;

        let contract = simple_contract::contract(&evm, simple, call);

        let out = contract.get_value()?.ok()?;
        assert_eq!(out, current.into());

        contract.test_add(10, 20)?.ok()?;
        current = 30u64;

        for _ in 0..1 {
            let out = contract.get_value()?.ok()?;
            assert_eq!(out, current.into());

            // add a value to the call, this value will be sent to the contract.
            contract
                .value(wei::from_ether(1))
                .set_value(out + 1.into())?
                .ok()?;

            current += 1;
        }

        let not_owner = Address::random();

        // non-owner is not allowed to set value.
        let result = contract.sender(not_owner).set_value(0)?;
        assert!(
            result.is_reverted_with("SimpleContract:setValue", "require(msg.sender == owner);")
        );

        Ok(())
    });

    runner.test("decrement step by step", || {
        use simple_contract::events as ev;

        let evm = evm.get()?;
        let mut current = 42u64;

        let contract = simple_contract::contract(&evm, simple, call);

        let out = contract.get_value()?.ok()?;
        assert_eq!(out, current.into());

        contract.test_add(10, 20)?.ok()?;
        current = 30u64;

        for _ in 0..1000 {
            let out = contract.get_value()?.ok()?;
            assert_eq!(out, current.into());

            // add a value to the call, this value will be sent to the contract.
            contract
                .value(wei::from_ether(1))
                .set_value(out + 1.into())?
                .ok()?;

            current += 1;
        }

        let not_owner = Address::random();

        // non-owner is not allowed to set value.
        let non_owned_res = contract.sender(not_owner).set_value(0)?;
        assert!(non_owned_res.is_reverted());

        let balance = evm.balance(owner)?;
        assert_eq!(U256::from(0), balance);

        // all money should have flowed into the simple contract.
        let contract_balance = evm.balance(simple)?;
        assert_eq!(wei::from_ether(1000), contract_balance);

        evm.logs(ev::value_updated())
            .filter(|e| e.filter(Some(100.into())))
            .drop()?;

        assert_eq!(999, evm.logs(ev::value_updated()).iter()?.count());
        assert!(!evm.has_logs()?, "there were unprocessed logs");

        Ok(())
    });

    runner.test("test balance", || {
        let evm = evm.get()?;

        let a = Address::random();
        let b = Address::random();

        evm.add_balance(a, wei::from_ether(100))?;

        // send 10 ether from a to b.
        let r = evm.call_default(
            b,
            Call::new(a)
                .gas(21000)
                .gas_price(10)
                .value(wei::from_ether(10)),
        )?;

        // we also have to subtract gas * gas price
        assert_ne!(evm.balance(a)?, wei::from_ether(90));
        assert_eq!(evm.balance(a)?, wei::from_ether(90) - r.gas());
        assert_eq!(evm.balance(b)?, wei::from_ether(10));
        Ok(())
    });

    runner.test("test ledger state", || {
        let a = Address::random();
        let b = Address::random();

        let call = call.sender(a);

        let evm = evm.get()?;

        let simple = evm.deploy(simple_ledger::constructor(), call)?.ok()?;
        let simple = simple_ledger::contract(&evm, simple, call.gas_price(10));

        let mut balances = Ledger::account_balance(&evm);
        let mut states = Ledger::new(State(&evm, simple.address));

        evm.add_balance(a, wei!(100 eth))?;

        // sync all addresses to initial states.
        balances.sync_all(vec![a, b, simple.address])?;
        states.sync_all(vec![a, b, simple.address])?;

        // add to a
        let res = simple.value(wei!(42 eth)).add(a)?;
        balances.sub(a, res.gas() + wei!(42 eth))?;
        balances.add(simple.address, wei!(42 eth))?;
        states.add(a, wei!(42 eth))?;

        // add to b
        let res = simple.value(wei!(12 eth)).add(b)?;
        balances.sub(a, res.gas() + wei!(12 eth))?;
        balances.add(simple.address, wei!(12 eth))?;
        states.add(b, wei!(12 eth))?;

        balances.verify()?;
        states.verify()?;

        return Ok(());

        pub struct State<'a>(&'a Evm, Address);

        impl<'a> State<'a> {
            /// Helper to get the current value stored on the blockchain.
            fn get_value(&self, address: Address) -> Result<U256> {
                use simple_ledger::functions as f;
                let call = Call::new(Address::random()).gas(10_000_000).gas_price(0);
                Ok(self.0.call(self.1, f::get(address), call)?.ok()?)
            }
        }

        impl<'a> LedgerState for State<'a> {
            type Entry = U256;

            fn new_instance(&self) -> Self::Entry {
                U256::default()
            }

            fn sync(&self, address: Address, instance: &mut Self::Entry) -> Result<()> {
                *instance = self.get_value(address)?;
                Ok(())
            }

            fn verify(&self, address: Address, expected: &Self::Entry) -> Result<()> {
                let value = self.get_value(address)?;

                if value != *expected {
                    bail!("value: expected {} but got {}", expected, value);
                }

                Ok(())
            }
        }
    });

    let reporter = StdoutReporter::new()?;
    runner.run(&reporter)?;
    reporter.close()?;

    let (count, total) = evm.get()?.calculate_visited()?;
    println!("Contract Coverage: {}%", count * 100 / total);

    Ok(())
}
