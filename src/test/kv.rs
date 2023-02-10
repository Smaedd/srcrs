#[cfg(test)]
use crate::kv::Object;

#[test]
fn test_long_vmt() {
    let kv = include_bytes!("../../test-data/water_pretty1_beneath.vmt");

    Object::from_io(kv.as_slice()).unwrap();
}

#[test]
fn test_long_vmf() {
    let kv = include_bytes!("../../test-data/outputtest.vmf");

    Object::from_io(kv.as_slice()).unwrap();
}
