//! ADXL345 三轴加速度计驱动。
//!
//! I²C 地址：0x53（SDO=GND）或 0x1D（SDO=VCC）。WHO_AM_I 返回 0xE5。
//! 配置为 ±4g、全分辨率 10-bit 模式，100 Hz ODR。
//! 输出单位：g（1 g = 9.80665 m/s²）。

use i2cdev::core::I2CDevice;
use i2cdev::linux::LinuxI2CDevice;
use std::path::Path;

/// ADXL345 默认 I²C 地址（SDO 接 GND）。
pub const DEFAULT_ADDR: u16 = 0x53;
/// WHO_AM_I 寄存器。
const REG_DEVID: u8 = 0x00;
/// 期望的器件 ID。
const DEVID: u8 = 0xE5;
/// 数据格式寄存器。
const REG_DATA_FORMAT: u8 = 0x31;
/// 电源控制寄存器。
const REG_POWER_CTL: u8 = 0x2D;
/// 采样率寄存器。
const REG_BW_RATE: u8 = 0x2C;
/// 数据起始寄存器（X0 低字节）。
const REG_DATA_X0: u8 = 0x32;
/// 温度寄存器。
const REG_TEMP: u8 = 0x33;

/// ±4g 量程，全分辨率（10-bit）。
const DATA_FORMAT: u8 = 0x09;
/// 100 Hz 输出数据率。
const BW_RATE: u8 = 0x0A;
/// 进入测量模式。
const POWER_MEASURE: u8 = 0x08;

/// ±4g 全分辨率下，每 LSB 对应的 g 值（3.9 mg/LSB）。
const SCALE_G_PER_LSB: f64 = 0.0039;

/// ADXL345 加速度计。
pub struct Adxl345 {
    dev: LinuxI2CDevice,
}

impl Adxl345 {
    /// 探测并初始化 ADXL345。
    pub fn new(bus: &Path, addr: u16) -> Result<Self, anyhow::Error> {
        let mut dev = LinuxI2CDevice::new(bus, addr)
            .map_err(|e| anyhow::anyhow!("ADXL345 open {bus:?} @ 0x{addr:02X}: {e}"))?;

        // WHO_AM_I 验证
        let id = dev
            .smbus_read_byte_data(REG_DEVID)
            .map_err(|e| anyhow::anyhow!("ADXL345 WHO_AM_I read: {e}"))?;
        if id != DEVID {
            return Err(anyhow::anyhow!(
                "ADXL345 WHO_AM_I mismatch: got 0x{id:02X}, expected 0x{DEVID:02X}"
            ));
        }

        // 采样率 100 Hz
        dev.smbus_write_byte_data(REG_BW_RATE, BW_RATE)
            .map_err(|e| anyhow::anyhow!("ADXL345 BW_RATE: {e}"))?;
        // ±4g, full resolution
        dev.smbus_write_byte_data(REG_DATA_FORMAT, DATA_FORMAT)
            .map_err(|e| anyhow::anyhow!("ADXL345 DATA_FORMAT: {e}"))?;
        // 进入测量模式
        dev.smbus_write_byte_data(REG_POWER_CTL, POWER_MEASURE)
            .map_err(|e| anyhow::anyhow!("ADXL345 POWER_CTL: {e}"))?;

        Ok(Self { dev })
    }

    /// 读取三轴加速度，单位 g。
    pub fn read_accel_g(&mut self) -> Result<[f64; 3], anyhow::Error> {
        let buf = self
            .dev
            .smbus_read_i2c_block_data(REG_DATA_X0, 6)
            .map_err(|e| anyhow::anyhow!("ADXL345 data read: {e}"))?;

        let x = i16::from_le_bytes([buf[0], buf[1]]);
        let y = i16::from_le_bytes([buf[2], buf[3]]);
        let z = i16::from_le_bytes([buf[4], buf[5]]);

        Ok([
            x as f64 * SCALE_G_PER_LSB,
            y as f64 * SCALE_G_PER_LSB,
            z as f64 * SCALE_G_PER_LSB,
        ])
    }

    /// 读取芯片温度（内置传感器，精度 ±3°C）。
    ///
    /// 公式：T(°C) = raw × 0.0625 + 25
    pub fn read_temperature_c(&mut self) -> Result<f32, anyhow::Error> {
        let raw = self
            .dev
            .smbus_read_byte_data(REG_TEMP)
            .map_err(|e| anyhow::anyhow!("ADXL345 temp: {e}"))?;
        Ok((raw as i8) as f32 * 0.0625 + 25.0)
    }
}
