extern crate parables_build;

fn main() {
    if let Err(e) = parables_build::compile(concat!(env!("CARGO_MANIFEST_DIR"), "/contracts")) {
        panic!("failed to compile contracts: {:?}", e);
    }
}
