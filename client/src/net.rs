use std::io::{self, Read, Write};
use std::mem::size_of;
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use common::SpawnRecord;

const RECORD_SIZE: usize = size_of::<SpawnRecord>();

pub struct ServerConnection {
    stream: TcpStream,
    buf: Vec<u8>,
}

impl ServerConnection {
    /// Connect with the default 100 ms read timeout.
    pub fn connect(addr: SocketAddr) -> io::Result<Self> {
        Self::connect_with_timeout(addr, 100)
    }

    pub fn connect_with_timeout(addr: SocketAddr, read_timeout_ms: u64) -> io::Result<Self> {
        let stream = TcpStream::connect(addr)?;
        stream.set_read_timeout(Some(Duration::from_millis(read_timeout_ms)))?;
        Ok(Self { stream, buf: Vec::new() })
    }

    /// Send a 4-byte LE request bitmask and receive all SpawnRecords in the response.
    /// Handles TCP fragmentation by accumulating bytes until the full payload arrives.
    pub fn tick(&mut self, request: i32) -> io::Result<Vec<SpawnRecord>> {
        self.stream.write_all(&request.to_le_bytes())?;

        let mut count_buf = [0u8; 4];
        read_all(&mut self.stream, &mut count_buf)?;
        let count = i32::from_le_bytes(count_buf) as usize;
        if count == 0 {
            return Ok(Vec::new());
        }

        let total = count * RECORD_SIZE;
        self.buf.resize(total, 0);
        read_all(&mut self.stream, &mut self.buf)?;

        let records = self.buf[..total]
            .chunks_exact(RECORD_SIZE)
            .map(record_from_bytes)
            .collect();
        Ok(records)
    }

    pub fn disconnect(self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

/// Read exactly `buf.len()` bytes, retrying transparently on partial reads and
/// timeout interrupts (the server may split a large payload across multiple TCP segments).
/// A genuine EOF or socket error terminates immediately.
fn read_all(stream: &mut TcpStream, buf: &mut [u8]) -> io::Result<()> {
    let mut filled = 0;
    while filled < buf.len() {
        match stream.read(&mut buf[filled..]) {
            Ok(0) => {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "connection closed"))
            }
            Ok(n) => filled += n,
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock
                    || e.kind() == io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn record_from_bytes(chunk: &[u8]) -> SpawnRecord {
    debug_assert_eq!(chunk.len(), RECORD_SIZE);
    let mut rec = SpawnRecord::zeroed();
    // SAFETY: SpawnRecord is #[repr(C, packed)] with size == RECORD_SIZE.
    // chunk is exactly RECORD_SIZE bytes of valid wire data with no alignment requirements.
    unsafe {
        std::ptr::copy_nonoverlapping(
            chunk.as_ptr(),
            &mut rec as *mut SpawnRecord as *mut u8,
            RECORD_SIZE,
        );
    }
    rec
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_from_bytes_round_trips() {
        let mut original = SpawnRecord::zeroed();
        original.id = 42;
        original.flags = 0xFD;
        original.name[0] = b'T';
        original.name[1] = b'e';
        original.name[2] = b's';
        original.name[3] = b't';

        let bytes: Vec<u8> = original.as_bytes().to_vec();
        let decoded = record_from_bytes(&bytes);

        assert_eq!({ decoded.id }, 42u32);
        assert_eq!({ decoded.flags }, 0xFDu32);
        assert_eq!(decoded.name[0], b'T');
    }
}