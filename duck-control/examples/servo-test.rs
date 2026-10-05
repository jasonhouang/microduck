//! Servo direction tester: command each joint individually to verify direction.
//!
//! Usage: cargo run --example servo-test -- --joint <name> --position <radians>
//! Example: cargo run --example servo-test -- --joint left_hip_pitch --position 0.5

use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // If no arguments, just read and display current positions
    if args.len() < 5 {
        #[cfg(target_os = "linux")]
        {
            use duck_control::bus::FeetechIo;
            use duck_control::io::RobotIo;

            eprintln!("Reading current joint positions...\n");
            let mut io = FeetechIo::open("/dev/ttyS2", Some("/dev/i2c-4"))
                .expect("open servo bus");

            let positions = io.present_positions().expect("read positions");
            eprintln!("Current joint positions:");
            for (i, name) in duck_control::model::JOINT_NAMES.iter().enumerate() {
                let home = duck_control::model::DEFAULT_POSITION[i];
                let delta = positions[i] - home;
                eprintln!("  {:<20} (ID {:2}): {:6.3} rad ({:6.1}°)  home: {:6.3} rad  delta: {:+.3} rad",
                    name, duck_control::model::JOINT_IDS[i],
                    positions[i], positions[i].to_degrees(),
                    home, delta);
            }
            return;
        }
        #[cfg(not(target_os = "linux"))]
        {
            eprintln!("This example only works on Linux");
            return;
        }
    }

    let joint_name = &args[2];
    let target_pos: f64 = args[4].parse().expect("position must be a number");

    let joint_idx = duck_control::model::joint_index(joint_name)
        .unwrap_or_else(|| {
            eprintln!("Unknown joint: {}", joint_name);
            std::process::exit(1);
        });

    let servo_id = duck_control::model::JOINT_IDS[joint_idx];
    let home_pos = duck_control::model::DEFAULT_POSITION[joint_idx];

    eprintln!("Testing joint: {} (ID {})", joint_name, servo_id);
    eprintln!("Home position: {:.3} rad", home_pos);
    eprintln!("Target position: {:.3} rad (delta: {:.3})", target_pos, target_pos - home_pos);
    eprintln!("\nPress Ctrl+C to stop\n");

    #[cfg(target_os = "linux")]
    {
        use duck_control::bus::FeetechIo;
        use duck_control::io::RobotIo;

        let mut io = FeetechIo::open("/dev/ttyS2", Some("/dev/i2c-4"))
            .expect("open servo bus");

        eprintln!("Servo bus opened. Reading current positions...");

        // Read and display current positions (one at a time, more tolerant of missing servos)
        let positions = io.present_positions().expect("read positions");
        eprintln!("Current joint positions (radians):");
        for (i, name) in duck_control::model::JOINT_NAMES.iter().enumerate() {
            eprintln!("  {}: {:.3} rad (ID {})", name, positions[i], duck_control::model::JOINT_IDS[i]);
        }

        eprintln!("\nEnabling torque and commanding {} to {:.3} rad...", joint_name, target_pos);
        eprintln!("Press Ctrl+C to stop\n");

        // Enable torque (ignore errors from missing servos)
        if let Err(e) = io.set_torque(true) {
            eprintln!("Warning: torque enable failed (some servos may be missing): {}", e);
        }

        loop {
            let mut targets = duck_control::model::DEFAULT_POSITION;  // Start with home positions
            targets[joint_idx] = target_pos;  // Override the test joint

            io.write(&duck_control::io::JointTargets::new(targets))
                .expect("write targets");

            std::thread::sleep(Duration::from_millis(100));
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("This example only works on Linux (needs real servo bus)");
    }
}
