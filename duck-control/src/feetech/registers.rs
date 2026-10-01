//! HD1910 memory table and sign-magnitude encoding.

/// Register access permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    ReadOnly,
    ReadWrite,
}

/// Memory table region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    Version,
    Eprom,
    SramControl,
    SramFeedback,
    Factory,
}

pub struct Register {
    pub addr: u8,
    pub key: &'static str,
    pub size: u8,
    pub access: Access,
    pub area: Area,
    pub min: i64,
    pub max: i64,
    /// Sign-magnitude sign-bit position: None = unsigned, Some(15) = BIT15 direction,
    /// Some(10) = BIT10 direction (load).
    pub sign_bit: Option<u8>,
}

/// Full HD1910 memory table (HLS_2).
pub const REGISTERS: &[Register] = &[
    // Version (read-only)
    Register { addr: 0,  key: "fw-major",    size: 1, access: Access::ReadOnly,  area: Area::Version,      min: 0, max: 255,   sign_bit: None },
    Register { addr: 1,  key: "fw-minor",    size: 1, access: Access::ReadOnly,  area: Area::Version,      min: 0, max: 255,   sign_bit: None },
    Register { addr: 2,  key: "endian",      size: 1, access: Access::ReadOnly,  area: Area::Version,      min: 0, max: 255,   sign_bit: None },
    Register { addr: 3,  key: "servo-major", size: 1, access: Access::ReadOnly,  area: Area::Version,      min: 0, max: 255,   sign_bit: None },
    Register { addr: 4,  key: "servo-minor", size: 1, access: Access::ReadOnly,  area: Area::Version,      min: 0, max: 255,   sign_bit: None },

    // EPROM (read-write, persists with lock=0)
    Register { addr: 5,  key: "id",              size: 1, access: Access::ReadWrite, area: Area::Eprom, min: 0, max: 253,   sign_bit: None },
    Register { addr: 6,  key: "baud",            size: 1, access: Access::ReadWrite, area: Area::Eprom, min: 0, max: 7,     sign_bit: None },
    Register { addr: 7,  key: "id2",             size: 1, access: Access::ReadWrite, area: Area::Eprom, min: 0, max: 253,   sign_bit: None },
    Register { addr: 8,  key: "resp-level",      size: 1, access: Access::ReadWrite, area: Area::Eprom, min: 0, max: 1,     sign_bit: None },
    Register { addr: 9,  key: "min-angle",       size: 2, access: Access::ReadWrite, area: Area::Eprom, min: 0, max: 4094,  sign_bit: None },
    Register { addr: 11, key: "max-angle",       size: 2, access: Access::ReadWrite, area: Area::Eprom, min: 1, max: 4095,  sign_bit: None },
    Register { addr: 13, key: "max-temp",        size: 1, access: Access::ReadWrite, area: Area::Eprom, min: 0, max: 100,   sign_bit: None },
    Register { addr: 14, key: "max-voltage",     size: 1, access: Access::ReadWrite, area: Area::Eprom, min: 0, max: 254,   sign_bit: None },
    Register { addr: 15, key: "min-voltage",     size: 1, access: Access::ReadWrite, area: Area::Eprom, min: 0, max: 254,   sign_bit: None },
    Register { addr: 16, key: "max-torque",      size: 2, access: Access::ReadWrite, area: Area::Eprom, min: 0, max: 1000,  sign_bit: None },
    Register { addr: 18, key: "phase",           size: 1, access: Access::ReadWrite, area: Area::Eprom, min: 0, max: 254,   sign_bit: None },
    Register { addr: 19, key: "unload-cond",     size: 1, access: Access::ReadWrite, area: Area::Eprom, min: 0, max: 254,   sign_bit: None },
    Register { addr: 20, key: "led-alarm",       size: 1, access: Access::ReadWrite, area: Area::Eprom, min: 0, max: 254,   sign_bit: None },
    Register { addr: 21, key: "pos-p",           size: 1, access: Access::ReadWrite, area: Area::Eprom, min: 0, max: 254,   sign_bit: None },
    Register { addr: 22, key: "pos-d",           size: 1, access: Access::ReadWrite, area: Area::Eprom, min: 0, max: 254,   sign_bit: None },
    Register { addr: 23, key: "pos-i",           size: 1, access: Access::ReadWrite, area: Area::Eprom, min: 0, max: 254,   sign_bit: None },
    Register { addr: 24, key: "min-start-force", size: 1, access: Access::ReadWrite, area: Area::Eprom, min: 0, max: 254,   sign_bit: None },
    Register { addr: 25, key: "integral-limit",  size: 1, access: Access::ReadWrite, area: Area::Eprom, min: 0, max: 254,   sign_bit: None },
    Register { addr: 26, key: "deadband-cw",     size: 1, access: Access::ReadWrite, area: Area::Eprom, min: 0, max: 16,    sign_bit: None },
    Register { addr: 27, key: "deadband-ccw",    size: 1, access: Access::ReadWrite, area: Area::Eprom, min: 0, max: 16,    sign_bit: None },
    Register { addr: 28, key: "protect-current", size: 2, access: Access::ReadWrite, area: Area::Eprom, min: 0, max: 2047,  sign_bit: None },
    Register { addr: 30, key: "angle-resolution",size: 1, access: Access::ReadWrite, area: Area::Eprom, min: 1, max: 128,   sign_bit: None },
    Register { addr: 31, key: "pos-offset",      size: 2, access: Access::ReadWrite, area: Area::Eprom, min: -4095, max: 4095, sign_bit: Some(15) },
    Register { addr: 33, key: "mode",            size: 1, access: Access::ReadWrite, area: Area::Eprom, min: 0, max: 4,     sign_bit: None },
    Register { addr: 34, key: "cur-p",           size: 1, access: Access::ReadWrite, area: Area::Eprom, min: 0, max: 254,   sign_bit: None },
    Register { addr: 35, key: "cur-i",           size: 1, access: Access::ReadWrite, area: Area::Eprom, min: 0, max: 254,   sign_bit: None },
    Register { addr: 36, key: "reserved-36",     size: 1, access: Access::ReadWrite, area: Area::Eprom, min: 0, max: 255,   sign_bit: None },
    Register { addr: 37, key: "vel-p",           size: 1, access: Access::ReadWrite, area: Area::Eprom, min: 0, max: 254,   sign_bit: None },
    Register { addr: 38, key: "overcur-time",    size: 1, access: Access::ReadWrite, area: Area::Eprom, min: 0, max: 254,   sign_bit: None },
    Register { addr: 39, key: "vel-i",           size: 1, access: Access::ReadWrite, area: Area::Eprom, min: 0, max: 254,   sign_bit: None },

    // SRAM control (read-write, volatile)
    Register { addr: 40, key: "torque-switch", size: 1, access: Access::ReadWrite, area: Area::SramControl, min: 0, max: 2,     sign_bit: None },
    Register { addr: 41, key: "acc",           size: 1, access: Access::ReadWrite, area: Area::SramControl, min: 0, max: 254, sign_bit: None },
    Register { addr: 42, key: "goal-pos",      size: 2, access: Access::ReadWrite, area: Area::SramControl, min: -32767, max: 32767, sign_bit: Some(15) },
    Register { addr: 44, key: "goal-current",  size: 2, access: Access::ReadWrite, area: Area::SramControl, min: -2047, max: 2047, sign_bit: Some(15) },
    Register { addr: 46, key: "speed",         size: 2, access: Access::ReadWrite, area: Area::SramControl, min: -32767, max: 32767, sign_bit: Some(15) },
    Register { addr: 48, key: "torque-limit",  size: 2, access: Access::ReadWrite, area: Area::SramControl, min: 0, max: 1000,  sign_bit: None },
    Register { addr: 50, key: "kp",            size: 1, access: Access::ReadWrite, area: Area::SramControl, min: 0, max: 254,   sign_bit: None },
    Register { addr: 51, key: "kd",            size: 1, access: Access::ReadWrite, area: Area::SramControl, min: 0, max: 254,   sign_bit: None },
    Register { addr: 52, key: "ki",            size: 1, access: Access::ReadWrite, area: Area::SramControl, min: 0, max: 254,   sign_bit: None },
    Register { addr: 53, key: "reserved-53",   size: 1, access: Access::ReadWrite, area: Area::SramControl, min: 0, max: 255,   sign_bit: None },
    Register { addr: 54, key: "reserved-54",   size: 1, access: Access::ReadWrite, area: Area::SramControl, min: 0, max: 255,   sign_bit: None },
    Register { addr: 55, key: "lock",          size: 1, access: Access::ReadWrite, area: Area::SramControl, min: 0, max: 1,     sign_bit: None },

    // SRAM feedback (read-only)
    Register { addr: 56, key: "pos",            size: 2, access: Access::ReadOnly, area: Area::SramFeedback, min: -32767, max: 32767, sign_bit: Some(15) },
    Register { addr: 58, key: "vel",            size: 2, access: Access::ReadOnly, area: Area::SramFeedback, min: -32767, max: 32767, sign_bit: Some(15) },
    Register { addr: 60, key: "load",           size: 2, access: Access::ReadOnly, area: Area::SramFeedback, min: -1000, max: 1000,   sign_bit: Some(10) },
    Register { addr: 62, key: "voltage",        size: 1, access: Access::ReadOnly, area: Area::SramFeedback, min: 0, max: 255,   sign_bit: None },
    Register { addr: 63, key: "temp",           size: 1, access: Access::ReadOnly, area: Area::SramFeedback, min: 0, max: 255,   sign_bit: None },
    Register { addr: 64, key: "async-flag",     size: 1, access: Access::ReadOnly, area: Area::SramFeedback, min: 0, max: 255,   sign_bit: None },
    Register { addr: 65, key: "status",         size: 1, access: Access::ReadOnly, area: Area::SramFeedback, min: 0, max: 255,   sign_bit: None },
    Register { addr: 66, key: "moving",         size: 1, access: Access::ReadOnly, area: Area::SramFeedback, min: 0, max: 255,   sign_bit: None },
    Register { addr: 67, key: "goal-pos-fb",    size: 2, access: Access::ReadOnly, area: Area::SramFeedback, min: 0, max: 65535, sign_bit: None },
    Register { addr: 69, key: "current",        size: 2, access: Access::ReadOnly, area: Area::SramFeedback, min: -32767, max: 32767, sign_bit: Some(15) },
    Register { addr: 71, key: "reserved-71",    size: 2, access: Access::ReadOnly, area: Area::SramFeedback, min: 0, max: 65535, sign_bit: None },
    Register { addr: 73, key: "current-offset", size: 2, access: Access::ReadOnly, area: Area::SramFeedback, min: 0, max: 65535, sign_bit: None },

    // Factory (read-only)
    Register { addr: 77, key: "v-fk",            size: 1, access: Access::ReadOnly, area: Area::Factory, min: 0, max: 255, sign_bit: None },
    Register { addr: 78, key: "v-kgi",           size: 1, access: Access::ReadOnly, area: Area::Factory, min: 0, max: 255, sign_bit: None },
    Register { addr: 79, key: "p-fk",            size: 1, access: Access::ReadOnly, area: Area::Factory, min: 0, max: 255, sign_bit: None },
    Register { addr: 80, key: "move-speed-th",   size: 1, access: Access::ReadOnly, area: Area::Factory, min: 0, max: 255, sign_bit: None },
    Register { addr: 81, key: "dts",             size: 1, access: Access::ReadOnly, area: Area::Factory, min: 0, max: 255, sign_bit: None },
    Register { addr: 82, key: "e-fk",            size: 1, access: Access::ReadOnly, area: Area::Factory, min: 0, max: 255, sign_bit: None },
    Register { addr: 83, key: "vk",              size: 1, access: Access::ReadOnly, area: Area::Factory, min: 0, max: 255, sign_bit: None },
    Register { addr: 84, key: "max-speed-limit", size: 1, access: Access::ReadOnly, area: Area::Factory, min: 0, max: 255, sign_bit: None },
    Register { addr: 85, key: "acc-limit",       size: 1, access: Access::ReadOnly, area: Area::Factory, min: 0, max: 255, sign_bit: None },
    Register { addr: 86, key: "acc-multi",       size: 1, access: Access::ReadOnly, area: Area::Factory, min: 0, max: 255, sign_bit: None },
];

/// Baud rate index table: register 6 value 0–7 → actual baud rate.
pub const BAUD_RATES: [u32; 8] = [
    1_000_000, 500_000, 250_000, 128_000, 115_200, 76_800, 57_600, 38_400,
];

/// Look up a register by key name or decimal/hex address.
pub fn find_register(name_or_addr: &str) -> Option<&'static Register> {
    let key = name_or_addr.to_lowercase();
    let addr: Option<u8> = if let Some(hex) = key.strip_prefix("0x") {
        u8::from_str_radix(hex, 16).ok()
    } else {
        key.parse::<u8>().ok()
    };
    if let Some(a) = addr {
        return REGISTERS.iter().find(|r| r.addr == a);
    }
    REGISTERS.iter().find(|r| r.key == key)
}

/// Sign-magnitude decode: raw u16 → signed integer.
pub fn decode_value(raw: u16, sign_bit: Option<u8>) -> i64 {
    match sign_bit {
        Some(bit) => {
            let sign_mask = 1u16 << bit;
            let mag_mask = sign_mask - 1;
            let mag = (raw & mag_mask) as i64;
            if raw & sign_mask != 0 { -mag } else { mag }
        }
        None => raw as i64,
    }
}

/// Sign-magnitude encode: signed integer → raw u16.
pub fn encode_value(value: i64, sign_bit: Option<u8>) -> u16 {
    match sign_bit {
        Some(bit) => {
            let sign_mask = 1u16 << bit;
            let mag = value.unsigned_abs() as u16;
            if value < 0 { mag | sign_mask } else { mag }
        }
        None => value as u16,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_by_key_and_addr() {
        assert_eq!(find_register("kp").unwrap().addr, 50);
        assert_eq!(find_register("KP").unwrap().addr, 50);
        assert_eq!(find_register("50").unwrap().key, "kp");
        assert_eq!(find_register("0x32").unwrap().key, "kp");
        assert!(find_register("not-exist").is_none());
    }

    #[test]
    fn sign_magnitude_roundtrip() {
        let raw = encode_value(-100, Some(15));
        assert_eq!(raw, 0x8064);
        assert_eq!(decode_value(raw, Some(15)), -100);
        assert_eq!(decode_value(1000, None), 1000);
        let raw = encode_value(-500, Some(10));
        assert_eq!(decode_value(raw, Some(10)), -500);
    }
}
