#[macro_use]
extern crate parables_testing;
#[macro_use]
extern crate failure;

use parables_testing::prelude::*;

contracts! {
    simple_contract {
        "SimpleContract",
        "contracts/SimpleContract_sol_SimpleContract.abi",
        "contracts/SimpleContract_sol_SimpleContract.bin"
    },
    simple_lib {
        "SimpleLib",
        "contracts/SimpleLib_sol_SimpleLib.abi",
        "contracts/SimpleLib_sol_SimpleLib.bin"
    },
}

fn main() -> Result<()> {
    let foundation = Spec::new_null();

    let client =
        EvmTestClient::new(&foundation).map_err(|e| format_err!("failed to create client: {}", e))?;

    let mut evm = Evm::new(client);
    let mut linker = Linker::new();

    let owner = Address::random();

    // template call
    let call = Call::new(owner).gas(1_000_000);

    let simple_lib = simple_lib::SimpleLib::default();

    let simple_lib_code = simple_lib::bin(&linker)?;
    let simple_lib_address = evm.deploy(simple_lib.constructor(simple_lib_code), call)?;
    linker.register_item("SimpleLib".to_string(), simple_lib_address);

    let mut current = 42u64;

    let simple_contract = simple_contract::SimpleContract::default();
    let simple_contract_code = simple_contract::bin(&linker)?;
    let simple = evm.deploy(
        simple_contract.constructor(simple_contract_code, current),
        call,
    )?;

    {
        let f = simple_contract.functions();

        let out = evm.call(simple, f.get_value(), call)?;
        assert_eq!(out, current.into());

        evm.call(simple, f.test_add(10, 20), call)?;
        current = 30u64;

        for _ in 0..1000 {
            let out = evm.call(simple, f.get_value(), call)?;
            assert_eq!(out, current.into());
            evm.call(simple, f.set_value(out + 1.into()), call)?;
            current += 1;
        }

        let not_owner = Address::random();

        // non-owner is not allowed to set value.
        let non_owned_res = evm.call(simple, f.set_value(0), call.sender(not_owner));
        assert!(non_owned_res.is_reverted());

        let ev = simple_contract.events();

        let filter = Filter::new(ev.value_updated(), |e| e.create_filter(Some(100.into())))?;

        // check logs.
        for log in evm.drain_logs(&filter) {
            println!("log: {:?}", log);
        }
    }

    Ok(())
}
