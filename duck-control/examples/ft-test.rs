use duck_control::feetech::servo::Servo;
use duck_control::feetech::registers;

fn main() {
    let port = "/dev/ttyS2";
    eprintln!("Opening {} at 1Mbps...", port);

    let mut servo = Servo::connect(port, 1_000_000, 100, false, false)
        .expect("open servo port");

    // Ping ID 20
    match servo.ping(20) {
        Ok(true) => eprintln!("ID 20: ping OK"),
        Ok(false) => { eprintln!("ID 20: no response"); return; }
        Err(e) => { eprintln!("ID 20: ping error: {}", e); return; }
    }

    // Read all expected registers
    for &(name, want) in &[("baud", 0u8), ("resp-level", 1), ("unload-cond", 7)] {
        let reg = registers::find_register(name).unwrap();
        match servo.read_reg(20, &reg) {
            Ok(v) => eprintln!("  {} = {} (expected {})", name, v, want),
            Err(e) => eprintln!("  {} read error: {}", name, e),
        }
    }

    eprintln!("Done.");
}
