//! FT-SCS protocol: packet assembly, parsing, checksum.

use crate::feetech::error::{Result, ServoError};

pub const HEADER: [u8; 2] = [0xFF, 0xFF];
pub const BROADCAST_ID: u8 = 0xFE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Instruction {
    Ping = 0x01,
    Read = 0x02,
    Write = 0x03,
    RegWrite = 0x04,
    Action = 0x05,
    Reset = 0x06,
    SyncWrite = 0x83,
}

/// Sum all bytes, invert, keep low 8 bits.
pub fn checksum(bytes: &[u8]) -> u8 {
    let sum = bytes.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
    !sum
}

pub fn build_packet(id: u8, inst: Instruction, params: &[u8]) -> Vec<u8> {
    let length = (params.len() + 2) as u8;
    let mut pkt = Vec::with_capacity(params.len() + 6);
    pkt.extend_from_slice(&HEADER);
    pkt.push(id);
    pkt.push(length);
    pkt.push(inst as u8);
    pkt.extend_from_slice(params);
    pkt.push(checksum(&pkt[2..]));
    pkt
}

pub struct StatusPacket {
    pub id: u8,
    pub error: u8,
    pub params: Vec<u8>,
}

pub fn parse_status(buf: &[u8]) -> Result<StatusPacket> {
    if buf.len() < 6 {
        return Err(ServoError::Malformed("frame shorter than 6 bytes"));
    }
    if buf[0] != HEADER[0] || buf[1] != HEADER[1] {
        return Err(ServoError::Malformed("header is not 0xFF 0xFF"));
    }
    let id = buf[2];
    let length = buf[3] as usize;
    if buf.len() != length + 4 {
        return Err(ServoError::Malformed("frame length disagrees with Length field"));
    }
    let expect = checksum(&buf[2..buf.len() - 1]);
    if expect != buf[buf.len() - 1] {
        return Err(ServoError::Checksum);
    }
    Ok(StatusPacket {
        id,
        error: buf[4],
        params: buf[5..buf.len() - 1].to_vec(),
    })
}

pub fn describe_status(status: u8) -> String {
    if status == 0 {
        return "ok".to_string();
    }
    let mut problems: Vec<&str> = Vec::new();
    let bits = [
        (1 << 0, "voltage"),
        (1 << 1, "encoder"),
        (1 << 2, "overheat"),
        (1 << 3, "overcurrent"),
    ];
    for (bit, name) in bits {
        if status & bit != 0 {
            problems.push(name);
        }
    }
    problems.join(" + ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_ping_packet() {
        let pkt = build_packet(1, Instruction::Ping, &[]);
        assert_eq!(pkt, vec![0xFF, 0xFF, 0x01, 0x02, 0x01, 0xFB]);
    }

    #[test]
    fn checksum_wraps() {
        assert_eq!(checksum(&[0xFF, 0xFF]), !0xFEu8);
    }

    #[test]
    fn parse_status_roundtrip() {
        let raw = [0xFF, 0xFF, 0x01, 0x04, 0x00, 0x00, 0x08, 0xF2];
        let pkt = parse_status(&raw).unwrap();
        assert_eq!(pkt.id, 1);
        assert_eq!(pkt.error, 0);
        assert_eq!(pkt.params, vec![0x00, 0x08]);
    }

    #[test]
    fn parse_bad_checksum() {
        let raw = [0xFF, 0xFF, 0x01, 0x04, 0x00, 0x00, 0x08, 0x00];
        assert!(matches!(parse_status(&raw), Err(ServoError::Checksum)));
    }
}
