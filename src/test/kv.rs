#[cfg(test)]
use crate::kv::KeyValues;

#[test]
fn test_long_vmt() {
    let kv = include_bytes!("../../test-data/water_pretty1_beneath.vmt");

    KeyValues::from_io(kv.as_slice()).unwrap();
}

#[test]
fn test_long_vmf() {
    let kv = include_bytes!("../../test-data/outputtest.vmf");

    KeyValues::from_io(kv.as_slice()).unwrap();
}
