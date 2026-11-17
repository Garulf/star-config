//! Read-only access to the entries of a `Data.p4k` archive.
//!
//! The archive is a ZIP64 file whose entry names use backslashes and whose
//! payloads use stored, deflate or zstd (method 100) compression. Only the
//! central directory is read when the archive is opened; each entry is read
//! on demand with its own seek.

use crate::error::{Error, Result};
use flate2::read::DeflateDecoder;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub const MAX_ENTRY_BYTES: u64 = 64 * 1024 * 1024;

const EOCD_SEARCH_BYTES: u64 = 1 << 20;
const EOCD_SIG: &[u8] = b"PK\x05\x06";
const ZIP64_LOCATOR_SIG: &[u8] = b"PK\x06\x07";
const ZIP64_RECORD_SIG: &[u8] = b"PK\x06\x06";
const ZIP64_RECORD_LEN: usize = 56;
const CENTRAL_ENTRY_LEN: usize = 46;
const LOCAL_HEADER_LEN: usize = 30;
const ZSTD_MAGIC: &[u8] = b"\x28\xb5\x2f\xfd";
const METHOD_STORE: u16 = 0;
const METHOD_DEFLATE: u16 = 8;
const METHOD_ZSTD: u16 = 100;
const ZIP64_EXTRA_ID: u16 = 1;
const U32_MAX: u64 = 0xFFFF_FFFF;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryInfo {
    pub name: String,
    pub method: u16,
    pub compressed: u64,
    pub size: u64,
}

#[derive(Debug)]
struct Entry {
    info: EntryInfo,
    header_offset: u64,
}

#[derive(Debug)]
pub struct Archive {
    path: PathBuf,
    index: HashMap<String, Entry>,
}

impl Archive {
    pub fn open(path: &Path) -> Result<Archive> {
        let mut file = File::open(path).map_err(|err| Error::io(path, err))?;
        let file_len = file.metadata().map_err(|err| Error::io(path, err))?.len();
        let bad = |message: &str| Error::Parse {
            path: path.to_path_buf(),
            line: 0,
            message: message.to_string(),
        };
        let io = |err| Error::io(path, err);

        let directory = find_directory(&mut file, file_len, io, bad)?;
        if directory
            .offset
            .checked_add(directory.size)
            .is_none_or(|end| end > file_len)
        {
            return Err(bad("the central directory lies past the end of the file"));
        }
        let mut raw = Vec::new();
        file.seek(SeekFrom::Start(directory.offset)).map_err(io)?;
        file.by_ref()
            .take(directory.size)
            .read_to_end(&mut raw)
            .map_err(io)?;
        if raw.len() as u64 != directory.size {
            return Err(bad("the central directory is truncated"));
        }
        let index = parse_directory(&raw).map_err(bad)?;
        Ok(Archive {
            path: path.to_path_buf(),
            index,
        })
    }

    pub fn entry(&self, name: &str) -> Option<&EntryInfo> {
        self.index.get(&key(name)).map(|entry| &entry.info)
    }

    pub fn read(&self, name: &str) -> Result<Vec<u8>> {
        let entry = self
            .index
            .get(&key(name))
            .ok_or_else(|| Error::NotFound(name.to_string()))?;
        let info = &entry.info;
        if info.size > MAX_ENTRY_BYTES || info.compressed > MAX_ENTRY_BYTES {
            return Err(Error::Invalid(format!(
                "{} is larger than star-config reads ({} bytes)",
                info.name,
                info.size.max(info.compressed)
            )));
        }
        let payload = self.read_payload(entry)?;
        match info.method {
            METHOD_STORE => Ok(payload),
            METHOD_DEFLATE => self.inflate(info, DeflateDecoder::new(payload.as_slice())),
            METHOD_ZSTD => {
                if !payload.starts_with(ZSTD_MAGIC) {
                    return Err(Error::Invalid(format!(
                        "{} is encrypted, which star-config cannot read",
                        info.name
                    )));
                }
                let decoder = ruzstd::decoding::StreamingDecoder::new(payload.as_slice())
                    .map_err(|err| self.damaged(info, err))?;
                self.inflate(info, decoder)
            }
            other => Err(Error::Invalid(format!(
                "{} uses compression method {other}, which star-config cannot read",
                info.name
            ))),
        }
    }

    fn read_payload(&self, entry: &Entry) -> Result<Vec<u8>> {
        let io = |err| Error::io(&self.path, err);
        let mut file = File::open(&self.path).map_err(io)?;
        file.seek(SeekFrom::Start(entry.header_offset))
            .map_err(io)?;
        let mut header = [0u8; LOCAL_HEADER_LEN];
        file.read_exact(&mut header)
            .map_err(|err| self.damaged(&entry.info, err))?;
        let skip = u64::from(u16_at(&header, 26)) + u64::from(u16_at(&header, 28));
        file.seek(SeekFrom::Current(skip as i64)).map_err(io)?;
        let mut payload = Vec::new();
        file.take(entry.info.compressed)
            .read_to_end(&mut payload)
            .map_err(io)?;
        if payload.len() as u64 != entry.info.compressed {
            return Err(self.damaged(&entry.info, "the data ends early"));
        }
        Ok(payload)
    }

    fn inflate(&self, info: &EntryInfo, mut decoder: impl Read) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        decoder
            .by_ref()
            .take(MAX_ENTRY_BYTES + 1)
            .read_to_end(&mut out)
            .map_err(|err| self.damaged(info, err))?;
        if out.len() as u64 > MAX_ENTRY_BYTES {
            return Err(Error::Invalid(format!(
                "{} is larger than star-config reads",
                info.name
            )));
        }
        Ok(out)
    }

    fn damaged(&self, info: &EntryInfo, cause: impl std::fmt::Display) -> Error {
        Error::Parse {
            path: self.path.clone(),
            line: 0,
            message: format!("{}: {cause}", info.name),
        }
    }
}

fn key(name: &str) -> String {
    name.replace('\\', "/").to_lowercase()
}

struct Directory {
    offset: u64,
    size: u64,
}

fn find_directory(
    file: &mut File,
    file_len: u64,
    io: impl Fn(std::io::Error) -> Error,
    bad: impl Fn(&str) -> Error,
) -> Result<Directory> {
    let tail_len = file_len.min(EOCD_SEARCH_BYTES);
    file.seek(SeekFrom::Start(file_len - tail_len))
        .map_err(&io)?;
    let mut tail = Vec::new();
    file.by_ref()
        .take(tail_len)
        .read_to_end(&mut tail)
        .map_err(&io)?;
    let eocd_at = rfind(&tail, EOCD_SIG, tail.len())
        .filter(|at| tail.len() - at >= 22)
        .ok_or_else(|| bad("not a Data.p4k: no end of central directory"))?;
    let classic = Directory {
        size: u64::from(u32_at(&tail, eocd_at + 12)),
        offset: u64::from(u32_at(&tail, eocd_at + 16)),
    };
    let Some(locator_at) = rfind(&tail, ZIP64_LOCATOR_SIG, eocd_at) else {
        return Ok(classic);
    };
    if eocd_at - locator_at < 20 {
        return Ok(classic);
    }
    let record_at = u64_at(&tail, locator_at + 8);
    if record_at
        .checked_add(ZIP64_RECORD_LEN as u64)
        .is_none_or(|end| end > file_len)
    {
        return Err(bad("the ZIP64 record lies past the end of the file"));
    }
    file.seek(SeekFrom::Start(record_at)).map_err(&io)?;
    let mut record = [0u8; ZIP64_RECORD_LEN];
    file.read_exact(&mut record).map_err(&io)?;
    if !record.starts_with(ZIP64_RECORD_SIG) {
        return Ok(classic);
    }
    Ok(Directory {
        size: u64_at(&record, 40),
        offset: u64_at(&record, 48),
    })
}

fn parse_directory(raw: &[u8]) -> std::result::Result<HashMap<String, Entry>, &'static str> {
    let mut index = HashMap::new();
    let mut at = 0;
    while raw.len() - at >= CENTRAL_ENTRY_LEN && raw[at..].starts_with(b"PK") {
        let fixed = &raw[at..at + CENTRAL_ENTRY_LEN];
        let name_len = usize::from(u16_at(fixed, 28));
        let extra_len = usize::from(u16_at(fixed, 30));
        let comment_len = usize::from(u16_at(fixed, 32));
        let name_end = at + CENTRAL_ENTRY_LEN + name_len;
        let extra_end = name_end + extra_len;
        if extra_end + comment_len > raw.len() {
            return Err("a central directory entry runs past the end");
        }
        let name = String::from_utf8_lossy(&raw[at + CENTRAL_ENTRY_LEN..name_end]).into_owned();
        let mut compressed = u64::from(u32_at(fixed, 20));
        let mut size = u64::from(u32_at(fixed, 24));
        let mut header_offset = u64::from(u32_at(fixed, 42));
        apply_zip64_extra(
            &raw[name_end..extra_end],
            &mut size,
            &mut compressed,
            &mut header_offset,
        )?;
        let info = EntryInfo {
            name,
            method: u16_at(fixed, 10),
            compressed,
            size,
        };
        index.insert(
            key(&info.name),
            Entry {
                info,
                header_offset,
            },
        );
        at = extra_end + comment_len;
    }
    if index.is_empty() && !raw.is_empty() {
        return Err("the central directory holds no readable entries");
    }
    Ok(index)
}

fn apply_zip64_extra(
    extra: &[u8],
    size: &mut u64,
    compressed: &mut u64,
    header_offset: &mut u64,
) -> std::result::Result<(), &'static str> {
    let mut at = 0;
    while at + 4 <= extra.len() {
        let id = u16_at(extra, at);
        let len = usize::from(u16_at(extra, at + 2));
        let body = &extra[at + 4..(at + 4 + len).min(extra.len())];
        if id == ZIP64_EXTRA_ID {
            let (words, _) = body.as_chunks::<8>();
            let mut values = words.iter().map(|word| u64::from_le_bytes(*word));
            for field in [&mut *size, &mut *compressed, &mut *header_offset] {
                if *field == U32_MAX {
                    *field = values.next().ok_or("a ZIP64 extra field is too short")?;
                }
            }
        }
        at += 4 + len;
    }
    Ok(())
}

fn rfind(haystack: &[u8], needle: &[u8], end: usize) -> Option<usize> {
    haystack[..end]
        .windows(needle.len())
        .rposition(|window| window == needle)
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().expect("eight bytes"))
}

#[cfg(test)]
pub(crate) mod testing {
    use flate2::Compression;
    use flate2::write::DeflateEncoder;
    use ruzstd::encoding::{CompressionLevel, compress_to_vec};
    use std::io::Write;

    #[derive(Clone, Copy)]
    pub enum Method {
        Store,
        Deflate,
        Zstd,
        ZstdGarbage,
    }

    pub fn build(entries: &[(&str, Method, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, method, data) in entries {
            let (code, payload) = encode(*method, data);
            let offset = out.len() as u64;
            out.extend(b"PK\x03\x14");
            out.extend([20, 0, 0, 0]);
            out.extend(code.to_le_bytes());
            out.extend([0u8; 16]);
            out.extend((name.len() as u16).to_le_bytes());
            out.extend(4u16.to_le_bytes());
            out.extend(name.as_bytes());
            out.extend([0xAA; 4]);
            out.extend(&payload);

            central.extend(b"PK\x01\x02");
            central.extend([45, 0, 45, 0, 0, 0]);
            central.extend(code.to_le_bytes());
            central.extend([0u8; 8]);
            central.extend(u32::MAX.to_le_bytes());
            central.extend(u32::MAX.to_le_bytes());
            central.extend((name.len() as u16).to_le_bytes());
            central.extend(28u16.to_le_bytes());
            central.extend([0u8; 10]);
            central.extend(u32::MAX.to_le_bytes());
            central.extend(name.as_bytes());
            central.extend(1u16.to_le_bytes());
            central.extend(24u16.to_le_bytes());
            central.extend((data.len() as u64).to_le_bytes());
            central.extend((payload.len() as u64).to_le_bytes());
            central.extend(offset.to_le_bytes());
        }
        let directory_at = out.len() as u64;
        out.extend(&central);
        let record_at = out.len() as u64;
        out.extend(b"PK\x06\x06");
        out.extend(44u64.to_le_bytes());
        out.extend([45, 0, 45, 0]);
        out.extend([0u8; 8]);
        out.extend((entries.len() as u64).to_le_bytes());
        out.extend((entries.len() as u64).to_le_bytes());
        out.extend((central.len() as u64).to_le_bytes());
        out.extend(directory_at.to_le_bytes());
        out.extend(b"PK\x06\x07");
        out.extend(0u32.to_le_bytes());
        out.extend(record_at.to_le_bytes());
        out.extend(1u32.to_le_bytes());
        out.extend(b"PK\x05\x06");
        out.extend([0xFF; 4]);
        out.extend([0xFF; 4]);
        out.extend(u32::MAX.to_le_bytes());
        out.extend(u32::MAX.to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out
    }

    fn encode(method: Method, data: &[u8]) -> (u16, Vec<u8>) {
        match method {
            Method::Store => (0, data.to_vec()),
            Method::Deflate => {
                let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
                encoder.write_all(data).unwrap();
                (8, encoder.finish().unwrap())
            }
            Method::Zstd => (100, compress_to_vec(data, CompressionLevel::Fastest)),
            Method::ZstdGarbage => {
                let mut payload = vec![0xDE, 0xAD, 0xBE, 0xEF];
                payload.extend(data);
                (100, payload)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{Method, build};
    use super::*;

    const PROFILE: &[u8] = b"<profile>some bytes that compress, some bytes that compress</profile>";

    fn archive_of(bytes: &[u8]) -> (tempfile::TempDir, Result<Archive>) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Data.p4k");
        std::fs::write(&path, bytes).unwrap();
        let archive = Archive::open(&path);
        (dir, archive)
    }

    fn single(method: Method) -> Vec<u8> {
        build(&[("Data\\one.bin", method, PROFILE)])
    }

    fn set_sizes(archive: &mut [u8], size: u64, compressed: u64) {
        let directory = archive
            .windows(4)
            .rposition(|window| window == b"PK\x01\x02")
            .unwrap();
        let extra = directory + CENTRAL_ENTRY_LEN + "Data\\one.bin".len() + 4;
        archive[extra..extra + 8].copy_from_slice(&size.to_le_bytes());
        archive[extra + 8..extra + 16].copy_from_slice(&compressed.to_le_bytes());
    }

    #[test]
    fn reads_store_and_zstd_entries_by_any_case_and_separator() {
        let bytes = build(&[
            (
                "Data\\Libs\\Config\\defaultProfile.xml",
                Method::Zstd,
                PROFILE,
            ),
            ("Data\\a.txt", Method::Store, b"hello"),
        ]);
        let (_dir, archive) = archive_of(&bytes);
        let archive = archive.unwrap();
        assert_eq!(
            archive.read("data/libs/config/DEFAULTPROFILE.xml").unwrap(),
            PROFILE
        );
        assert_eq!(archive.entry("data\\a.txt").unwrap().size, 5);
        assert_eq!(archive.read("Data/a.txt").unwrap(), b"hello");
    }

    #[test]
    fn reads_deflate_entries() {
        let (_dir, archive) = archive_of(&single(Method::Deflate));
        let archive = archive.unwrap();
        assert_eq!(archive.entry("data/one.bin").unwrap().method, 8);
        assert_eq!(archive.read("data/one.bin").unwrap(), PROFILE);
    }

    #[test]
    fn entry_reports_the_name_as_stored_with_both_sizes() {
        let (_dir, archive) = archive_of(&single(Method::Zstd));
        let info = archive.unwrap().entry("DATA/ONE.BIN").unwrap().clone();
        assert_eq!(info.name, "Data\\one.bin");
        assert_eq!(info.method, 100);
        assert_eq!(info.size, PROFILE.len() as u64);
        assert!(info.compressed > 0);
    }

    #[test]
    fn encrypted_entry_is_reported_not_decoded() {
        let (_dir, archive) = archive_of(&single(Method::ZstdGarbage));
        match archive.unwrap().read("data/one.bin") {
            Err(Error::Invalid(message)) => assert!(message.contains("encrypted"), "{message}"),
            other => panic!("expected an encrypted error, got {other:?}"),
        }
    }

    #[test]
    fn missing_entry_is_not_found() {
        let (_dir, archive) = archive_of(&single(Method::Store));
        let archive = archive.unwrap();
        assert!(archive.entry("data/none.bin").is_none());
        assert!(matches!(
            archive.read("data/none.bin"),
            Err(Error::NotFound(_))
        ));
    }

    #[test]
    fn truncated_or_foreign_files_fail_cleanly() {
        let whole = single(Method::Zstd);
        let cases: [&[u8]; 4] = [
            &whole[..whole.len() / 2],
            &whole[..whole.len() - 1],
            b"0123456789",
            b"just some text that is not an archive at all, but longer than a header",
        ];
        for bytes in cases {
            let (_dir, archive) = archive_of(bytes);
            assert!(archive.is_err(), "opened {} bytes", bytes.len());
        }
    }

    #[test]
    fn a_missing_file_is_an_io_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            Archive::open(&dir.path().join("none.p4k")),
            Err(Error::Io { .. })
        ));
    }

    #[test]
    fn oversized_entry_is_refused() {
        let mut bytes = single(Method::Store);
        set_sizes(&mut bytes, MAX_ENTRY_BYTES + 1, PROFILE.len() as u64);
        let (_dir, archive) = archive_of(&bytes);
        assert!(matches!(
            archive.unwrap().read("data/one.bin"),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn oversized_compressed_claim_is_refused_before_allocating() {
        let mut bytes = single(Method::Store);
        set_sizes(&mut bytes, 10, MAX_ENTRY_BYTES + 1);
        let (_dir, archive) = archive_of(&bytes);
        assert!(matches!(
            archive.unwrap().read("data/one.bin"),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn data_claimed_past_the_end_of_the_file_is_an_error() {
        let mut bytes = single(Method::Store);
        set_sizes(&mut bytes, 1_000_000, 1_000_000);
        let (_dir, archive) = archive_of(&bytes);
        assert!(matches!(
            archive.unwrap().read("data/one.bin"),
            Err(Error::Parse { .. })
        ));
    }

    #[test]
    fn a_corrupt_zstd_body_is_an_error() {
        let mut bytes = single(Method::Zstd);
        let at = bytes
            .windows(4)
            .position(|window| window == ZSTD_MAGIC)
            .unwrap();
        bytes[at + 4..at + 12].fill(0xFF);
        let (_dir, archive) = archive_of(&bytes);
        assert!(archive.unwrap().read("data/one.bin").is_err());
    }

    #[test]
    fn a_central_entry_with_overlong_lengths_is_an_error() {
        let mut bytes = single(Method::Store);
        let directory = bytes
            .windows(4)
            .rposition(|window| window == b"PK\x01\x02")
            .unwrap();
        bytes[directory + 28..directory + 30].copy_from_slice(&u16::MAX.to_le_bytes());
        let (_dir, archive) = archive_of(&bytes);
        assert!(archive.is_err());
    }

    #[test]
    fn every_prefix_of_an_archive_is_handled_without_panicking() {
        let whole = build(&[
            ("Data\\a.txt", Method::Store, b"hello"),
            ("Data\\b.bin", Method::Zstd, PROFILE),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Data.p4k");
        for len in 0..whole.len() {
            std::fs::write(&path, &whole[..len]).unwrap();
            if let Ok(archive) = Archive::open(&path) {
                let _ = archive.read("data/a.txt");
                let _ = archive.read("data/b.bin");
            }
        }
    }
}
