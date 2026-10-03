//! Body IMU over I²C: the LSM6DSV16X, the same part the `imu_to_dxl` v2 board carried on the
//! Dynamixel bus. Now wired directly to the Radxa's I²C3 (header pins 3/5: GPIO1_A0 SDA,
//! GPIO1_A1 SCL). The board's SFLP block ships a game-rotation quaternion and estimates its
//! own gyro bias — no host-side fusion to run.
//!
//! The FIFO delivers 6 bytes per sample (Qx, Qy, Qz as IEEE half-precision); the raw gyro
//! comes from `OUT_GYRO_*` so the existing [`crate::imu::SflpDecoder`] can keep its spike
//! rejection. The two reads together form the same 12-byte block the Dynamixel bus used to
//! carry, so the decoder is reused unchanged.

use std::thread;
use std::time::Duration;

use i2cdev::core::*;
use i2cdev::linux::LinuxI2CDevice;

use crate::imu::{ImuData, SflpDecoder};
use crate::io::ImuStale;

/// 7-bit I²C address, SDO/SA0 tied high.
const I2C_ADDR: u16 = 0x6B;

/// WHO_AM_I response.
const WHO_AM_I_REG: u8 = 0x0F;
const WHO_AM_I_OK: u8 = 0x70;

// Main bank registers.
const FUNC_CFG_ACCESS: u8 = 0x01;
const FIFO_CTRL1: u8 = 0x07;
const FIFO_CTRL3: u8 = 0x09;
const FIFO_CTRL4: u8 = 0x0A;
const FIFO_STATUS1: u8 = 0x1B;
const FIFO_STATUS2: u8 = 0x1C;
const CTRL1: u8 = 0x10;
const CTRL2: u8 = 0x11;
const CTRL3: u8 = 0x12;
const CTRL6: u8 = 0x15;
const CTRL8: u8 = 0x17;
const OUT_GYRO_X_L: u8 = 0x22;
const FIFO_DATA_OUT_TAG: u8 = 0x78;

// Embedded function bank registers (accessed via FUNC_CFG_ACCESS bit 7).
const EMB_FUNC_EN_A: u8 = 0x04;
const EMB_FUNC_FIFO_EN_A: u8 = 0x44;
const SFLP_ODR: u8 = 0x5E;

/// SFLP game rotation vector FIFO tag (verified on-chip; the ST C driver header
/// has this wrong — it says 0x13 but the chip emits 0x17 for game rotation).
const SFLP_GAME_TAG: u8 = 0x17;

/// One FIFO entry: 1 tag + 6 data bytes.
const FIFO_ENTRY_LEN: usize = 7;

/// How long to wait for the SFLP algorithm to bootstrap after enablement.
const SFLP_STARTUP: Duration = Duration::from_millis(300);

/// Sensor sample rate — both accel/gyro and SFLP.
const ODR_HZ: u8 = 0x04; // 30 Hz

/// Encode an f32 as IEEE 754 half-precision (2 bytes, little-endian).
fn f32_to_f16(v: f64) -> [u8; 2] {
    let bits = v as f32;
    let f = bits.to_bits();
    let sign = ((f >> 16) & 0x8000) as u16;
    let exp = ((f >> 23) & 0xFF) as i32;
    let mant = f & 0x007F_FFFF;

    let h = if exp == 0 && mant == 0 {
        sign // zero
    } else if exp == 0xFF {
        // inf / NaN
        sign | 0x7C00 | if mant != 0 { 0x0200 } else { 0 }
    } else {
        let new_exp = exp - 127 + 15;
        if new_exp >= 31 {
            sign | 0x7C00 // overflow → inf
        } else if new_exp <= 0 {
            // subnormal
            let shift = 14 - new_exp;
            sign | (((mant | 0x0080_0000) >> (shift + 13)) as u16)
        } else {
            sign | ((new_exp as u16) << 10) | ((mant >> 13) as u16)
        }
    };
    h.to_le_bytes()
}

pub struct Lsm6dsv16x {
    dev: LinuxI2CDevice,
    decoder: SflpDecoder,
    stale: ImuStale,
}

impl Lsm6dsv16x {
    /// Open the I²C bus, verify the chip, and start SFLP game rotation output.
    pub fn open(bus: &str) -> Result<Self, String> {
        let mut dev = LinuxI2CDevice::new(bus, I2C_ADDR)
            .map_err(|e| format!("open {bus}: {e}"))?;

        let id = dev.smbus_read_byte_data(WHO_AM_I_REG).map_err(|e| format!("WHO_AM_I: {e}"))?;
        if id != WHO_AM_I_OK {
            return Err(format!("WHO_AM_I = 0x{id:02x}, expected 0x{WHO_AM_I_OK:02x}"));
        }

        // Software reset.
        let mut ctrl3 = dev.smbus_read_byte_data(CTRL3).map_err(|e| format!("read CTRL3: {e}"))?;
        ctrl3 |= 0x01;
        dev.smbus_write_byte_data(CTRL3, ctrl3).map_err(|e| format!("reset: {e}"))?;
        thread::sleep(Duration::from_millis(30));
        // Wait for sw_reset to self-clear.
        for _ in 0..10 {
            ctrl3 = dev.smbus_read_byte_data(CTRL3).unwrap_or(0);
            if ctrl3 & 0x01 == 0 {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }

        // Accelerometer: 30 Hz, default operating mode.
        dev.smbus_write_byte_data(CTRL1, ODR_HZ).map_err(|e| format!("CTRL1: {e}"))?;
        // Gyroscope: 30 Hz, default operating mode.
        dev.smbus_write_byte_data(CTRL2, ODR_HZ).map_err(|e| format!("CTRL2: {e}"))?;
        // BDU (bit 6) + IF_INC (bit 2).
        dev.smbus_write_byte_data(CTRL3, 0x44).map_err(|e| format!("CTRL3: {e}"))?;
        // Gyro full scale: 2000 dps (CTRL6 bits 0-3 = 0x4).
        dev.smbus_write_byte_data(CTRL6, 0x04).map_err(|e| format!("CTRL6: {e}"))?;
        // Accel full scale: 4 g (CTRL8 bits 0-1 = 0x1).
        dev.smbus_write_byte_data(CTRL8, 0x01).map_err(|e| format!("CTRL8: {e}"))?;

        // FIFO watermark = 7 bytes (one SFLP entry).
        dev.smbus_write_byte_data(FIFO_CTRL1, FIFO_ENTRY_LEN as u8)
            .map_err(|e| format!("FIFO_CTRL1: {e}"))?;
        // Batch accel at 30 Hz (low nibble), gyro at 30 Hz (high nibble).
        dev.smbus_write_byte_data(FIFO_CTRL3, (ODR_HZ << 4) | ODR_HZ)
            .map_err(|e| format!("FIFO_CTRL3: {e}"))?;
        // FIFO stream mode (bits 0-2 = 0x6).
        dev.smbus_write_byte_data(FIFO_CTRL4, 0x06).map_err(|e| format!("FIFO_CTRL4: {e}"))?;

        // Switch to embedded function bank.
        dev.smbus_write_byte_data(FUNC_CFG_ACCESS, 0x80)
            .map_err(|e| format!("enter emb bank: {e}"))?;
        // Enable SFLP game rotation (EMB_FUNC_EN_A bit 1).
        dev.smbus_write_byte_data(EMB_FUNC_EN_A, 0x02)
            .map_err(|e| format!("EMB_FUNC_EN_A: {e}"))?;
        // Enable SFLP game in FIFO (EMB_FUNC_FIFO_EN_A bit 1).
        dev.smbus_write_byte_data(EMB_FUNC_FIFO_EN_A, 0x02)
            .map_err(|e| format!("EMB_FUNC_FIFO_EN_A: {e}"))?;
        // SFLP ODR = 30 Hz (bits 0-2 = 0x1).
        dev.smbus_write_byte_data(SFLP_ODR, 0x01)
            .map_err(|e| format!("SFLP_ODR: {e}"))?;
        // Switch back to main bank.
        dev.smbus_write_byte_data(FUNC_CFG_ACCESS, 0x00)
            .map_err(|e| format!("leave emb bank: {e}"))?;

        // Wait for SFLP to bootstrap.
        thread::sleep(SFLP_STARTUP);

        tracing::info!("body IMU: LSM6DSV16X ready on {bus} @ 0x{I2C_ADDR:02x}");
        Ok(Self {
            dev,
            decoder: SflpDecoder::default(),
            stale: ImuStale::default(),
        })
    }

    /// Read one sample: SFLP quaternion from the FIFO, raw gyro from the output registers.
    /// Returns `None` if the FIFO has no complete entry yet.
    pub fn read(&mut self) -> Option<ImuData> {
        // Drain non-game-rotation entries until we find the SFLP tag or exhaust the FIFO.
        let mut fifo = [0u8; FIFO_ENTRY_LEN];
        loop {
            let lo = self.dev.smbus_read_byte_data(FIFO_STATUS1).ok()?;
            let hi = self.dev.smbus_read_byte_data(FIFO_STATUS2).ok()?;
            let level = ((hi as u16 & 0x01) << 8) | lo as u16;
            if level < FIFO_ENTRY_LEN as u16 {
                return None;
            }
            if self.dev.write(&[FIFO_DATA_OUT_TAG]).is_err()
                || self.dev.read(&mut fifo).is_err()
            {
                return None;
            }
            if fifo[0] == SFLP_GAME_TAG {
                break;
            }
        }

        // The 0x17 payload is a rotation vector: 3 × i16 LE, scale 2⁻¹⁵ rad/LSB.
        // Convert to a unit quaternion, then encode (Qx, Qy, Qz) as IEEE half-floats
        // for the SflpDecoder which expects that format.
        let rotvec_bytes = &fifo[1..7];
        let rx = i16::from_le_bytes([rotvec_bytes[0], rotvec_bytes[1]]) as f64 / 32768.0;
        let ry = i16::from_le_bytes([rotvec_bytes[2], rotvec_bytes[3]]) as f64 / 32768.0;
        let rz = i16::from_le_bytes([rotvec_bytes[4], rotvec_bytes[5]]) as f64 / 32768.0;

        let angle = (rx * rx + ry * ry + rz * rz).sqrt();
        let (qw, qx, qy, qz) = if angle < 1e-9 {
            (1.0, 0.0, 0.0, 0.0)
        } else {
            let half = angle * 0.5;
            let sinc_half = half.sin() / angle;
            (half.cos(), rx * sinc_half, ry * sinc_half, rz * sinc_half)
        };

        // Read raw gyro (6 bytes from OUT_GYRO_X_L).
        let mut gyro = [0u8; 6];
        if self.dev.write(&[OUT_GYRO_X_L]).is_err()
            || self.dev.read(&mut gyro).is_err()
        {
            return None;
        }

        // Build the 12-byte block the decoder expects:
        // [gyro_x(2), gyro_y(2), gyro_z(2), quat_x(2), quat_y(2), quat_z(2)]
        let mut block = [0u8; 12];
        block[..6].copy_from_slice(&gyro);
        block[6..8].copy_from_slice(&f32_to_f16(qx));
        block[8..10].copy_from_slice(&f32_to_f16(qy));
        block[10..12].copy_from_slice(&f32_to_f16(qz));

        let data = self.decoder.decode(&block);
        if self.decoder.ready() {
            self.stale.run = 0;
        } else {
            self.stale.total += 1;
            self.stale.run += 1;
        }
        Some(data)
    }

    pub fn ready(&self) -> bool {
        self.decoder.ready()
    }

    pub fn stale(&self) -> ImuStale {
        self.stale
    }
}
