#[cfg(test)]
use crate::vpk::VPK;

use std::io::Seek;
#[cfg(test)]
use std::{
    io::{Read, SeekFrom},
    path::Path,
};

#[test]
fn test_chunk_vpk() {
    let mut vpk = VPK::load(Path::new("test-data/Misc_dir.vpk")).unwrap();

    let mut chapter1 = vpk.get(Path::new("cfg/chapter1.cfg")).unwrap();
    chapter1.verify().unwrap();

    let chapter1_truth = include_bytes!("../../test-data/chapter1.cfg");

    let mut chapter1_data = vec![0u8; chapter1.len()];
    assert_eq!(
        chapter1.read(chapter1_data.as_mut_slice()).unwrap(),
        chapter1_truth.len()
    );

    assert_eq!(chapter1_data, chapter1_truth);

    assert_eq!(chapter1.seek(SeekFrom::Start(10)).unwrap(), 10);
    assert_eq!(
        chapter1
            .read(&mut chapter1_data.as_mut_slice()[10..])
            .unwrap(),
        chapter1_truth.len() - 10
    );
    assert_eq!(chapter1_data, chapter1_truth);
}

#[test]
fn test_chunkless_vpk() {
    let mut vpk = VPK::load(Path::new("test-data/blastoffold.vpk")).unwrap();

    let mut blastoff = vpk.get(Path::new("blastoff.nut")).unwrap();
    blastoff.verify().unwrap();

    let blastoff_truth = include_bytes!("../../test-data/blastoff.nut");

    let mut blastoff_data = vec![0u8; blastoff.len()];
    assert_eq!(
        blastoff.read(blastoff_data.as_mut_slice()).unwrap(),
        blastoff_truth.len()
    );

    assert_eq!(blastoff_data, blastoff_truth);
}
