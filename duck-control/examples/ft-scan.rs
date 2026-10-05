//! Scan the Feetech bus: ping every ID 1–253, report who answers.
//!
//! Usage: cargo run --example ft-scan -- --port /dev/ttyS2

use duck_control::feetech::servo::Servo;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let port = args
        .windows(2)
        .find(|w| w[0] == "--port")
        .map(|w| w[1].as_str())
        .unwrap_or("/dev/ttyS2");

    let baud: u32 = args
        .windows(2)
        .find(|w| w[0] == "--baud")
        .and_then(|w| w[1].parse().ok())
        .unwrap_or(1_000_000);

    let verbose = args.iter().any(|a| a == "--verbose" || a == "-v");

    eprintln!("scanning {} at {} bps …", port, baud);

    let mut servo = match Servo::connect(port, baud, 50, false, verbose) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot open {}: {}", port, e);
            std::process::exit(1);
        }
    };

    let mut found = Vec::new();
    for id in 1..=253u8 {
        match servo.ping(id) {
            Ok(true) => {
                eprintln!("  id {:>3}  ✓", id);
                found.push(id);
            }
            Ok(false) => {}
            Err(e) => eprintln!("  id {:>3}  ✗ {}", id, e),
        }
    }

    eprintln!();
    if found.is_empty() {
        eprintln!("no servos answered.");
    } else {
        eprintln!("found {} servo(s): {:?}", found.len(), found);
    }
}
