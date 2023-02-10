#[cfg(test)]
use crate::vpk::{File, VPK};

#[cfg(test)]
use std::{io::Read, path::Path};

#[test]
fn test_vpk() {
    let mut vpk = VPK::load(Path::new("test-data/Misc_dir.vpk")).unwrap();

    let mut chapter1 = vpk.get(Path::new("cfg/chapter1.cfg")).unwrap();

    let mut chapter1_data = vec![0u8; chapter1.len()];
    chapter1.read(chapter1_data.as_mut_slice()).unwrap();

    let chapter1_truth = include_bytes!("../../test-data/chapter1.cfg");
    assert_eq!(chapter1_data, chapter1_truth);
}
