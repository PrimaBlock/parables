fn main() {
    println!("cargo:rerun-if-changed=contracts/SimpleLedger.sol");
    println!("cargo:rerun-if-changed=contracts/SimpleContract.sol");
    println!("cargo:rerun-if-changed=contracts/SimpleLib.sol");
}
