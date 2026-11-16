//! Integrity-checked state envelopes compatible with the existing application.
#![forbid(unsafe_code)]
mod session;
use serde::{Deserialize, Serialize};
pub use session::SaveSession;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
pub const MAX_PAYLOAD: u64 = 128 * 1024 * 1024;
const MAX_HEADER: usize = 16384;
const MAGIC: &[u8] = b"EMULUNA1\n";
const LEGACY_MAGIC: &[u8] = b"OELINUX1\n";
pub const STATE_EXTENSION: &str = "emuluna-state";
const LEGACY_EXTENSION: &str = "oesavestate";

pub fn automatic_path(directory: &Path) -> PathBuf {
    directory.join("auto").with_extension(STATE_EXTENSION)
}

pub fn slot_path(directory: &Path, slot: u8) -> PathBuf {
    directory
        .join(format!("slot-{slot}"))
        .with_extension(STATE_EXTENSION)
}

pub fn readable_path(path: &Path) -> PathBuf {
    if path.extension().is_some_and(|ext| ext == STATE_EXTENSION) && !path.is_file() {
        let legacy = path.with_extension(LEGACY_EXTENSION);
        if legacy.is_file() {
            return legacy;
        }
    }
    path.to_owned()
}

pub fn has_state_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            ext.eq_ignore_ascii_case(STATE_EXTENSION) || ext.eq_ignore_ascii_case(LEGACY_EXTENSION)
        })
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Renderer {
    #[default]
    #[serde(rename = "software")]
    Software,
    #[serde(rename = "opengl")]
    OpenGl,
    #[serde(rename = "vulkan")]
    Vulkan,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub core: String,
    pub rom: String,
    pub renderer: Renderer,
}
#[derive(Serialize, Deserialize)]
struct Header {
    format: u32,
    core: String,
    rom: String,
    #[serde(default)]
    renderer: Renderer,
    sha256: String,
}
fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn hash_string(digest: impl AsRef<[u8]>) -> String {
    digest.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}
fn hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl Identity {
    pub fn new(
        core_id: &str,
        binary_digest: &str,
        rom_digest: &str,
        renderer: Renderer,
    ) -> Result<Self, String> {
        if core_id.is_empty()
            || core_id.len() > 128
            || !core_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            || !hex_digest(binary_digest)
            || !hex_digest(rom_digest)
        {
            return Err("Invalid state identity".into());
        }
        Ok(Self {
            core: format!("libretro:{core_id}:{binary_digest}"),
            rom: rom_digest.into(),
            renderer,
        })
    }
}
pub fn digest_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(error)?;
    if !file.metadata().map_err(error)?.is_file() {
        return Err("Expected a regular file".into());
    }
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer).map_err(error)?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
    }
    Ok(hash_string(digest.finalize()))
}
fn copy_payload(mut input: impl Read, mut output: impl Write) -> Result<(u64, String), String> {
    let mut limited = (&mut input).take(MAX_PAYLOAD + 1);
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut total = 0;
    loop {
        let n = limited.read(&mut buffer).map_err(error)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > MAX_PAYLOAD {
            return Err("The saved state is too large.".into());
        }
        output.write_all(&buffer[..n]).map_err(error)?;
        digest.update(&buffer[..n]);
    }
    if total == 0 {
        return Err("The saved state has no payload.".into());
    }
    Ok((total, hash_string(digest.finalize())))
}
fn open_state(path: &Path) -> Result<BufReader<File>, String> {
    let file = File::open(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "There is no saved state in this slot yet.".to_string()
        } else {
            e.to_string()
        }
    })?;
    let metadata = file.metadata().map_err(error)?;
    if !metadata.is_file() || metadata.len() > MAX_PAYLOAD + MAX_HEADER as u64 + MAGIC.len() as u64
    {
        return Err("The saved state is too large or is not a regular file.".into());
    }
    Ok(BufReader::new(file))
}
fn line(reader: &mut impl BufRead, limit: usize) -> Result<Vec<u8>, String> {
    let mut result = Vec::new();
    (&mut *reader)
        .take(limit as u64 + 1)
        .read_until(b'\n', &mut result)
        .map_err(error)?;
    if result.len() > limit || result.last() != Some(&b'\n') {
        return Err("Invalid or oversized state header.".into());
    }
    Ok(result)
}
fn header(reader: &mut impl BufRead) -> Result<Header, String> {
    let magic = line(reader, 32)?;
    if magic != MAGIC && magic != LEGACY_MAGIC {
        return Err("This is not an EmuLuna saved state.".into());
    }
    let header: Header = serde_json::from_slice(&line(reader, MAX_HEADER)?)
        .map_err(|_| "The saved state header is damaged.")?;
    if header.format != 1
        || !hex_digest(&header.sha256)
        || !hex_digest(&header.rom)
        || header.core.len() > 2048
    {
        return Err("Invalid saved state metadata.".into());
    }
    Ok(header)
}
/// Inspect only the bounded envelope header, without loading its large payload.
pub fn inspect(path: &Path) -> Result<Identity, String> {
    let header = header(&mut open_state(path)?)?;
    Ok(Identity {
        core: header.core,
        rom: header.rom,
        renderer: header.renderer,
    })
}
/// Atomically replace a slot only after its complete envelope is flushed. The
/// temporary file lives beside the slot, so publication cannot cross devices.
pub fn write_from_raw(destination: &Path, raw: &Path, identity: &Identity) -> Result<(), String> {
    let mut source = File::open(raw).map_err(error)?;
    let metadata = source.metadata().map_err(error)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_PAYLOAD {
        return Err("Invalid native state payload size.".into());
    }
    if !hex_digest(&identity.rom) || identity.core.len() > 2048 {
        return Err("Invalid state identity".into());
    }
    let mut header = Header {
        format: 1,
        core: identity.core.clone(),
        rom: identity.rom.clone(),
        renderer: identity.renderer,
        sha256: "0".repeat(64),
    };
    let placeholder = serde_json::to_vec(&header).map_err(error)?;
    if placeholder.len() + 1 > MAX_HEADER {
        return Err("State header exceeds its limit.".into());
    }
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(error)?;
    let mut output = tempfile::NamedTempFile::new_in(parent).map_err(error)?;
    output.write_all(MAGIC).map_err(error)?;
    output.write_all(&placeholder).map_err(error)?;
    output.write_all(b"\n").map_err(error)?;
    let (_, digest) = copy_payload(&mut source, &mut output)?;
    header.sha256 = digest;
    let encoded = serde_json::to_vec(&header).map_err(error)?;
    if encoded.len() != placeholder.len() {
        return Err("State header changed size.".into());
    }
    output
        .seek(SeekFrom::Start(MAGIC.len() as u64))
        .map_err(error)?;
    output.write_all(&encoded).map_err(error)?;
    output.flush().map_err(error)?;
    output.as_file().sync_all().map_err(error)?;
    output
        .persist(destination)
        .map_err(|e| e.error.to_string())?;
    #[cfg(unix)]
    File::open(parent)
        .and_then(|dir| dir.sync_all())
        .map_err(error)?;
    Ok(())
}
/// A validated private payload, prepared before core creation for startup
/// resume. Invalid input cannot trigger teardown of a half-initialized core.
pub struct PreparedState {
    raw: tempfile::NamedTempFile,
    identity: Identity,
}
impl PreparedState {
    pub fn raw_path(&self) -> &Path {
        self.raw.path()
    }
    pub fn identity(&self) -> &Identity {
        &self.identity
    }
}
pub fn prepare(path: &Path, expected: &Identity) -> Result<PreparedState, String> {
    let mut source = open_state(path)?;
    let header = header(&mut source)?;
    if header.core != expected.core || header.rom != expected.rom {
        return Err("This state belongs to a different game or core.".into());
    }
    if header.renderer != expected.renderer {
        return Err("Resume this state with the video renderer that created it.".into());
    }
    let mut raw = tempfile::NamedTempFile::new().map_err(error)?;
    let (_, digest) = copy_payload(&mut source, &mut raw)?;
    if header.sha256 != digest {
        return Err("The saved state payload is damaged.".into());
    }
    raw.flush().map_err(error)?;
    Ok(PreparedState {
        raw,
        identity: expected.clone(),
    })
}
/// The core sees a private raw file only after identity, renderer and checksum
/// pass. It is deleted immediately after the owner-thread callback returns.
pub fn with_payload<T>(
    path: &Path,
    expected: &Identity,
    load: impl FnOnce(&Path) -> Result<T, String>,
) -> Result<T, String> {
    let prepared = prepare(path, expected)?;
    load(prepared.raw_path())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn identity(renderer: Renderer) -> Identity {
        Identity::new("fixture", &"a".repeat(64), &"b".repeat(64), renderer).unwrap()
    }
    #[test]
    fn envelope_round_trip_and_existing_python_header_compatibility() {
        let root = tempfile::tempdir().unwrap();
        let raw = root.path().join("raw");
        let slot = slot_path(root.path(), 1);
        let payload = b"binary\0state\nwith\xffbytes";
        fs::write(&raw, payload).unwrap();
        let id = identity(Renderer::OpenGl);
        write_from_raw(&slot, &raw, &id).unwrap();
        assert_eq!(inspect(&slot).unwrap(), id);
        assert!(fs::read(&slot).unwrap().starts_with(MAGIC));
        with_payload(&slot, &id, |path| {
            assert_eq!(fs::read(path).unwrap(), payload);
            Ok(())
        })
        .unwrap();
        let metadata = serde_json::json!({"format":1,"core":id.core,"rom":id.rom,"sha256":hash_string(Sha256::digest(payload))});
        let mut bytes = LEGACY_MAGIC.to_vec();
        bytes.extend(serde_json::to_vec(&metadata).unwrap());
        bytes.push(b'\n');
        bytes.extend(payload);
        let legacy = slot.with_extension(LEGACY_EXTENSION);
        fs::write(&legacy, &bytes).unwrap();
        assert_eq!(readable_path(&slot), slot);
        fs::remove_file(&slot).unwrap();
        assert_eq!(readable_path(&slot), legacy);
        let software = identity(Renderer::Software);
        let path = readable_path(&slot);
        assert_eq!(inspect(&path).unwrap().renderer, Renderer::Software);
        with_payload(&path, &software, |_| Ok(())).unwrap();
        write_from_raw(&slot, &raw, &software).unwrap();
        assert_eq!(fs::read(&legacy).unwrap(), bytes);
    }
    #[test]
    fn wrong_game_core_renderer_and_corrupt_payload_never_reach_core() {
        let root = tempfile::tempdir().unwrap();
        let raw = root.path().join("raw");
        let slot = root.path().join("slot");
        fs::write(&raw, [1, 2, 3]).unwrap();
        let id = identity(Renderer::Software);
        write_from_raw(&slot, &raw, &id).unwrap();
        for variant in 0..3 {
            let mut other = id.clone();
            match variant {
                0 => other.rom = "c".repeat(64),
                1 => other.core.push_str("other"),
                _ => other.renderer = Renderer::OpenGl,
            };
            assert!(with_payload(&slot, &other, |_| -> Result<(), String> {
                panic!("Invalid state reached core")
            })
            .is_err());
        }
        let mut bytes = fs::read(&slot).unwrap();
        *bytes.last_mut().unwrap() ^= 1;
        fs::write(&slot, bytes).unwrap();
        assert!(with_payload(&slot, &id, |_| -> Result<(), String> {
            panic!("Corrupt state reached core")
        })
        .is_err());
    }
    #[test]
    fn failed_save_preserves_existing_slot_and_oversized_headers_are_bounded() {
        let root = tempfile::tempdir().unwrap();
        let raw = root.path().join("raw");
        let slot = root.path().join("slot");
        fs::write(&raw, []).unwrap();
        fs::write(&slot, b"previous slot").unwrap();
        assert!(write_from_raw(&slot, &raw, &identity(Renderer::Software)).is_err());
        assert_eq!(fs::read(&slot).unwrap(), b"previous slot");
        let mut bytes = MAGIC.to_vec();
        bytes.extend(vec![b' '; MAX_HEADER + 100]);
        bytes.push(b'\n');
        fs::write(&slot, bytes).unwrap();
        assert!(inspect(&slot).is_err());
        let sparse = File::create(&slot).unwrap();
        sparse
            .set_len(MAX_PAYLOAD + MAX_HEADER as u64 + 100)
            .unwrap();
        assert!(inspect(&slot).is_err());
    }
    #[test]
    fn streaming_hash_matches_known_sha256() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file");
        fs::write(&path, b"abc").unwrap();
        assert_eq!(
            digest_file(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
