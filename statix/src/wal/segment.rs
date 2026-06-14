//! On-disk segment + frame codec for the disk WAL spillway (Phase 11).
//!
//! Layout — purely sequential, zero-copy on the write path (the `bytes::Bytes`
//! JSON payload is written verbatim after a fixed 16-byte header):
//!
//! ```text
//! segment file: [16B segment header] [frame] [frame] ...
//! segment header: [8B magic "STATXWAL"][u32 format_version][u32 reserved]
//! frame:          [u32 payload_len][u32 crc32(payload)][u64 batch_seq][payload bytes]
//! ```
//!
//! Both headers are little-endian and built on the stack (no per-frame heap
//! allocation). A torn tail (SIGKILL / power-loss mid-write) is detected by a
//! short read or a CRC mismatch and truncated during recovery; an interior CRC
//! mismatch marks the frame corrupt.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub const SEGMENT_MAGIC: [u8; 8] = *b"STATXWAL";
pub const FORMAT_VERSION: u32 = 1;
pub const SEGMENT_HEADER_LEN: u64 = 16;
pub const FRAME_HEADER_LEN: usize = 16;

/// Upper bound on a single frame payload — a sanity guard so a corrupt length
/// prefix can never trigger a multi-GiB allocation during recovery.
pub const MAX_FRAME_PAYLOAD: u32 = 16 * 1024 * 1024;

pub const SEGMENT_PREFIX: &str = "seg-";
pub const SEGMENT_SUFFIX: &str = ".wal";

/// `seg-<seq>.wal` path inside `dir`. Seq is zero-padded for lexical ordering.
pub fn segment_path(dir: &Path, seq: u64) -> PathBuf {
    dir.join(format!("{SEGMENT_PREFIX}{seq:020}{SEGMENT_SUFFIX}"))
}

/// Parse the `<seq>` out of a `seg-<seq>.wal` file name, if it matches.
pub fn parse_segment_seq(name: &str) -> Option<u64> {
    name.strip_prefix(SEGMENT_PREFIX)?
        .strip_suffix(SEGMENT_SUFFIX)?
        .parse::<u64>()
        .ok()
}

/// Encode the 16-byte segment header onto the stack.
pub fn encode_segment_header() -> [u8; SEGMENT_HEADER_LEN as usize] {
    let mut buf = [0u8; SEGMENT_HEADER_LEN as usize];
    buf[0..8].copy_from_slice(&SEGMENT_MAGIC);
    buf[8..12].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
    // bytes 12..16 reserved (zero)
    buf
}

/// Validate a segment header read from disk.
pub fn validate_segment_header(buf: &[u8]) -> io::Result<()> {
    if buf.len() < SEGMENT_HEADER_LEN as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "segment header truncated",
        ));
    }
    if buf[0..8] != SEGMENT_MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "bad segment magic",
        ));
    }
    let version = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
    if version != FORMAT_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported WAL format version {version}"),
        ));
    }
    Ok(())
}

/// Encode a frame header (len + crc + seq) onto the stack. The payload is
/// written separately so the caller's `bytes::Bytes` is never copied.
pub fn encode_frame_header(payload: &[u8], batch_seq: u64) -> [u8; FRAME_HEADER_LEN] {
    let crc = crc32fast::hash(payload);
    let mut buf = [0u8; FRAME_HEADER_LEN];
    buf[0..4].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    buf[4..8].copy_from_slice(&crc.to_le_bytes());
    buf[8..16].copy_from_slice(&batch_seq.to_le_bytes());
    buf
}

/// On-disk size of a frame for a given payload length.
pub fn frame_len_on_disk(payload_len: usize) -> u64 {
    FRAME_HEADER_LEN as u64 + payload_len as u64
}

/// A frame decoded during recovery / replay.
#[derive(Debug, Clone)]
pub struct DecodedFrame {
    pub batch_seq: u64,
    pub payload: Vec<u8>,
}

/// Outcome of decoding a single frame at the current reader position.
pub enum FrameRead {
    /// A valid frame plus the absolute offset just past it.
    Frame(DecodedFrame, u64),
    /// Clean end of data (no more bytes / partial header) — torn tail boundary.
    End,
    /// CRC or length sanity failure at this offset — corrupt frame boundary.
    Corrupt,
}

/// Read one frame from `file` starting at `offset` (which must be a frame
/// boundary). Returns the decoded frame and the next offset, or an End/Corrupt
/// marker so recovery can decide whether to truncate.
pub fn read_frame_at(file: &mut File, offset: u64) -> io::Result<FrameRead> {
    file.seek(SeekFrom::Start(offset))?;
    let mut header = [0u8; FRAME_HEADER_LEN];
    match read_full_or_eof(file, &mut header)? {
        ReadStatus::Eof => return Ok(FrameRead::End),
        ReadStatus::Partial => return Ok(FrameRead::End), // torn header → tail
        ReadStatus::Full => {}
    }

    let payload_len = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
    let crc_expected = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    let batch_seq = u64::from_le_bytes([
        header[8], header[9], header[10], header[11], header[12], header[13], header[14],
        header[15],
    ]);

    if payload_len == 0 || payload_len > MAX_FRAME_PAYLOAD {
        // A zero or absurd length is either a torn/zeroed tail or corruption.
        return Ok(FrameRead::Corrupt);
    }

    let mut payload = vec![0u8; payload_len as usize];
    match read_full_or_eof(file, &mut payload)? {
        ReadStatus::Full => {}
        _ => return Ok(FrameRead::End), // torn payload → tail
    }

    if crc32fast::hash(&payload) != crc_expected {
        return Ok(FrameRead::Corrupt);
    }

    let next = offset + frame_len_on_disk(payload_len as usize);
    Ok(FrameRead::Frame(DecodedFrame { batch_seq, payload }, next))
}

enum ReadStatus {
    Full,
    Partial,
    Eof,
}

/// Read exactly `buf.len()` bytes, distinguishing clean EOF from a short read.
fn read_full_or_eof(file: &mut File, buf: &mut [u8]) -> io::Result<ReadStatus> {
    let mut filled = 0;
    while filled < buf.len() {
        match file.read(&mut buf[filled..]) {
            Ok(0) => {
                return Ok(if filled == 0 {
                    ReadStatus::Eof
                } else {
                    ReadStatus::Partial
                });
            }
            Ok(n) => filled += n,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(ReadStatus::Full)
}

/// Release page-cache pages backing `file` (best-effort). Called on a segment
/// once it is fully consumed or rotated out, to avoid cold-backlog page-cache
/// pollution. Failures are non-fatal.
pub fn advise_dontneed(file: &File, len: u64) {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::io::AsRawFd;
        // SAFETY: fd is valid for the lifetime of `file`; POSIX_FADV_DONTNEED
        // has no memory-safety effect, only a page-cache hint.
        unsafe {
            libc::posix_fadvise(
                file.as_raw_fd(),
                0,
                len as libc::off_t,
                libc::POSIX_FADV_DONTNEED,
            );
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (file, len);
    }
}

/// `fdatasync` (data only, skips metadata write-amplification of `fsync`).
pub fn fdatasync(file: &File) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::io::AsRawFd;
        // SAFETY: fd is valid for the lifetime of `file`.
        let rc = unsafe { libc::fdatasync(file.as_raw_fd()) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        file.sync_data()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_dir() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "statix-wal-seg-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn write_seg_with(frames: &[(&[u8], u64)], trailing_garbage: Option<&[u8]>) -> (PathBuf, File) {
        let dir = temp_dir();
        let path = segment_path(&dir, 0);
        let mut f = File::create(&path).unwrap();
        f.write_all(&encode_segment_header()).unwrap();
        for (payload, seq) in frames {
            f.write_all(&encode_frame_header(payload, *seq)).unwrap();
            f.write_all(payload).unwrap();
        }
        if let Some(g) = trailing_garbage {
            f.write_all(g).unwrap();
        }
        f.flush().unwrap();
        let read = File::open(&path).unwrap();
        (path, read)
    }

    #[test]
    fn frame_round_trip_preserves_payload_and_seq() {
        let payload = br#"{"node":"n1","batch":42}"#;
        let (_p, mut f) = write_seg_with(&[(payload, 7)], None);
        match read_frame_at(&mut f, SEGMENT_HEADER_LEN).unwrap() {
            FrameRead::Frame(frame, next) => {
                assert_eq!(frame.payload, payload);
                assert_eq!(frame.batch_seq, 7);
                assert_eq!(next, SEGMENT_HEADER_LEN + frame_len_on_disk(payload.len()));
            }
            _ => panic!("expected a valid frame"),
        }
    }

    #[test]
    fn corrupt_payload_is_detected_by_crc() {
        let payload = b"hello-world-payload";
        let (path, _f) = write_seg_with(&[(payload, 1)], None);
        // Flip a byte in the payload region (past both headers).
        let mut bytes = std::fs::read(&path).unwrap();
        let flip = (SEGMENT_HEADER_LEN as usize) + FRAME_HEADER_LEN + 2;
        bytes[flip] ^= 0xFF;
        std::fs::write(&path, &bytes).unwrap();
        let mut f = File::open(&path).unwrap();
        assert!(matches!(
            read_frame_at(&mut f, SEGMENT_HEADER_LEN).unwrap(),
            FrameRead::Corrupt
        ));
    }

    #[test]
    fn torn_tail_reads_as_end() {
        let payload = b"good-frame";
        // Append a partial frame header (torn write) after one good frame.
        let (_p, mut f) = write_seg_with(&[(payload, 1)], Some(&[0xAA, 0xBB, 0xCC]));
        let next = match read_frame_at(&mut f, SEGMENT_HEADER_LEN).unwrap() {
            FrameRead::Frame(_, next) => next,
            _ => panic!("first frame should be valid"),
        };
        assert!(matches!(
            read_frame_at(&mut f, next).unwrap(),
            FrameRead::End
        ));
    }

    #[test]
    fn segment_header_validation() {
        assert!(validate_segment_header(&encode_segment_header()).is_ok());
        let mut bad = encode_segment_header();
        bad[0] = b'X';
        assert!(validate_segment_header(&bad).is_err());
    }

    #[test]
    fn seq_path_parses_round_trip() {
        let dir = Path::new("/tmp/x");
        let p = segment_path(dir, 12345);
        let name = p.file_name().unwrap().to_str().unwrap();
        assert_eq!(parse_segment_seq(name), Some(12345));
        assert_eq!(parse_segment_seq("not-a-seg"), None);
    }
}
