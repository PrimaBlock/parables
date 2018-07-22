#[macro_use]
extern crate parables_testing;

use parables_testing::prelude::*;

contracts!();

fn main() -> Result<()> {
    use simple_contract::simple_contract;
    use simple_lib::simple_lib;

    let owner = Address::random();
    // template call
    let call = Call::new(owner).gas(1_000_000);

    let foundation = Spec::new_null();
    let evm = Evm::new(&foundation, new_context())?;
    evm.add_balance(owner, wei::from_ether(1000))?;

    // set up simple lib
    evm.deploy(simple_lib::constructor(), call)?;
    let simple = evm.deploy(simple_contract::constructor(42), call)?.address;

    let evm = Snapshot::new(evm);

    let mut runner = TestRunner::new();

    runner.test(
        "any set value",
        pt!{
            |(x in any::<u64>())| {
                use simple_contract::simple_contract;
                use simple_contract::simple_contract::events as ev;

                let evm = evm.get()?;

                let contract = simple_contract::contract(&evm, simple, call);

                let out = contract.get_value()?.output;
                assert_eq!(out, 42.into());

                contract.set_value(x)?;

                let out = contract.get_value()?.output;
                assert_eq!(out, x.into());

                for e in evm.logs(ev::value_updated()).filter(|e| e.filter(Some(100.into()))).iter()? {
                    assert_eq!(U256::from(100), e.value);
                }

                assert_eq!(1, evm.logs(ev::value_updated()).iter()?.count());
                assert!(!evm.has_logs()?, "there were unprocessed logs");
            }
        },
    );

    runner.test("decrement step by step", || {
        use simple_contract::simple_contract;
        use simple_contract::simple_contract::events as ev;

        let evm = evm.get()?;
        let mut current = 42u64;

        let contract = simple_contract::contract(&evm, simple, call);

        let out = contract.get_value()?.output;
        assert_eq!(out, current.into());

        contract.test_add(10, 20)?;
        current = 30u64;

        for _ in 0..1000 {
            let out = contract.get_value()?.output;
            assert_eq!(out, current.into());

            // add a value to the call, this value will be sent to the contract.
            contract
                .with_value(wei::from_ether(1))
                .set_value(out + 1.into())?;

            current += 1;
        }

        let not_owner = Address::random();

        // non-owner is not allowed to set value.
        let non_owned_res = contract.with_sender(not_owner).set_value(0);
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
        assert_eq!(evm.balance(a)?, wei::from_ether(90) - r.gas_total());
        assert_eq!(evm.balance(b)?, wei::from_ether(10));
        Ok(())
    });

    let reporter = StdoutReporter::new();
    runner.run(&reporter)?;
    reporter.close()?;

    Ok(())
}
