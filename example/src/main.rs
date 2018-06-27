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

    // set up simple lib
    let code = simple_lib::bin(&linker)?;
    let simple_lib_address = evm.deploy(simple_lib::constructor(code), call)?;
    linker.register_item("SimpleLib".to_string(), simple_lib_address);

    let evm = Snapshot::new(evm);

    let mut runner = TestRunner::new();

    runner.test("property #1", || {
        proptest!(|(x in 0u32..32)| {
            println!("x: {}", x);
            let mut evm = evm.get()?;

            let mut current = 42u64;

            let simple_contract_code = simple_contract::bin(&linker)?;
            let simple = evm.deploy(
                simple_contract::constructor(simple_contract_code, current),
                call,
            )?;

            {
                use simple_contract::events as ev;
                use simple_contract::functions as f;

                let out = evm.call(simple, f::get_value(), call)?;
                assert_eq!(out, current.into());

                evm.call(simple, f::test_add(10, 20), call)?;
                current = 30u64;

                for _ in 0..1000 {
                    let out = evm.call(simple, f::get_value(), call)?;
                    assert_eq!(out, current.into());
                    evm.call(simple, f::set_value(out + 1.into()), call)?;
                    current += 1;
                }

                let not_owner = Address::random();

                // non-owner is not allowed to set value.
                let non_owned_res = evm.call(simple, f::set_value(0), call.sender(not_owner));
                assert!(non_owned_res.is_reverted());

                let filter = Filter::new(ev::value_updated())?
                    .with_filter(|e| e.create_filter(Some(100.into())));

                // check logs.
                for _log in evm.drain_logs(filter) {
                    // println!("log: {:?}", log);
                }
            }
        });
    });

    for i in 0..4 {
        runner.test(format!("test #{}", i), || {
            let mut evm = evm.get()?;

            let mut current = 42u64;

            let simple_contract_code = simple_contract::bin(&linker)?;
            let simple = evm.deploy(
                simple_contract::constructor(simple_contract_code, current),
                call,
            )?;

            {
                use simple_contract::events as ev;
                use simple_contract::functions as f;

                let out = evm.call(simple, f::get_value(), call)?;
                assert_eq!(out, current.into());

                evm.call(simple, f::test_add(10, 20), call)?;
                current = 30u64;

                for _ in 0..1000 {
                    let out = evm.call(simple, f::get_value(), call)?;
                    assert_eq!(out, current.into());
                    evm.call(simple, f::set_value(out + 1.into()), call)?;
                    current += 1;
                }

                let not_owner = Address::random();

                // non-owner is not allowed to set value.
                let non_owned_res = evm.call(simple, f::set_value(0), call.sender(not_owner));
                assert!(non_owned_res.is_reverted());

                let filter = Filter::new(ev::value_updated())?
                    .with_filter(|e| e.create_filter(Some(100.into())));

                // check logs.
                for _log in evm.drain_logs(filter) {
                    // println!("log: {:?}", log);
                }
            }

            Ok(())
        });
    }

    runner.test("test_something", || {
        assert_eq!(true, false);
    });

    let reporter = StdoutReporter::new();
    runner.run(&reporter)?;
    reporter.close()?;

    Ok(())
}
