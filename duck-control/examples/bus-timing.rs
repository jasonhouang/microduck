use std::time::Instant;

fn main() {
    let port = "/dev/ttyS2";
    eprintln!("Opening {} at 1Mbps...", port);

    let mut servo = duck_control::feetech::servo::Servo::connect(port, 1_000_000, 5, false, false)
        .expect("open servo port");

    // Warm up
    let _ = servo.read(20, 56, 6);

    // Time 50 reads
    let t0 = Instant::now();
    let mut ok = 0;
    for _ in 0..50 {
        match servo.read(20, 56, 6) {
            Ok(_) => ok += 1,
            Err(e) => eprintln!("  read error: {}", e),
        }
    }
    let elapsed = t0.elapsed();
    eprintln!(
        "Serial: {}/50 responded, {:.0}ms total ({:.1}ms each)",
        ok,
        elapsed.as_secs_f64() * 1000.0,
        elapsed.as_secs_f64() * 1000.0 / 50.0
    );

    // Time 50 reads on all 15 servos (simulating one full tick)
    let ids: [u8; 15] = [20, 21, 22, 23, 24, 30, 31, 32, 33, 34, 10, 11, 12, 13, 14];
    let t0 = Instant::now();
    let mut ticks_ok = 0;
    for tick in 0..20 {
        let tick_start = Instant::now();
        let mut all_ok = true;
        for &id in &ids {
            if servo.read(id, 56, 6).is_err() {
                all_ok = false;
            }
        }
        if all_ok {
            ticks_ok += 1;
        }
        if tick < 3 {
            eprintln!(
                "  tick {}: {:.1}ms",
                tick,
                tick_start.elapsed().as_secs_f64() * 1000.0
            );
        }
    }
    let elapsed = t0.elapsed();
    eprintln!(
        "Full ticks: {}/20 OK, {:.0}ms total ({:.1}ms each)",
        ticks_ok,
        elapsed.as_secs_f64() * 1000.0,
        elapsed.as_secs_f64() * 1000.0 / 20.0
    );
}
