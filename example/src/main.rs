#[macro_use]
extern crate parables_testing;

use parables_testing::prelude::*;

contracts! {
    simple_contract {
        "contracts/SimpleContract_sol_SimpleContract.abi",
        "contracts/SimpleContract_sol_SimpleContract.bin"
    },
    simple_lib {
        "contracts/SimpleLib_sol_SimpleLib.abi",
        "contracts/SimpleLib_sol_SimpleLib.bin"
    },
}

fn main() -> Result<()> {
    let owner = Address::random();
    // template call
    let call = Call::new(owner).gas(1_000_000);

    let mut linker = Linker::new();

    let foundation = Spec::new_null();
    let mut evm = Evm::new(&foundation)?;
    evm.add_balance(owner, wei::from_ether(1000))?;

    // set up simple lib
    let code = simple_lib::bin(&linker)?;
    let simple_lib_address = evm.deploy(simple_lib::constructor(code), call)?.address;
    linker.register_item("SimpleLib".to_string(), simple_lib_address);

    let simple_contract_code = simple_contract::bin(&linker)?;
    let simple = evm.deploy(simple_contract::constructor(simple_contract_code, 42), call)?
        .address;

    let evm = Snapshot::new(evm);

    let mut runner = TestRunner::new();

    runner.test("any set value", pt!{
        |(x in any::<u64>())| {
            use simple_contract::events as ev;
            use simple_contract::functions as f;

            let mut evm = evm.get()?;

            let out = evm.call(simple, f::get_value(), call)?.output;
            assert_eq!(out, 42.into());

            evm.call(simple, f::set_value(x), call)?;

            let out = evm.call(simple, f::get_value(), call)?.output;
            assert_eq!(out, x.into());

            let filter = Filter::new(ev::value_updated())?
                .with_filter(|e| e.create_filter(Some(100.into())));

            // check logs.
            for _log in evm.drain_logs(filter) {
                // println!("log: {:?}", log);
            }
        }
    });

    runner.test("decrement step by step", || {
        use simple_contract::events as ev;
        use simple_contract::functions as f;

        let mut evm = evm.get()?;
        let mut current = 42u64;

        let out = evm.call(simple, f::get_value(), call)?.output;
        assert_eq!(out, current.into());

        evm.call(simple, f::test_add(10, 20), call)?;
        current = 30u64;

        for _ in 0..1000 {
            let out = evm.call(simple, f::get_value(), call)?.output;
            assert_eq!(out, current.into());
            evm.call(
                simple,
                f::set_value(out + 1.into()),
                call.value(wei::from_ether(1)),
            )?;
            current += 1;
        }

        let not_owner = Address::random();

        // non-owner is not allowed to set value.
        let non_owned_res = evm.call(simple, f::set_value(0), call.sender(not_owner));
        assert!(non_owned_res.is_reverted());

        let filter =
            Filter::new(ev::value_updated())?.with_filter(|e| e.create_filter(Some(100.into())));

        // check logs.
        for _log in evm.drain_logs(filter) {
            // println!("log: {:?}", log);
        }

        let balance = evm.balance(owner)?;
        assert_eq!(U256::from(0), balance);

        // all money should have flowed into the simple contract.
        let contract_balance = evm.balance(simple)?;
        assert_eq!(wei::from_ether(1000), contract_balance);

        Ok(())
    });

    runner.test("test balance", || {
        let mut evm = evm.get()?;

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
