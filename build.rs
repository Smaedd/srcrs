use fs_extra::dir::{copy, CopyOptions};
use std::env;

fn main() {
    if cfg!(test) {
        println!("cargo:rerun-if-changed=test-data/*");

        let mut options = CopyOptions::new();
        options.overwrite = true;
        options.content_only = false;

        copy(
            env::var("CARGO_MANIFEST_DIR").unwrap() + "\\test-data",
            env::var("OUT_DIR").unwrap(),
            &options,
        )
        .unwrap();
    }
}
