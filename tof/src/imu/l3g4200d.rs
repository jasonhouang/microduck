//! L3G4200D 三轴陀螺仪驱动。
//!
//! I²C 地址：0x68（SDO=0）或 0x69（SDO=1）。WHO_AM_I 返回 0xD3。
//! 配置为 100 Hz ODR、±2000 dps 量程。
//! 输出单位：°/s。

use i2cdev::core::I2CDevice;
use i2cdev::linux::LinuxI2CDevice;
use std::path::Path;

/// L3G4200D 默认 I²C 地址（SDO=0）。
pub const DEFAULT_ADDR: u16 = 0x68;
/// WHO_AM_I 寄存器。
const REG_WHO_AM_I: u8 = 0x0F;
/// 期望的器件 ID。
const WHO_AM_I_VALUE: u8 = 0xD3;
/// 控制寄存器 1：ODR + 使能。
const REG_CTRL_REG1: u8 = 0x20;
/// 控制寄存器 4：量程。
const REG_CTRL_REG4: u8 = 0x23;
/// 输出数据起始寄存器（X 低字节）。
const REG_OUT_X_L: u8 = 0x28;
/// 温度数据寄存器。
const REG_OUT_TEMP: u8 = 0x26;

/// 100 Hz ODR，25 Hz 截止，XYZ 全部使能。
const CTRL_REG1: u8 = 0x0F;
/// ±2000 dps，块数据地址自增。
const CTRL_REG4: u8 = 0x30;

/// ±2000 dps 灵敏度：70 mdps/digit。
const SCALE_DPS_PER_DIGIT: f64 = 0.070;

/// L3G4200D 陀螺仪。
pub struct L3g4200d {
    dev: LinuxI2CDevice,
}

impl L3g4200d {
    /// 探测并初始化 L3G4200D。
    pub fn new(bus: &Path, addr: u16) -> Result<Self, anyhow::Error> {
        let mut dev = LinuxI2CDevice::new(bus, addr)
            .map_err(|e| anyhow::anyhow!("L3G4200D open {bus:?} @ 0x{addr:02X}: {e}"))?;

        // WHO_AM_I 验证
        let id = dev
            .smbus_read_byte_data(REG_WHO_AM_I)
            .map_err(|e| anyhow::anyhow!("L3G4200D WHO_AM_I: {e}"))?;
        if id != WHO_AM_I_VALUE {
            return Err(anyhow::anyhow!(
                "L3G4200D WHO_AM_I mismatch: got 0x{id:02X}, expected 0x{WHO_AM_I_VALUE:02X}"
            ));
        }

        // 100 Hz, XYZ enable
        dev.smbus_write_byte_data(REG_CTRL_REG1, CTRL_REG1)
            .map_err(|e| anyhow::anyhow!("L3G4200D CTRL_REG1: {e}"))?;
        // ±2000 dps, block data increment
        dev.smbus_write_byte_data(REG_CTRL_REG4, CTRL_REG4)
            .map_err(|e| anyhow::anyhow!("L3G4200D CTRL_REG4: {e}"))?;

        Ok(Self { dev })
    }

    /// 读取三轴角速度，单位 °/s。
    pub fn read_gyro_dps(&mut self) -> Result<[f64; 3], anyhow::Error> {
        // MSB=1 启用地址自增（burst read）
        let buf = self
            .dev
            .smbus_read_i2c_block_data(REG_OUT_X_L | 0x80, 6)
            .map_err(|e| anyhow::anyhow!("L3G4200D data read: {e}"))?;

        let x = i16::from_le_bytes([buf[0], buf[1]]);
        let y = i16::from_le_bytes([buf[2], buf[3]]);
        let z = i16::from_le_bytes([buf[4], buf[5]]);

        Ok([
            x as f64 * SCALE_DPS_PER_DIGIT,
            y as f64 * SCALE_DPS_PER_DIGIT,
            z as f64 * SCALE_DPS_PER_DIGIT,
        ])
    }

    /// 读取温度传感器。
    ///
    /// OUT_TEMP (0x26)：有符号 8-bit，T(°C) ≈ raw + 25。
    pub fn read_temperature_c(&mut self) -> Result<f32, anyhow::Error> {
        let raw = self
            .dev
            .smbus_read_byte_data(REG_OUT_TEMP)
            .map_err(|e| anyhow::anyhow!("L3G4200D temp: {e}"))?;
        Ok((raw as i8) as f32 + 25.0)
    }
}
