//! Servo driver: serial IO, packet transact, register-level API.

use std::io::{Read, Write};
use std::time::Duration;

use serialport::SerialPort;

use crate::feetech::error::{Result, ServoError};
use crate::feetech::protocol::{self, Instruction, StatusPacket, BROADCAST_ID};
use crate::feetech::registers::{self, Register};

pub struct Servo {
    port: Box<dyn SerialPort>,
    discard_echo: bool,
    verbose: bool,
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
    pub fn connect(path: &str, baud: u32, timeout_ms: u64, discard_echo: bool, verbose: bool) -> Result<Self> {
        let port = serialport::new(path, baud)
            .timeout(Duration::from_millis(timeout_ms))
            .data_bits(serialport::DataBits::Eight)
            .parity(serialport::Parity::None)
            .stop_bits(serialport::StopBits::One)
            .open()?;
        Ok(Servo { port, discard_echo, verbose })
    }

    // ── low-level ──────────────────────────────────────────────────────────

    fn transact(&mut self, id: u8, inst: Instruction, params: &[u8], expect_reply: bool) -> Result<StatusPacket> {
        let pkt = protocol::build_packet(id, inst, params);
        let _ = self.port.clear(serialport::ClearBuffer::Input);

        if self.verbose {
            eprintln!("  >> tx: {:02X?}", pkt);
        }

        self.port.write_all(&pkt)?;
        self.port.flush()?;

        if self.discard_echo {
            let mut echo = vec![0u8; pkt.len()];
            let _ = self.port.read_exact(&mut echo);
        }

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
        let mut window = [0u8; 2];
        let mut found = false;
        for _ in 0..64 {
            let mut b = [0u8; 1];
            self.port.read_exact(&mut b).map_err(|e| match e.kind() {
                std::io::ErrorKind::TimedOut => ServoError::Timeout,
                _ => ServoError::Io(e),
            })?;
            window[0] = window[1];
            window[1] = b[0];
            if window == protocol::HEADER {
                found = true;
                break;
            }
        }
        if !found {
            return Err(ServoError::Malformed("no header found"));
        }

        let mut head = [0u8; 2];
        self.port.read_exact(&mut head).map_err(|e| match e.kind() {
            std::io::ErrorKind::TimedOut => ServoError::Timeout,
            _ => ServoError::Io(e),
        })?;
        let length = head[1] as usize;

        let mut rest = vec![0u8; length];
        self.port.read_exact(&mut rest).map_err(|e| match e.kind() {
            std::io::ErrorKind::TimedOut => ServoError::Timeout,
            _ => ServoError::Io(e),
        })?;

        let mut full = Vec::with_capacity(4 + length);
        full.extend_from_slice(&protocol::HEADER);
        full.extend_from_slice(&head);
        full.extend_from_slice(&rest);
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
