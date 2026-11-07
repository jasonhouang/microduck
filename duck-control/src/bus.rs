//! The Feetech HD1910 bus.
//!
//! FT-SCS has no `sync_read`, so each servo is read individually. The tick reads only
//! position, velocity and current (6 bytes per servo) to stay within budget; voltage and
//! temperature come from [`RobotIo::slow_sensors`], called about once a second.
//!
//! Written against the in-crate `feetech` module. Conversion factors come from the HD1910
//! datasheet: 0.087°/unit position, 0.732 RPM/unit velocity, 6.5 mA/unit current.

use std::f64::consts::PI;
use std::time::Duration;

use crate::feetech::registers;
use crate::feetech::servo::Servo;
use crate::imu::{SflpDecoder, IMU_BLOCK_LEN};
use crate::io::{ImuStale, IoError, JointTargets, Result, RobotIo, Sensors, SlowSensors};
use crate::model::{
    BAUD_RATE, EXPECTED_REGISTERS, FACTORY_ID, IMU_BUS_ADDR, IMU_BUS_ID, IMU_BUS_LEN, JOINT_IDS, JOINT_NAMES, NUM_JOINTS,
};

/// 0.087 degrees per unit → radians per unit.
const RAD_PER_UNIT: f64 = 0.087 * PI / 180.0;

/// 0.732 RPM per unit → rad/s per unit.
const RAD_PER_SEC_PER_UNIT: f64 = 0.732 * 2.0 * PI / 60.0;

/// 6.5 mA per unit.
const MA_PER_UNIT: f64 = 6.5;

/// 0.1 V per unit (register 62).
const VOLTS_PER_COUNT: f64 = 0.1;

/// A healthy servo answers well inside this. Capping means a missing device costs a bounded
/// hiccup rather than stalling the loop on the serial driver's default.
const READ_TIMEOUT: Duration = Duration::from_millis(30);

/// How long a servo is off the bus after a factory reset before it answers again.
const REBOOT_SETTLE: Duration = Duration::from_millis(500);

/// Pause after each EPROM write.
const EEPROM_SETTLE: Duration = Duration::from_millis(20);

/// HD1910 position offset: 2048 = 0° (center)
const POS_OFFSET: i64 = 2048;

/// Run of consecutive stale reads at which the journal says something.
/// 25 reads is half a second at 50 Hz — the same span [`SflpDecoder::ready`] waits for before
/// it will call the chip's output a measurement.
const STALE_RUN_WARN: u64 = 25;

/// IMU source configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImuSource {
    /// No IMU fitted.
    None,
    /// IMU on the servo bus (ID 200, address 124).
    ServoBus,
    /// IMU on a separate I2C bus (Linux only).
    #[cfg(target_os = "linux")]
    I2c,
}

impl Default for ImuSource {
    fn default() -> Self {
        ImuSource::None
    }
}

impl std::str::FromStr for ImuSource {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "none" | "off" | "disabled" => Ok(ImuSource::None),
            "servo" | "servo-bus" | "bus" => Ok(ImuSource::ServoBus),
            #[cfg(target_os = "linux")]
            "i2c" => Ok(ImuSource::I2c),
            #[cfg(not(target_os = "linux"))]
            "i2c" => Err("I2C IMU is only supported on Linux".to_string()),
            _ => Err(format!("unknown IMU source: {}", s)),
        }
    }
}

fn raw_to_radians(raw: i64) -> f64 {
    // Negate: Feetech servos are installed opposite to the simulation's Dynamixel convention.
    // This is the single place where the hardware sign flip happens.
    -((raw - POS_OFFSET) as f64 * RAD_PER_UNIT)
}

fn radians_to_raw(rad: f64) -> i64 {
    // Negate: see raw_to_radians.
    (-rad / RAD_PER_UNIT).round() as i64 + POS_OFFSET
}

fn raw_vel_to_rad_per_sec(raw: i64) -> f64 {
    raw as f64 * RAD_PER_SEC_PER_UNIT
}

pub struct FeetechIo {
    servo: Servo,
    imu_source: ImuSource,
    /// I2C IMU (Linux only, when imu_source == I2c).
    #[cfg(target_os = "linux")]
    imu_i2c: Option<crate::imu_i2c::ImuBus>,
    /// Servo bus IMU decoder (when imu_source == ServoBus).
    imu_bus_decoder: Option<SflpDecoder>,
    /// Stale tracking for servo bus IMU.
    imu_bus_stale: Option<StaleImuTracker>,
}

/// Tracks stale IMU reads on the servo bus.
#[derive(Default)]
struct StaleImuTracker {
    last: Option<[u8; IMU_BLOCK_LEN]>,
    stale: ImuStale,
}

impl StaleImuTracker {
    fn observe(&mut self, block: &[u8; IMU_BLOCK_LEN]) -> u64 {
        if self.last.replace(*block) == Some(*block) {
            self.stale.total = self.stale.total.saturating_add(1);
            self.stale.run = self.stale.run.saturating_add(1);
        } else {
            self.stale.run = 0;
        }
        self.stale.run
    }

    fn get(&self) -> ImuStale {
        self.stale
    }
}

impl FeetechIo {
    /// Open the servo bus and optionally attach an IMU.
    ///
    /// `imu_source` selects where the body IMU lives:
    /// - `ImuSource::None`: no body IMU
    /// - `ImuSource::ServoBus`: IMU board on the servo bus (ID 200, address 124)
    /// - `ImuSource::I2c`: IMU on a separate I2C bus (Linux only)
    ///
    /// `i2c_bus_path` is only used when `imu_source == I2c`.
    /// `imu_mount` is the mount quaternion for the body IMU orientation correction.
    pub fn open(port: &str, imu_source: ImuSource, i2c_bus_path: Option<&str>, imu_mount: [f64; 4]) -> Result<Self> {
        let servo = Servo::connect(port, BAUD_RATE, READ_TIMEOUT.as_millis() as u64, false, false)
            .map_err(|e| IoError::Port {
                path: port.to_owned(),
                source: std::io::Error::other(std::io::Error::other(e.to_string())),
            })?;

        #[cfg(target_os = "linux")]
        let imu_i2c = if imu_source == ImuSource::I2c {
            match i2c_bus_path {
                Some(bus) => match crate::imu_i2c::ImuBus::open(std::path::Path::new(bus), 0x6A) {
                    Ok(imu) => Some(imu),
                    Err(e) => {
                        eprintln!("IMU open error: {}", e);
                        tracing::warn!(error = %e, bus, "I2C IMU not available");
                        None
                    }
                },
                None => {
                    tracing::warn!("I2C IMU requested but no bus path provided");
                    None
                }
            }
        } else {
            None
        };

        let imu_bus_decoder = if imu_source == ImuSource::ServoBus {
            Some(SflpDecoder::new(imu_mount))
        } else {
            None
        };

        let imu_bus_stale = if imu_source == ImuSource::ServoBus {
            Some(StaleImuTracker::default())
        } else {
            None
        };

        Ok(Self {
            servo,
            imu_source,
            #[cfg(target_os = "linux")]
            imu_i2c,
            imu_bus_decoder,
            imu_bus_stale,
        })
    }

    /// Assert — and correct — the EPROM registers the control loop depends on.
    /// Returns how many needed fixing.
    pub fn check_registers(&mut self) -> Result<usize> {
        let mut fixed = 0;
        for &id in &JOINT_IDS {
            fixed += self.check_registers_of(id)?;
        }
        Ok(fixed)
    }

    fn check_registers_of(&mut self, id: u8) -> Result<usize> {
        let mut fixed = 0;
        for &(name, want) in EXPECTED_REGISTERS {
            let reg = registers::find_register(name)
                .unwrap_or_else(|| panic!("unknown register {name}"));
            let got = self
                .servo
                .read_reg(id, reg)
                .map_err(|e| IoError::Bus(format!("read {name} on {id}: {e}")))?;

            if got == want as i64 {
                continue;
            }
            tracing::warn!(id, register = name, got, want, "correcting motor register");
            self.servo
                .save_reg(id, reg, want as i64)
                .map_err(|e| IoError::Bus(format!("write {name} on {id}: {e}")))?;
            std::thread::sleep(EEPROM_SETTLE);
            fixed += 1;
        }

        // pos-offset (homing offset) is user-calibrated and must not be overwritten.
        // The original code enforced a fixed homing_offset per joint (knees had -512,
        // others 0), but this clobbers user calibration done with external tools.
        // If a servo needs a homing offset, it should be calibrated once and preserved.

        Ok(fixed)
    }

    /// The expected servo IDs that do not answer a ping, in [`JOINT_IDS`] order.
    pub fn missing_servos(&mut self) -> Result<Vec<u8>> {
        let mut missing = Vec::new();
        for &id in &JOINT_IDS {
            let answered = self
                .servo
                .ping(id)
                .map_err(|e| IoError::Bus(format!("ping {id}: {e}")))?;
            if !answered {
                missing.push(id);
            }
        }
        Ok(missing)
    }

    /// Flash a factory-fresh servo so it takes the place of the one that is missing.
    ///
    /// A new HD1910 answers as ID 1 at 1 Mbps (baud index 0). ID 1 is unused on this bus,
    /// so when exactly one expected servo is silent the new one can be found, given the
    /// missing ID, switched to the correct ID, and then handed the same EEPROM check every
    /// other servo gets.
    ///
    /// Returns `Ok(false)` when nothing answers at the factory ID.
    pub fn adopt_replacement(&mut self, id: u8) -> Result<bool> {
        let name = JOINT_IDS
            .iter()
            .position(|&j| j == id)
            .map(|i| JOINT_NAMES[i])
            .ok_or_else(|| IoError::Bus(format!("{id} is not a joint id")))?;

        if !self.ping_fresh()? {
            return Ok(false);
        }
        tracing::warn!(
            id,
            joint = name,
            "factory-fresh servo on the bus; flashing it as the missing joint"
        );

        // Write new ID.
        let id_reg = registers::find_register("id").unwrap();
        self.servo
            .save_reg(FACTORY_ID, id_reg, id as i64)
            .map_err(|e| IoError::Bus(format!("write id {id} on {FACTORY_ID}: {e}")))?;
        std::thread::sleep(EEPROM_SETTLE);

        // Now an ordinary servo at the right address: the same check the others get pins
        // baud, resp-level, unload-cond.
        let fixed = self.check_registers_of(id)?;

        // Factory reset to clear any hardware-error latch, then verify it came back.
        RobotIo::reboot(self, id)?;
        std::thread::sleep(REBOOT_SETTLE);
        let back = self
            .servo
            .ping(id)
            .map_err(|e| IoError::Bus(format!("ping {id} after reboot: {e}")))?;
        if !back {
            return Err(IoError::Bus(format!(
                "servo {id} ({name}) was flashed but did not come back from its reboot"
            )));
        }
        tracing::warn!(
            id,
            joint = name,
            registers_fixed = fixed,
            "replacement servo adopted"
        );
        Ok(true)
    }

    fn ping_fresh(&mut self) -> Result<bool> {
        self.servo
            .ping(FACTORY_ID)
            .map_err(|e| IoError::Bus(format!("ping factory id {FACTORY_ID}: {e}")))
    }

    /// Present positions only — a lighter read than [`RobotIo::read`], used once at startup
    /// to adopt the pose the robot is already in.
    pub fn present_positions(&mut self) -> Result<[f64; NUM_JOINTS]> {
        let pos_reg = registers::find_register("pos").unwrap();
        let mut out = [0.0; NUM_JOINTS];
        for (i, &id) in JOINT_IDS.iter().enumerate() {
            match self.servo.read_reg(id, pos_reg) {
                Ok(raw) => out[i] = raw_to_radians(raw),
                Err(e) => {
                    tracing::warn!(id, error = %e, "failed to read position, using 0.0");
                }
            }
        }
        Ok(out)
    }

    /// Ramp every joint from where it is now to `target`, linearly. Blocking.
    pub fn interpolate_to(
        &mut self,
        target: &[f64; NUM_JOINTS],
        duration: Duration,
        step: Duration,
    ) -> Result<()> {
        let start = self.present_positions()?;
        let steps = (duration.as_secs_f64() / step.as_secs_f64())
            .ceil()
            .max(1.0) as u32;
        for i in 1..=steps {
            let t = i as f64 / steps as f64;
            let mut next = [0.0; NUM_JOINTS];
            for j in 0..NUM_JOINTS {
                next[j] = start[j] + (target[j] - start[j]) * t;
            }
            self.write(&JointTargets::new(next))?;
            std::thread::sleep(step);
        }
        Ok(())
    }

    /// Read a single register from a servo (for diagnostics).
    pub fn read_reg(&mut self, id: u8, reg: &crate::feetech::registers::Register) -> Result<i64> {
        self.servo.read_reg(id, reg).map_err(|e| IoError::Bus(format!("read reg on {id}: {e}")))
    }

    /// Read raw IMU bytes directly from the bus (for diagnostics)
    #[cfg(target_os = "linux")]
    pub fn read_imu_raw(&mut self) -> std::result::Result<[u8; 12], String> {
        use crate::model::{IMU_BUS_ADDR, IMU_BUS_ID, IMU_BUS_LEN};

        // Read 12 bytes from IMU board at address 124
        match self.servo.read(IMU_BUS_ID, IMU_BUS_ADDR, IMU_BUS_LEN) {
            Ok(data) => {
                if data.len() >= 12 {
                    let mut bytes = [0u8; 12];
                    bytes.copy_from_slice(&data[..12]);
                    Ok(bytes)
                } else {
                    Err(format!(
                        "IMU returned only {} bytes, expected 12",
                        data.len()
                    ))
                }
            }
            Err(e) => Err(format!("Failed to read IMU: {}", e)),
        }
    }
    pub fn read_imu(&mut self) -> Option<crate::imu::ImuData> {
        match self.imu_source {
            ImuSource::I2c => self.imu_i2c.as_mut().and_then(|imu| imu.read().ok()),
            ImuSource::ServoBus => None, // Servo bus IMU is read in the main read() loop
            ImuSource::None => None,
        }
    }
}

/// Which servo a factory-fresh one should become, given the IDs that did not answer.
pub fn replacement_target(missing: &[u8]) -> Option<u8> {
    match missing {
        [one] => Some(*one),
        _ => None,
    }
}

impl RobotIo for FeetechIo {
    /// Read position, velocity and load for every joint, plus body IMU orientation when fitted.
    ///
    /// When IMU source is ServoBus, the IMU board (ID 200) is read in the same sync_read as the
    /// servos, from address 124 (12 bytes: gyro + quaternion). When IMU source is I2c, the IMU
    /// is read from a separate I2C bus.
    fn read(&mut self) -> Result<Sensors> {
        let mut sensors = Sensors::default();

        // Build the ID list for sync_read: servos + optionally the IMU board
        let mut ids = Vec::with_capacity(JOINT_IDS.len() + 1);
        ids.extend_from_slice(&JOINT_IDS);
        if self.imu_source == ImuSource::ServoBus {
            ids.push(IMU_BUS_ID);
        }

        // Use SyncRead (0x82) to read all servos (and IMU if on bus) in one transaction
        let results = self
            .servo
            .sync_read(&ids, 56, 6)
            .map_err(|e| IoError::Bus(format!("sync_read feedback: {e}")))?;

        // Strict check: all servos must respond (IMU is optional, may not answer)
        let servo_count = results.iter().filter(|(id, _)| *id != IMU_BUS_ID).count();
        if servo_count != JOINT_IDS.len() {
            return Err(IoError::ShortRead {
                what: "sync_read feedback",
                expected: JOINT_IDS.len(),
                got: servo_count,
            });
        }

        // Parse servo results
        for (id, raw) in &results {
            if *id == IMU_BUS_ID {
                continue; // IMU handled separately
            }
            if let Some(i) = JOINT_IDS.iter().position(|&jid| jid == *id) {
                if raw.len() < 6 {
                    return Err(IoError::ShortRead {
                        what: "motor feedback",
                        expected: 6,
                        got: raw.len(),
                    });
                }
                let u16le = |o: usize| u16::from_le_bytes([raw[o], raw[o + 1]]);
                let raw_pos = registers::decode_value(u16le(0), Some(15));
                let raw_vel = registers::decode_value(u16le(2), Some(15));
                let raw_load = registers::decode_value(u16le(4), Some(10));

                sensors.positions[i] = raw_to_radians(raw_pos);
                sensors.velocities[i] = raw_vel_to_rad_per_sec(raw_vel);
                // Load is duty-cycle %, take magnitude like the old bus did for current.
                sensors.currents_ma[i] = raw_load.abs() as f64 * 10.0; // 0.1% per unit → ~mA proxy
            } else {
                return Err(IoError::Bus(format!(
                    "sync_read: unexpected servo ID {} in response",
                    id
                )));
            }
        }

        // Handle IMU based on source
        match self.imu_source {
            ImuSource::ServoBus => {
                // IMU board serves its data at IMU_BUS_ADDR (56, same as servos).
                // Read it separately with a single-device read instruction.
                match self.servo.read(IMU_BUS_ID, IMU_BUS_ADDR, IMU_BUS_LEN) {
                    Ok(raw) if raw.len() >= IMU_BLOCK_LEN => {
                        let mut block = [0u8; IMU_BLOCK_LEN];
                        block.copy_from_slice(&raw[..IMU_BLOCK_LEN]);
                        if let (Some(decoder), Some(tracker)) =
                            (&mut self.imu_bus_decoder, &mut self.imu_bus_stale)
                        {
                            tracker.observe(&block);
                            sensors.imu = decoder.decode(&block);
                        }
                    }
                    Ok(_) => {
                        // Short response — IMU board answered but with fewer bytes than expected.
                        // Degrade gracefully: keep the last good IMU sample.
                    }
                    Err(_) => {
                        // IMU board did not answer. This is expected if it is not fitted.
                        // Degrade gracefully: keep the last good IMU sample.
                    }
                }
            }
            #[cfg(target_os = "linux")]
            ImuSource::I2c => {
                if let Some(imu) = self.imu_i2c.as_mut() {
                    if let Ok(data) = imu.read() {
                        sensors.imu = data;
                    }
                }
            }
            ImuSource::None => {}
        }

        Ok(sensors)
    }

    fn write(&mut self, targets: &JointTargets) -> Result<()> {
        let goal_pos = registers::find_register("goal-pos").unwrap();
        let raw: Vec<i64> = targets.positions.iter().map(|&rad| radians_to_raw(rad)).collect();
        self.servo
            .sync_write(&JOINT_IDS, goal_pos, &raw)
            .map_err(|e| IoError::Bus(format!("sync_write goal positions: {e}")))?;
        Ok(())
    }

    fn set_torque(&mut self, on: bool) -> Result<()> {
        let reg = registers::find_register("torque-switch").unwrap();
        let val = if on { 1 } else { 0 };
        let mut failed = Vec::new();
        for &id in &JOINT_IDS {
            if let Err(e) = self.servo.write_reg(id, reg, val) {
                failed.push(format!("torque {on} on {id}: {e}"));
            }
        }
        if failed.is_empty() {
            Ok(())
        } else {
            Err(IoError::Bus(failed.join("; ")))
        }
    }

    fn reboot(&mut self, id: u8) -> Result<()> {
        // FT-SCS Reset (0x06) restores EPROM defaults. The servo may not ack before resetting.
        let _ = self.servo.factory_reset(id);
        Ok(())
    }

    fn set_gain(&mut self, kp: u16) -> Result<()> {
        let kp_reg = registers::find_register("kp").unwrap();
        let kd_reg = registers::find_register("kd").unwrap();
        let ki_reg = registers::find_register("ki").unwrap();
        for &id in &JOINT_IDS {
            self.servo
                .write_reg(id, kp_reg, kp as i64)
                .map_err(|e| IoError::Bus(format!("kp {kp} on {id}: {e}")))?;
            self.servo
                .write_reg(id, kd_reg, 0)
                .map_err(|e| IoError::Bus(format!("kd 0 on {id}: {e}")))?;
            self.servo
                .write_reg(id, ki_reg, 0)
                .map_err(|e| IoError::Bus(format!("ki 0 on {id}: {e}")))?;
        }
        Ok(())
    }

    fn slow_sensors(&mut self) -> Result<SlowSensors> {
        let vreg = registers::find_register("voltage").unwrap();
        let treg = registers::find_register("temp").unwrap();

        let mut temps_c = [0.0; NUM_JOINTS];
        let mut volts = Vec::with_capacity(NUM_JOINTS);

        for (joint, &id) in JOINT_IDS.iter().enumerate() {
            let raw_v = self
                .servo
                .read_reg(id, vreg)
                .map_err(|e| IoError::Bus(format!("read voltage on {id}: {e}")))?;
            let raw_t = self
                .servo
                .read_reg(id, treg)
                .map_err(|e| IoError::Bus(format!("read temp on {id}: {e}")))?;

            let v = raw_v as f64 * VOLTS_PER_COUNT;
            if v > 0.0 {
                volts.push(v);
            }
            temps_c[joint] = raw_t as f64;
        }

        if volts.is_empty() {
            return Err(IoError::ShortRead {
                what: "input voltage",
                expected: NUM_JOINTS,
                got: 0,
            });
        }
        Ok(SlowSensors {
            volts: volts.iter().sum::<f64>() / volts.len() as f64,
            temps_c,
        })
    }

    // Body IMU: reports real state when fitted, defaults otherwise.
    fn imu_stale(&self) -> ImuStale {
        match self.imu_source {
            ImuSource::ServoBus => self
                .imu_bus_stale
                .as_ref()
                .map(|t| t.get())
                .unwrap_or_default(),
            #[cfg(target_os = "linux")]
            ImuSource::I2c => self
                .imu_i2c
                .as_ref()
                .map(|imu| imu.stale())
                .unwrap_or_default(),
            ImuSource::None => ImuStale::default(),
        }
    }

    fn imu_ready(&self) -> bool {
        match self.imu_source {
            ImuSource::ServoBus => self
                .imu_bus_decoder
                .as_ref()
                .map(|d| d.ready())
                .unwrap_or(false),
            #[cfg(target_os = "linux")]
            ImuSource::I2c => self
                .imu_i2c
                .as_ref()
                .map(|imu| imu.ready())
                .unwrap_or(false),
            ImuSource::None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_replacement_is_inferred_only_from_exactly_one_missing_servo() {
        assert_eq!(replacement_target(&[]), None);
        assert_eq!(replacement_target(&[23]), Some(23));
        assert_eq!(replacement_target(&[23, 31]), None);
        assert_eq!(replacement_target(&JOINT_IDS), None);
    }

    /// Position round-trip: radians → raw → radians.
    #[test]
    fn position_conversion_round_trips() {
        for rad in [-PI, -1.0, 0.0, 0.5, PI] {
            let raw = radians_to_raw(rad);
            let back = raw_to_radians(raw);
            assert!(
                (back - rad).abs() < RAD_PER_UNIT,
                "rad {rad} did not survive the round trip: got {back}"
            );
        }
    }

    /// Velocity scale: 1 unit = 0.732 RPM = 0.732 * 2π/60 rad/s.
    #[test]
    fn velocity_scale_matches_the_datasheet_figure() {
        let one_unit = RAD_PER_SEC_PER_UNIT;
        let expected_rpm = 0.732;
        assert!((one_unit * 60.0 / (2.0 * PI) - expected_rpm).abs() < 1e-12);
    }
}
