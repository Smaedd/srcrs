use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{Error, ErrorKind, Read, Result, Seek, SeekFrom};
use std::mem;
use std::path::{Path, PathBuf};
use std::str;

use zerocopy::FromBytes;

#[repr(C, packed)]
#[derive(FromBytes, Default)]
struct VPKHeaderV1 {
    signature: u32,
    version: u32,

    tree_size: u32,
}

#[repr(C, packed)]
#[derive(FromBytes, Default)]
struct VPKHeaderV2 {
    v1: VPKHeaderV1,

    file_data_section_size: u32,
    archive_md5_section_size: u32,
    other_md5_section_size: u32,
    signature_section_size: u32,
}

#[repr(C, packed)]
#[derive(FromBytes)]
struct VPKDirectoryEntry {
    crc: u32,
    preload_bytes: u16,

    archive_index: u16,
    entry_offset: u32,
    entry_length: u32,

    terminator: u16,
}

const VPK_SIGNATURE: u32 = 0x55aa1234;

pub struct VPK {
    path: PathBuf,
    base_path: PathBuf,
    files: HashMap<PathBuf, VPKFile>,
}

const DIRECTORY_INDEX: u16 = 0x7FFF;

struct VPKFile {
    crc: u32,

    preload_data: Vec<u8>,

    archive_index: u16,
    archive_offset: u64, // Larger for DIRECTORY_INDEX case
    archive_length: u32,
}

impl VPK {
    pub fn load(path: &Path) -> Result<VPK> {
        let mut vpk_file = fs::File::open(path)?;

        let base_path = {
            let file_name = path
                .file_name()
                .unwrap()
                .to_str()
                .expect("Non-UTF8 paths not supported");

            path.with_file_name::<OsString>(file_name.replace("_dir", "").into())
        };

        let mut vpk = VPK {
            path: path.into(),
            base_path: base_path,
            files: HashMap::new(),
        };

        vpk.load_internal(&mut vpk_file)?;
        Ok(vpk)
    }

    fn load_internal(&mut self, vpk_file: &mut fs::File) -> Result<()> {
        let mut header_data = [0u8; mem::size_of::<VPKHeaderV2>()];
        vpk_file.read(&mut header_data[..mem::size_of::<VPKHeaderV1>()])?;

        let v1_header = VPKHeaderV1::read_from_prefix(header_data.as_slice()).unwrap();

        if v1_header.signature != VPK_SIGNATURE {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "Invalid VPK signature encountered. Is this a vpk file?".to_string(),
            ));
        }

        let version = v1_header.version;
        match version {
            2 => {
                vpk_file.read(&mut header_data[mem::size_of::<VPKHeaderV1>()..])?;

                let v2_header = VPKHeaderV2::read_from_prefix(header_data.as_slice()).unwrap();

                self.load_v2(v2_header, vpk_file)?;
            }
            1 => self.load_v1(v1_header, vpk_file)?,
            _ => {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    format!("Invalid version number {}", version),
                ))
            }
        }

        Ok(())
    }

    fn read_string(data: &[u8]) -> Result<(usize, &str)> {
        let terminator = data
            .iter()
            .position(|&byte| byte == 0x00)
            .expect("Tree entry with unterminated name");

        let parsed_str = str::from_utf8(&data[..terminator]).or_else(|_| {
            Err(Error::new(
                ErrorKind::InvalidData,
                "Invalid tree entry name (Non-UTF8)",
            ))
        })?;

        Ok((terminator + 1, parsed_str))
    }

    fn load_tree(
        &mut self,
        tree_size: usize,
        header_offset: usize,
        vpk_file: &mut fs::File,
    ) -> Result<()> {
        let mut loaded_data = vec![0u8; tree_size];
        vpk_file.read(loaded_data.as_mut_slice())?;
        let loaded_data = loaded_data;

        let mut position = 0usize;
        while position < tree_size {
            let (num_read, extension) = Self::read_string(&loaded_data[position..])?;
            position += num_read;

            if extension.is_empty() {
                break;
            }

            let extension = if extension == " " { "" } else { extension };

            loop {
                let (num_read, path) = Self::read_string(&loaded_data[position..])?;
                position += num_read;

                if path.is_empty() {
                    break;
                }

                let path = if path == " " { "" } else { path };

                loop {
                    let (num_read, file_name) = Self::read_string(&loaded_data[position..])?;
                    position += num_read;

                    if file_name.is_empty() {
                        break;
                    }

                    let file_name = if file_name == " " { "" } else { file_name };

                    let mut full_path = PathBuf::from(path);
                    full_path.push(OsStr::new(file_name));
                    full_path.set_extension(extension);

                    let directory_entry = VPKDirectoryEntry::read_from_prefix(
                        &loaded_data[position..],
                    )
                    .ok_or_else(|| Error::new(ErrorKind::InvalidData, "VPK tree malformed"))?;
                    position += mem::size_of::<VPKDirectoryEntry>();

                    let preload_data = Vec::from(
                        &loaded_data[position..position + directory_entry.preload_bytes as usize],
                    );
                    position += directory_entry.preload_bytes as usize;

                    let archive_offset = if directory_entry.archive_index == DIRECTORY_INDEX {
                        directory_entry.entry_offset as u64 + header_offset as u64
                    } else {
                        directory_entry.entry_offset as u64
                    };

                    let vpkfile = VPKFile {
                        crc: directory_entry.crc,
                        preload_data: preload_data,
                        archive_index: directory_entry.archive_index,
                        archive_offset: archive_offset,
                        archive_length: directory_entry.entry_length,
                    };

                    self.files.insert(full_path, vpkfile);
                }
            }
        }

        Ok(())
    }

    fn load_v2(&mut self, header: VPKHeaderV2, vpk_file: &mut fs::File) -> Result<()> {
        self.load_tree(
            header.v1.tree_size as usize,
            mem::size_of::<VPKHeaderV2>() + header.v1.tree_size as usize,
            vpk_file,
        )?;

        // Don't bother with the rest for now
        Ok(())
    }

    fn load_v1(&mut self, header: VPKHeaderV1, vpk_file: &mut fs::File) -> Result<()> {
        self.load_tree(
            header.tree_size as usize,
            mem::size_of::<VPKHeaderV1>() + header.tree_size as usize,
            vpk_file,
        )?;

        Ok(())
    }

    pub fn get(&mut self, path: &Path) -> Result<File<'_>> {
        let entry = self.files.get(path).ok_or_else(|| {
            Error::new(
                ErrorKind::NotFound,
                format!("{} not found in VPK", path.display()),
            )
        })?;

        // Handle preload data case
        if entry.archive_length == 0 {
            return Ok(File {
                fs_file: None,
                metadata: entry,
                position: 0,
            });
        }

        let archive_name = if entry.archive_index == DIRECTORY_INDEX {
            self.path.clone()
        } else {
            let mut file_prefix =
                OsString::from(self.base_path.with_extension("").file_name().unwrap());

            file_prefix.push(format!("_{:03}", entry.archive_index));
            self.base_path
                .with_file_name(file_prefix)
                .with_extension(self.base_path.extension().unwrap())
        };

        let mut fs_file = fs::File::open(archive_name)?;
        fs_file.seek(SeekFrom::Start(entry.archive_offset))?;

        Ok(File {
            fs_file: Some(fs_file),
            metadata: entry,
            position: 0,
        })
    }
}

// Should implement Read and Seek, CANNOT implement Write (just yet).
pub struct File<'a> {
    fs_file: Option<fs::File>, // None if preload data is all that is needed.
    metadata: &'a VPKFile,

    position: u64,
}

impl<'a> Read for File<'a> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let preload_len = self.metadata.preload_data.len();
        let total_size = self.metadata.archive_length as usize + preload_len;
        let position = self.position as usize;

        let maximum_read = usize::min(total_size - position as usize, buf.len());

        let read_buf = &mut buf[..maximum_read];

        if position < preload_len {
            let maximum_preload_read = usize::min(preload_len - position, read_buf.len());

            read_buf[..maximum_preload_read].clone_from_slice(
                &self.metadata.preload_data.as_slice()[position..position + maximum_preload_read],
            );

            if let Some(file) = self.fs_file.as_mut() {
                let num_read = file.read(
                    &mut read_buf[maximum_preload_read..maximum_read - maximum_preload_read],
                )?;

                Ok(maximum_preload_read + num_read)
            } else {
                Ok(maximum_preload_read)
            }
        } else if let Some(file) = self.fs_file.as_mut() {
            file.read(&mut read_buf[..maximum_read])?;

            Ok(maximum_read)
        } else {
            Ok(0)
        }
    }
}

impl<'a> Seek for File<'a> {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
        self.position = match pos {
            SeekFrom::Current(offset) => self.position + offset as u64,
            SeekFrom::End(offset) => (self.metadata.archive_length as i128 + offset as i128) as u64,
            SeekFrom::Start(offset) => offset,
        };

        if let Some(file) = self.fs_file.as_mut() {
            let file_position = i128::max(
                self.position as i128 - self.metadata.preload_data.len() as i128,
                0,
            ) as u64;

            file.seek(SeekFrom::Start(
                self.metadata.archive_offset + file_position,
            ))?;
        }

        Ok(self.position)
    }

    #[cfg(seek_stream_len)]
    fn stream_len(&mut self) -> Result<u64> {
        Ok(self.metadata.archive_length as u64)
    }

    fn stream_position(&mut self) -> Result<u64> {
        Ok(self.position)
    }
}

impl<'a> File<'a> {
    pub fn len(&self) -> usize {
        self.metadata.archive_length as usize
    }

    pub fn verify(&mut self) -> Result<()> {
        let old_position = self.stream_position()?;

        let crc_maybe = self.calc_crc32();
        self.seek(SeekFrom::Start(old_position))?;

        match crc_maybe {
            Ok(crc) => {
                if crc != self.metadata.crc {
                    Err(Error::new(
                        ErrorKind::InvalidData,
                        format!(
                            "Calculated crc {} does not match stored crc {}",
                            crc, self.metadata.crc
                        ),
                    ))
                } else {
                    Ok(())
                }
            }
            Err(err) => Err(err),
        }
    }

    fn calc_crc32(&mut self) -> Result<u32> {
        self.seek(SeekFrom::Start(0))?;

        let mut data = vec![0; self.len()];
        self.read(data.as_mut_slice())?;

        Ok(crc32fast::hash(&data))
    }
}
