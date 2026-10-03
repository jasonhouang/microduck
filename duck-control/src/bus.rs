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
use crate::io::{ImuStale, IoError, JointTargets, Result, RobotIo, Sensors, SlowSensors};
use crate::model::{
    BAUD_RATE, EXPECTED_REGISTERS, FACTORY_ID, JOINT_IDS,
    JOINT_NAMES, NUM_JOINTS,
};

#[cfg(target_os = "linux")]
use crate::imu_i2c::Lsm6dsv16x;

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

fn raw_to_radians(raw: i64) -> f64 {
    raw as f64 * RAD_PER_UNIT
}

fn radians_to_raw(rad: f64) -> i64 {
    (rad / RAD_PER_UNIT).round() as i64
}

fn raw_vel_to_rad_per_sec(raw: i64) -> f64 {
    raw as f64 * RAD_PER_SEC_PER_UNIT
}

pub struct FeetechIo {
    servo: Servo,
    #[cfg(target_os = "linux")]
    imu: Option<Lsm6dsv16x>,
}

impl FeetechIo {
    pub fn open(port: &str, imu_bus: Option<&str>) -> Result<Self> {
        let servo = Servo::connect(port, BAUD_RATE, READ_TIMEOUT.as_millis() as u64, true, false)
            .map_err(|e| IoError::Port {
                path: port.to_owned(),
                source: std::io::Error::other(std::io::Error::other(e.to_string())),
            })?;
        #[cfg(target_os = "linux")]
        let imu = match imu_bus {
            Some(bus) => match Lsm6dsv16x::open(bus) {
                Ok(imu) => Some(imu),
                Err(e) => {
                    tracing::warn!(error = %e, bus, "body IMU not available");
                    None
                }
            },
            None => None,
        };
        Ok(Self {
            servo,
            #[cfg(target_os = "linux")]
            imu,
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
            let raw = self
                .servo
                .read_reg(id, pos_reg)
                .map_err(|e| IoError::Bus(format!("read pos on {id}: {e}")))?;
            out[i] = raw_to_radians(raw);
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
    /// One 6-byte read per servo (addresses 56–61: pos, vel, load are contiguous).
    /// 15 servos × 1 transaction = 15 transactions per tick, vs 45 if read separately.
    fn read(&mut self) -> Result<Sensors> {
        let mut sensors = Sensors::default();

        for (i, &id) in JOINT_IDS.iter().enumerate() {
            let raw = self
                .servo
                .read(id, 56, 6)
                .map_err(|e| IoError::Bus(format!("read feedback on {id}: {e}")))?;
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
        }

        #[cfg(target_os = "linux")]
        if let Some(imu) = self.imu.as_mut() {
            if let Some(data) = imu.read() {
                sensors.imu = data;
            }
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
        #[cfg(target_os = "linux")]
        if let Some(imu) = self.imu.as_ref() {
            return imu.stale();
        }
        ImuStale::default()
    }

    fn imu_ready(&self) -> bool {
        #[cfg(target_os = "linux")]
        if let Some(imu) = self.imu.as_ref() {
            return imu.ready();
        }
        // No IMU fitted — fall detection is gated on this elsewhere.
        false
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
