//! IMU raw bytes diagnostic - read raw 12 bytes from IMU board
//!
//! Usage: cargo run --example imu-raw --target aarch64-unknown-linux-gnu

fn main() {
    #[cfg(target_os = "linux")]
    {
        use duck_control::bus::FeetechIo;
        use duck_control::io::RobotIo;
        use duck_control::ImuSource;

        println!("Opening servo bus with servo-bus IMU source...");
        let mut io = match FeetechIo::open("/dev/ttyS2", ImuSource::ServoBus, None, [1.0, 0.0, 0.0, 0.0]) {
            Ok(io) => io,
            Err(e) => {
                eprintln!("Failed to open servo bus: {}", e);
                return;
            }
        };

        println!("Reading raw IMU bytes 5 times (100ms interval)...\n");

        for i in 0..5 {
            // Read raw IMU bytes directly from the bus
            match io.read_imu_raw() {
                Ok(bytes) => {
                    println!("Read #{}:", i + 1);
                    print!("  Raw bytes (12): ");
                    for b in &bytes {
                        print!("{:02X} ", b);
                    }
                    println!();
                    
                    // Parse gyro (first 6 bytes, 3x i16 LE)
                    let gyro_x = i16::from_le_bytes([bytes[0], bytes[1]]);
                    let gyro_y = i16::from_le_bytes([bytes[2], bytes[3]]);
                    let gyro_z = i16::from_le_bytes([bytes[4], bytes[5]]);
                    println!("  Gyro raw counts:  [{}, {}, {}]", gyro_x, gyro_y, gyro_z);
                    
                    // Parse quaternion (next 6 bytes, 3x u16 LE half-precision)
                    let quat_x_raw = u16::from_le_bytes([bytes[6], bytes[7]]);
                    let quat_y_raw = u16::from_le_bytes([bytes[8], bytes[9]]);
                    let quat_z_raw = u16::from_le_bytes([bytes[10], bytes[11]]);
                    println!("  Quat raw (half):  [0x{:04X}, 0x{:04X}, 0x{:04X}]", 
                             quat_x_raw, quat_y_raw, quat_z_raw);
                    
                    // Check if all zeros
                    if bytes.iter().all(|&b| b == 0) {
                        println!("  → All zeros - IMU chip not initialized or not responding");
                    } else if gyro_x == 0 && gyro_y == 0 && gyro_z == 0 &&
                              quat_x_raw == 0 && quat_y_raw == 0 && quat_z_raw == 0 {
                        println!("  → Gyro and quat both zero - SFLP not started");
                    } else if quat_x_raw == 0 && quat_y_raw == 0 && quat_z_raw == 0 {
                        println!("  → Quaternion zero but gyro has data - SFLP not started");
                    } else {
                        println!("  → Data present - SFLP may be running");
                    }
                    
                    println!();
                }
                Err(e) => {
                    eprintln!("Read failed: {}", e);
                }
            }
            
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("This example only works on Linux");
    }
}
