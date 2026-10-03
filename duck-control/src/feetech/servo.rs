//! Servo driver: serial IO, packet transact, register-level API.
//!
//! Uses raw `libc` calls (`open`/`read`/`write`/`poll`) instead of the `serialport` crate.
//! The `serialport` crate's termios configuration adds ~13ms per transaction on the RK3568
//! UART driver; raw `poll()` with `VMIN=0, VTIME=0` returns in 0.3–0.7ms.

use std::io;
use std::mem;
use std::os::unix::io::RawFd;

use crate::feetech::error::{Result, ServoError};
use crate::feetech::protocol::{self, Instruction, StatusPacket, BROADCAST_ID};
use crate::feetech::registers::{self, Register};

/// Low-latency serial port using raw libc.
struct RawSerial {
    fd: RawFd,
}

impl RawSerial {
    fn open(path: &str, baud: u32, timeout_ms: u64) -> Result<Self> {
        use std::ffi::CString;
        let cpath = CString::new(path).map_err(|_| ServoError::Io(io::Error::new(
            io::ErrorKind::InvalidInput, "path contains null byte",
        )))?;
        // O_RDWR | O_NOCTTY, no O_NONBLOCK — we use poll() for timeouts.
        let fd = unsafe { libc::open(cpath.as_ptr(), libc::O_RDWR | libc::O_NOCTTY) };
        if fd < 0 {
            return Err(ServoError::Io(io::Error::last_os_error()));
        }
        if let Err(e) = Self::configure(fd, baud, timeout_ms) {
            unsafe { libc::close(fd); }
            return Err(ServoError::Io(e));
        }
        Ok(Self { fd })
    }

    fn configure(fd: RawFd, baud: u32, _timeout_ms: u64) -> io::Result<()> {
        unsafe {
            let mut tio: libc::termios = mem::zeroed();
            // Raw mode: no echo, no signals, no line processing.
            tio.c_cflag = libc::CS8 | libc::CLOCAL | libc::CREAD;
            tio.c_iflag = 0;
            tio.c_oflag = 0;
            tio.c_lflag = 0;
            // VMIN=0, VTIME=0: read returns immediately (0 bytes if nothing available).
            // We use poll() for timeouts instead.
            tio.c_cc[libc::VMIN] = 0;
            tio.c_cc[libc::VTIME] = 0;

            let speed = baud_to_speed(baud)?;
            libc::cfsetispeed(&mut tio, speed);
            libc::cfsetospeed(&mut tio, speed);
            libc::tcflush(fd, libc::TCIOFLUSH);
            if libc::tcsetattr(fd, libc::TCSANOW, &tio) != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    fn write_all(&self, data: &[u8]) -> Result<()> {
        let mut written = 0;
        while written < data.len() {
            let n = unsafe {
                libc::write(self.fd, data[written..].as_ptr() as *const _, data.len() - written)
            };
            if n < 0 {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted { continue; }
                return Err(ServoError::Io(err));
            }
            written += n as usize;
        }
        Ok(())
    }

    /// Read a response using poll() for the timeout. Returns bytes read (0 on timeout).
    fn read_with_timeout(&self, buf: &mut [u8], timeout_ms: i32) -> Result<usize> {
        let mut pfd = libc::pollfd {
            fd: self.fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let ret = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
        if ret < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted { return Ok(0); }
            return Err(ServoError::Io(err));
        }
        if ret == 0 {
            return Ok(0); // timeout
        }
        let n = unsafe { libc::read(self.fd, buf.as_mut_ptr() as *mut _, buf.len()) };
        if n < 0 {
            return Err(ServoError::Io(io::Error::last_os_error()));
        }
        Ok(n as usize)
    }

    /// Drain any stale data from the input buffer.
    fn drain_input(&self) {
        let mut discard = [0u8; 256];
        loop {
            let mut pfd = libc::pollfd {
                fd: self.fd,
                events: libc::POLLIN,
                revents: 0,
            };
            let ret = unsafe { libc::poll(&mut pfd, 1, 1) };
            if ret <= 0 { break; }
            let n = unsafe { libc::read(self.fd, discard.as_mut_ptr() as *mut _, discard.len()) };
            if n <= 0 { break; }
        }
    }
}

impl Drop for RawSerial {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd); }
    }
}

fn baud_to_speed(baud: u32) -> io::Result<libc::speed_t> {
    Ok(match baud {
        9600 => libc::B9600,
        19200 => libc::B19200,
        38400 => libc::B38400,
        57600 => libc::B57600,
        115200 => libc::B115200,
        230400 => libc::B230400,
        460800 => libc::B460800,
        500000 => libc::B500000,
        576000 => libc::B576000,
        921600 => libc::B921600,
        1_000_000 => libc::B1000000,
        1_152_000 => libc::B1152000,
        1_500_000 => libc::B1500000,
        2_000_000 => libc::B2000000,
        _ => return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unsupported baud rate: {baud}"),
        )),
    })
}

pub struct Servo {
    port: RawSerial,
    verbose: bool,
    /// Poll timeout for reading a response, ms.
    read_timeout_ms: i32,
}

/// Live feedback from one servo (addresses 56–69).
#[derive(Debug)]
pub struct Feedback {
    pub pos: i64,
    pub vel: i64,
    pub load: i64,
    pub voltage: f64,
    pub temp: i64,
    pub status: u8,
    pub moving: u8,
    pub current: f64,
}

impl Servo {
    pub fn connect(path: &str, baud: u32, timeout_ms: u64, _discard_echo: bool, verbose: bool) -> Result<Self> {
        let port = RawSerial::open(path, baud, timeout_ms)?;
        Ok(Servo {
            port,
            verbose,
            read_timeout_ms: timeout_ms.max(1).min(1000) as i32,
        })
    }

    // ── low-level ──────────────────────────────────────────────────────────

    fn transact(&mut self, id: u8, inst: Instruction, params: &[u8], expect_reply: bool) -> Result<StatusPacket> {
        let pkt = protocol::build_packet(id, inst, params);

        if self.verbose {
            eprintln!("  >> tx: {:02X?}", pkt);
        }

        self.port.write_all(&pkt)?;

        if !expect_reply || id == BROADCAST_ID {
            return Ok(StatusPacket { id, error: 0, params: Vec::new() });
        }

        let raw = self.recv_packet()?;
        if self.verbose {
            eprintln!("  << rx: {:02X?}", raw);
        }
        let status = protocol::parse_status(&raw)?;
        if status.id != id {
            return Err(ServoError::Malformed("reply id mismatch"));
        }
        if status.error != 0 {
            return Err(ServoError::ServoStatus(
                status.error,
                protocol::describe_status(status.error),
            ));
        }
        Ok(status)
    }

    fn recv_packet(&mut self) -> Result<Vec<u8>> {
        // Read a generous chunk via poll()+read. The poll timeout gates how long we
        // wait for the servo to start responding.
        let mut buf = [0u8; 64];
        let n = self.port.read_with_timeout(&mut buf, self.read_timeout_ms).map_err(|e| match e {
            ServoError::Io(ref ie) if ie.kind() == std::io::ErrorKind::TimedOut => ServoError::Timeout,
            _ => e,
        })?;
        if n == 0 {
            return Err(ServoError::Timeout);
        }
        if n < 2 {
            return Err(ServoError::Timeout);
        }

        // Find the 0xFF 0xFF header.
        let mut header_pos = None;
        for i in 0..n - 1 {
            if buf[i] == 0xFF && buf[i + 1] == 0xFF {
                header_pos = Some(i);
                break;
            }
        }
        let h = header_pos.ok_or(ServoError::Malformed("no header found"))?;

        // We need header(2) + length_field(2) to know how much more to read.
        if h + 4 > n {
            return Err(ServoError::Malformed("packet truncated"));
        }
        let length = buf[h + 3] as usize;
        let total = 4 + length; // header(2) + length(2) + payload(length)

        // Read any remaining bytes if the first read didn't get them all.
        let mut full = buf[h..n].to_vec();
        while full.len() < total {
            let mut extra = [0u8; 32];
            let m = self.port.read_with_timeout(&mut extra, self.read_timeout_ms)?;
            if m == 0 {
                return Err(ServoError::Timeout);
            }
            full.extend_from_slice(&extra[..m]);
        }
        full.truncate(total);
        Ok(full)
    }

    // ── instruction-level API ──────────────────────────────────────────────

    pub fn ping(&mut self, id: u8) -> Result<bool> {
        match self.transact(id, Instruction::Ping, &[], true) {
            Ok(_) => Ok(true),
            Err(ServoError::Timeout) => Ok(false),
            Err(e) => Err(e),
        }
    }

    pub fn read(&mut self, id: u8, addr: u8, len: u8) -> Result<Vec<u8>> {
        let resp = self.transact(id, Instruction::Read, &[addr, len], true)?;
        Ok(resp.params)
    }

    pub fn write(&mut self, id: u8, addr: u8, data: &[u8]) -> Result<()> {
        let mut params = Vec::with_capacity(data.len() + 1);
        params.push(addr);
        params.extend_from_slice(data);
        self.transact(id, Instruction::Write, &params, true)?;
        Ok(())
    }

    pub fn reg_write(&mut self, id: u8, addr: u8, data: &[u8]) -> Result<()> {
        let mut params = Vec::with_capacity(data.len() + 1);
        params.push(addr);
        params.extend_from_slice(data);
        self.transact(id, Instruction::RegWrite, &params, true)?;
        Ok(())
    }

    pub fn action(&mut self) -> Result<()> {
        self.transact(BROADCAST_ID, Instruction::Action, &[], false)?;
        Ok(())
    }

    /// SyncWrite (0x83): one instruction, per-servo values for one register.
    pub fn sync_write(&mut self, ids: &[u8], reg: &Register, values: &[i64]) -> Result<()> {
        if ids.is_empty() || ids.len() != values.len() {
            return Err(ServoError::InvalidParam("sync_write: ids and values must match".into()));
        }
        for (i, &id) in ids.iter().enumerate() {
            if values[i] < reg.min || values[i] > reg.max {
                return Err(ServoError::InvalidParam(format!(
                    "value {} for id {} out of {}..{}",
                    values[i], id, reg.min, reg.max
                )));
            }
        }

        let data_len = reg.size as usize;
        let mut params = Vec::with_capacity(2 + ids.len() * (1 + data_len));
        params.push(reg.addr);
        params.push(data_len as u8);
        for (&id, &value) in ids.iter().zip(values.iter()) {
            let val = registers::encode_value(value, reg.sign_bit);
            params.push(id);
            if data_len == 1 {
                params.push(val as u8);
            } else {
                params.extend_from_slice(&val.to_le_bytes());
            }
        }
        self.transact(BROADCAST_ID, Instruction::SyncWrite, &params, false)?;
        Ok(())
    }

    /// Factory reset (0x06): EPROM returns to defaults.
    pub fn factory_reset(&mut self, id: u8) -> Result<()> {
        self.transact(id, Instruction::Reset, &[], true)?;
        Ok(())
    }

    // ── register-level API ─────────────────────────────────────────────────

    pub fn read_reg(&mut self, id: u8, reg: &Register) -> Result<i64> {
        let raw = self.read(id, reg.addr, reg.size)?;
        if raw.len() < reg.size as usize {
            return Err(ServoError::Malformed("read data too short"));
        }
        let val = if reg.size == 1 {
            raw[0] as u16
        } else {
            u16::from_le_bytes([raw[0], raw[1]])
        };
        Ok(registers::decode_value(val, reg.sign_bit))
    }

    pub fn write_reg(&mut self, id: u8, reg: &Register, value: i64) -> Result<()> {
        if reg.access == registers::Access::ReadOnly {
            return Err(ServoError::InvalidParam(format!(
                "register {} is read-only", reg.key
            )));
        }
        if value < reg.min || value > reg.max {
            return Err(ServoError::InvalidParam(format!(
                "value {} out of {}..{} for {}", value, reg.min, reg.max, reg.key
            )));
        }
        let val = registers::encode_value(value, reg.sign_bit);
        let bytes = if reg.size == 1 {
            vec![val as u8]
        } else {
            val.to_le_bytes().to_vec()
        };
        self.write(id, reg.addr, &bytes)
    }

    /// Write EPROM register with lock/unlock cycle for persistence.
    pub fn save_reg(&mut self, id: u8, reg: &Register, value: i64) -> Result<()> {
        if reg.area != registers::Area::Eprom {
            return self.write_reg(id, reg, value);
        }
        let lock = registers::find_register("lock").expect("lock register exists");
        self.write_reg(id, lock, 0)?; // unlock
        let result = self.write_reg(id, reg, value);
        let relock = self.write_reg(id, lock, 1);
        result.and(relock)
    }

    // ── bulk feedback ──────────────────────────────────────────────────────

    /// Read all live feedback (addresses 56–70, 15 bytes) in one transaction.
    pub fn read_feedback(&mut self, id: u8) -> Result<Feedback> {
        let raw = self.read(id, 56, 15)?;
        if raw.len() < 15 {
            return Err(ServoError::Malformed("feedback data too short"));
        }
        let u16le = |i: usize| u16::from_le_bytes([raw[i], raw[i + 1]]);
        Ok(Feedback {
            pos: registers::decode_value(u16le(0), Some(15)),
            vel: registers::decode_value(u16le(2), Some(15)),
            load: registers::decode_value(u16le(4), Some(10)),
            voltage: raw[6] as f64 / 10.0,
            temp: raw[7] as i64,
            status: raw[9],
            moving: raw[10],
            current: registers::decode_value(u16le(13), Some(15)) as f64 * 6.5,
        })
    }
}
