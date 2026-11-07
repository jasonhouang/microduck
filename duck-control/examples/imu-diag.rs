//! IMU diagnostic tool: read raw bytes from IMU board on servo bus
//!
//! Usage: cargo run --example imu-diag --target aarch64-unknown-linux-gnu

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

        println!("Reading IMU data 10 times (50ms interval)...\n");

        for i in 0..10 {
            // Read sensors which includes IMU
            match io.read() {
                Ok(sensors) => {
                    let imu = sensors.imu;
                    
                    // Print raw quaternion and gyro
                    println!("Read #{}:", i + 1);
                    println!("  Gyro (rad/s):     [{:+.4}, {:+.4}, {:+.4}]", 
                             imu.gyro[0], imu.gyro[1], imu.gyro[2]);
                    println!("  Gravity (unit):   [{:+.4}, {:+.4}, {:+.4}]", 
                             imu.gravity[0], imu.gravity[1], imu.gravity[2]);
                    println!("  Quaternion (w,x,y,z): [{:+.4}, {:+.4}, {:+.4}, {:+.4}]", 
                             imu.quat[0], imu.quat[1], imu.quat[2], imu.quat[3]);
                    
                    // Check if quaternion is identity (no fusion)
                    if (imu.quat[0] - 1.0).abs() < 0.001 && 
                       imu.quat[1].abs() < 0.001 && 
                       imu.quat[2].abs() < 0.001 && 
                       imu.quat[3].abs() < 0.001 {
                        println!("  → Quaternion is identity [1,0,0,0] - SFLP not started");
                    }
                    
                    // Check if all zeros
                    if imu.gyro.iter().all(|&x| x.abs() < 0.0001) &&
                       imu.quat.iter().all(|&x| x.abs() < 0.0001) {
                        println!("  → All zeros - IMU not responding");
                    }
                    
                    println!();
                }
                Err(e) => {
                    eprintln!("Read failed: {}", e);
                }
            }
            
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        
        println!("IMU ready: {}", io.imu_ready());
        let stale = io.imu_stale();
        println!("IMU stale: total={}, run={}", stale.total, stale.run);
    }

    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("This example only works on Linux");
    }
}
