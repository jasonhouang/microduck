//! Read and display all servo temperatures and IMU data.

fn main() {
    #[cfg(target_os = "linux")]
    {
        use duck_control::bus::FeetechIo;
        use duck_control::io::RobotIo;

        println!("Opening servo bus and IMU...");
        let mut io = match FeetechIo::open("/dev/ttyS2", Some("/dev/i2c-4")) {
            Ok(io) => io,
            Err(e) => {
                eprintln!("Failed to open servo bus: {}", e);
                return;
            }
        };

        println!("Reading servo temperatures and IMU data...\n");

        // Read temperatures one by one (more tolerant of missing servos)
        println!("Servo temperatures:");
        let temp_reg = duck_control::feetech::registers::find_register("temp").unwrap();
        for (i, name) in duck_control::model::JOINT_NAMES.iter().enumerate() {
            let id = duck_control::model::JOINT_IDS[i];
            match io.read_reg(id, temp_reg) {
                Ok(temp) => println!("  {}: {:.0}°C (ID {})", name, temp, id),
                Err(_) => println!("  {}: N/A (ID {} - no response)", name, id),
            }
        }

        println!("\nReading IMU data...");
        // Read IMU directly (independent of servo bus)
        // Retry a few times since SFLP needs time to initialize.
        #[cfg(target_os = "linux")]
        {
            let mut imu_data = None;
            for _ in 0..10 {
                if let Some(imu) = io.read_imu() {
                    imu_data = Some(imu);
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            match imu_data {
                Some(imu) => {
                println!("IMU orientation (quaternion w,x,y,z):");
                println!("  w: {:.3}, x: {:.3}, y: {:.3}, z: {:.3}",
                    imu.quat[0], imu.quat[1], imu.quat[2], imu.quat[3]);
                println!("IMU angular velocity (gyro, rad/s):");
                println!("  x: {:.3}, y: {:.3}, z: {:.3}",
                    imu.gyro[0], imu.gyro[1], imu.gyro[2]);
                println!("IMU projected gravity (unit vector, upright=[0,0,-1]):");
                println!("  x: {:.3}, y: {:.3}, z: {:.3}",
                    imu.gravity[0], imu.gravity[1], imu.gravity[2]);

                // Calculate tilt angles from quaternion
                // Standard quaternion to euler conversion
                let w = imu.quat[0];
                let x = imu.quat[1];
                let y = imu.quat[2];
                let z = imu.quat[3];

                // Roll (x-axis rotation)
                let sinr_cosp = 2.0 * (w * x + y * z);
                let cosr_cosp = 1.0 - 2.0 * (x * x + y * y);
                let roll = sinr_cosp.atan2(cosr_cosp).to_degrees();

                // Pitch (y-axis rotation)
                let sinp = 2.0 * (w * y - z * x);
                let pitch = if sinp.abs() >= 1.0 {
                    90.0_f64.copysign(sinp)
                } else {
                    sinp.asin().to_degrees()
                };

                // Yaw (z-axis rotation)
                let siny_cosp = 2.0 * (w * z + x * y);
                let cosy_cosp = 1.0 - 2.0 * (y * y + z * z);
                let yaw = siny_cosp.atan2(cosy_cosp).to_degrees();

                println!("\nOrientation from quaternion (degrees):");
                println!("  Roll:  {:.1}° (positive = right lean)", roll);
                println!("  Pitch: {:.1}° (positive = forward lean)", pitch);
                println!("  Yaw:   {:.1}° (positive = clockwise)", yaw);

                // Also calculate from gravity vector
                let gx = imu.gravity[0];
                let gy = imu.gravity[1];
                let gz = imu.gravity[2];
                let grav_roll = (gx).atan2(gz).to_degrees();
                let grav_pitch = (-gy).atan2(gz).to_degrees();
                println!("\nTilt from gravity vector (degrees):");
                println!("  Roll:  {:.1}° (positive = right lean)", grav_roll);
                println!("  Pitch: {:.1}° (positive = forward lean)", grav_pitch);
                }
                None => {
                    println!("IMU not available or read failed");
                }
            }
        }
        #[cfg(not(target_os = "linux"))]
        println!("IMU only available on Linux");
    }

    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("This tool only works on Linux");
    }
}
