//! 头部 IMU：支持 BMI088 或 ADXL345+L3G4200D 组合。
//!
//! 自动探测总线上安装的传感器组合：
//! 1. 先尝试 BMI088（二合一芯片）
//! 2. 若失败，探测 ADXL345（加速度计）+ L3G4200D（陀螺仪）
//!
//! 两组传感器共享 I2C3 总线，通过 WHO_AM_I 寄存器区分。
//! 输出统一为 `proto::HeadImuFrame`，IPC 通道不变。

pub mod adxl345;
pub mod ahrs;
pub mod l3g4200d;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use duck_ipc_proto as proto;

#[cfg(target_os = "linux")]
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::sync::atomic::Ordering;
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use linux_embedded_hal::I2cdev;

#[cfg(target_os = "linux")]
use crate::BUS_CANDIDATES;

/// Madgwick 收敛速率。与 BMI088 crate 一致。
#[cfg(target_os = "linux")]
const BETA: f64 = 0.1;

/// 探测失败后的退避。
#[cfg(target_os = "linux")]
const RETRY_MIN: Duration = Duration::from_millis(500);
#[cfg(target_os = "linux")]
const RETRY_MAX: Duration = Duration::from_secs(30);

/// 订阅者缓冲大小。~2.5 s @ 100 Hz。
pub const FRAME_BUFFER: usize = 256;

/// 温度读取频率（每 N 次采样）。
#[cfg(target_os = "linux")]
const TEMP_EVERY: u64 = 100;

/// IMU 状态报告。
#[derive(Clone)]
pub struct ImuStatus {
    hz: u8,
    inner: Arc<std::sync::Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    sensor: Option<String>,
    unavailable: Option<String>,
}

impl ImuStatus {
    pub fn new(hz: u8) -> Self {
        Self {
            hz,
            inner: Arc::new(std::sync::Mutex::new(Inner {
                unavailable: Some("no reading yet".to_owned()),
                ..Inner::default()
            })),
        }
    }

    #[cfg(target_os = "linux")]
    fn found(&self, sensor: &str) {
        let mut inner = self.inner.lock().unwrap();
        inner.sensor = Some(sensor.to_owned());
        inner.unavailable = None;
    }

    pub fn off(&self) {
        self.lost(
            "the head IMU is off — `[head_imu] enabled = true` in robotd.toml, then restart tofd"
                .to_owned(),
        );
    }

    fn lost(&self, why: String) {
        let mut inner = self.inner.lock().unwrap();
        inner.sensor = None;
        inner.unavailable = Some(why);
    }

    pub fn result(&self) -> proto::HeadImuStreamResult {
        let inner = self.inner.lock().unwrap();
        proto::HeadImuStreamResult {
            accepted: true,
            sensor: inner.sensor.clone(),
            unavailable: inner.unavailable.clone(),
            hz: self.hz,
        }
    }
}

/// 头部 IMU 抽象：无论底层是 BMI088 还是 ADXL345+L3G4200D，输出统一。
#[cfg(target_os = "linux")]
enum ImuDevice {
    Bmi088(bmi088::Bmi088Ahrs<I2cdev>),
    AdxlL3g {
        accel: adxl345::Adxl345,
        gyro: l3g4200d::L3g4200d,
        ahrs: ahrs::MadgwickAhrs,
    },
}

/// 读取头部 IMU 并发布帧。
#[cfg(target_os = "linux")]
pub fn imu_loop(
    bus: Option<&Path>,
    hz: u8,
    status: &ImuStatus,
    frames: &tokio::sync::broadcast::Sender<proto::HeadImuFrame>,
    shutdown: &Arc<AtomicBool>,
) {
    let started = Instant::now();
    let period = Duration::from_secs_f64(1.0 / f64::from(hz.max(1)));
    let mut seq = 0u64;
    let mut backoff = RETRY_MIN;

    while !shutdown.load(Ordering::Acquire) {
        let mut imu = match open_imu(bus) {
            Ok((device, name)) => {
                tracing::info!("head IMU found: {name}");
                status.found(&name);
                backoff = RETRY_MIN;
                device
            }
            Err(e) => {
                status.lost(e.to_string());
                tracing::warn!(error = %e, backoff_ms = backoff.as_millis(), "no head IMU; will retry");
                sleep_unless_shutdown(backoff, shutdown);
                backoff = (backoff * 2).min(RETRY_MAX);
                continue;
            }
        };

        let mut last = Instant::now();
        let mut temp_c = 0.0f32;
        while !shutdown.load(Ordering::Acquire) {
            let tick = Instant::now();
            let dt = (tick - last).as_secs_f32().clamp(1e-4, 0.2);
            last = tick;

            match read_imu(&mut imu, dt, seq, &mut temp_c) {
                Ok(Some(frame)) => {
                    seq += 1;
                    let _ = frames.send(frame);
                }
                Ok(None) => {
                    // 温度采样周期，不发布帧
                }
                Err(e) => {
                    status.lost(format!("read failed: {e:?}"));
                    tracing::warn!("head IMU read failed; reopening");
                    break;
                }
            }
            let elapsed = tick.elapsed();
            if elapsed < period {
                sleep_unless_shutdown(period - elapsed, shutdown);
            }
        }

        sleep_unless_shutdown(backoff, shutdown);
        backoff = (backoff * 2).min(RETRY_MAX);
    }
}

/// 读取一次 IMU 数据并发布帧。
#[cfg(target_os = "linux")]
fn read_imu(
    imu: &mut ImuDevice,
    dt: f32,
    seq: u64,
    temp_c: &mut f32,
) -> anyhow::Result<Option<proto::HeadImuFrame>> {
    match imu {
        ImuDevice::Bmi088(ahrs) => {
            match ahrs.update_all(dt) {
                Ok((accel, gyro, quat)) => {
                    if seq.is_multiple_of(TEMP_EVERY)
                        && let Ok(t) = ahrs.imu().read_temperature()
                    {
                        *temp_c = t;
                    }
                    Ok(Some(proto::HeadImuFrame {
                        seq,
                        at_us: 0, // 由调用者填充
                        t_ns: proto::clock::monotonic_ns(),
                        gyro,
                        accel,
                        quat,
                        temp_c: *temp_c,
                    }))
                }
                Err(e) => Err(anyhow::anyhow!("BMI088 read: {e:?}")),
            }
        }
        ImuDevice::AdxlL3g {
            accel,
            gyro,
            ahrs,
        } => {
            let accel_g = accel.read_accel_g()?;
            let gyro_dps = gyro.read_gyro_dps()?;

            // Madgwick 融合
            let q = ahrs.update(accel_g, gyro_dps, dt as f64);

            // 转换为 [f32; 4]
            let quat = [q[0] as f32, q[1] as f32, q[2] as f32, q[3] as f32];
            let accel_f = [accel_g[0] as f32, accel_g[1] as f32, accel_g[2] as f32];
            let gyro_f = [gyro_dps[0] as f32, gyro_dps[1] as f32, gyro_dps[2] as f32];

            if seq.is_multiple_of(TEMP_EVERY) {
                if let Ok(t) = gyro.read_temperature_c() {
                    *temp_c = t;
                }
            }

            Ok(Some(proto::HeadImuFrame {
                seq,
                at_us: 0,
                t_ns: proto::clock::monotonic_ns(),
                gyro: gyro_f,
                accel: accel_f,
                quat,
                temp_c: *temp_c,
            }))
        }
    }
}

/// 探测总线上的 IMU。
///
/// 先尝试 BMI088，再尝试 ADXL345+L3G4200D 组合。
#[cfg(target_os = "linux")]
fn open_imu(bus: Option<&Path>) -> anyhow::Result<(ImuDevice, String)> {
    let buses: Vec<PathBuf> = match bus {
        Some(bus) => vec![bus.to_path_buf()],
        None => BUS_CANDIDATES.iter().map(PathBuf::from).collect(),
    };

    let mut last_err = None;

    for bus_path in &buses {
        if !bus_path.exists() {
            last_err = Some(anyhow::anyhow!("{} does not exist", bus_path.display()));
            continue;
        }

        // 尝试 BMI088
        let i2c = match I2cdev::new(bus_path) {
            Ok(i2c) => i2c,
            Err(e) => {
                last_err = Some(anyhow::anyhow!("open {}: {e}", bus_path.display()));
                continue;
            }
        };
        match bmi088::Bmi088::new(i2c, bmi088::Config::default()) {
            Ok(imu) => {
                tracing::info!(bus = %bus_path.display(), "BMI088 answered");
                return Ok((
                    ImuDevice::Bmi088(bmi088::Bmi088Ahrs::new(imu, BETA)),
                    "BMI088".to_owned(),
                ));
            }
            Err(e) => {
                tracing::debug!(bus = %bus_path.display(), "BMI088 not found: {e:?}");
                last_err = Some(anyhow::anyhow!("BMI088 init on {}: {e:?}", bus_path.display()));
            }
        }

        // 尝试 ADXL345 + L3G4200D
        // 每个传感器尝试默认地址和备选地址
        let accel_addrs = [adxl345::DEFAULT_ADDR, 0x1D];
        let gyro_addrs = [l3g4200d::DEFAULT_ADDR, 0x69];

        let mut accel_found: Option<adxl345::Adxl345> = None;
        for &addr in &accel_addrs {
            match adxl345::Adxl345::new(bus_path, addr) {
                Ok(a) => {
                    tracing::info!(bus = %bus_path.display(), addr = format!("0x{addr:02X}"), "ADXL345 found");
                    accel_found = Some(a);
                    break;
                }
                Err(e) => tracing::debug!(addr = format!("0x{addr:02X}"), "ADXL345 probe: {e}"),
            }
        }

        let mut gyro_found: Option<l3g4200d::L3g4200d> = None;
        for &addr in &gyro_addrs {
            match l3g4200d::L3g4200d::new(bus_path, addr) {
                Ok(g) => {
                    tracing::info!(bus = %bus_path.display(), addr = format!("0x{addr:02X}"), "L3G4200D found");
                    gyro_found = Some(g);
                    break;
                }
                Err(e) => tracing::debug!(addr = format!("0x{addr:02X}"), "L3G4200D probe: {e}"),
            }
        }

        match (accel_found, gyro_found) {
            (Some(accel), Some(gyro)) => {
                tracing::info!(bus = %bus_path.display(), "ADXL345+L3G4200D answered");
                return Ok((
                    ImuDevice::AdxlL3g {
                        accel,
                        gyro,
                        ahrs: ahrs::MadgwickAhrs::new(BETA),
                    },
                    "ADXL345+L3G4200D".to_owned(),
                ));
            }
            _ => {
                let why = "ADXL345+L3G4200D not found on this bus".to_owned();
                tracing::debug!(bus = %bus_path.display(), "{why}");
                last_err = Some(anyhow::anyhow!("{why}"));
            }
        }
    }

    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no bus to look on")))
}

#[cfg(not(target_os = "linux"))]
pub fn imu_loop(
    _bus: Option<&Path>,
    _hz: u8,
    status: &ImuStatus,
    _frames: &tokio::sync::broadcast::Sender<proto::HeadImuFrame>,
    _shutdown: &Arc<AtomicBool>,
) {
    status.lost("the head IMU is on an I2C bus, which exists only on Linux".to_owned());
}

#[cfg(target_os = "linux")]
fn sleep_unless_shutdown(dur: Duration, shutdown: &Arc<AtomicBool>) {
    let slice = Duration::from_millis(50);
    let mut left = dur;
    while left > Duration::ZERO && !shutdown.load(Ordering::Acquire) {
        let step = left.min(slice);
        std::thread::sleep(step);
        left = left.saturating_sub(step);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switched_off_reads_differently_from_absent() {
        let status = ImuStatus::new(100);

        let fresh = status.result();
        assert!(fresh.sensor.is_none());
        assert_eq!(fresh.hz, 100);

        status.off();
        let off = status.result();
        let why = off.unavailable.expect("a reason");
        assert!(why.contains("[head_imu] enabled"), "{why}");
        assert!(off.sensor.is_none());
        assert!(off.accepted);

        status.lost("nothing answered on any bus".to_owned());
        let absent = status.result();
        let why = absent.unavailable.expect("a reason");
        assert!(!why.contains("[head_imu]"), "{why}");
    }
}
