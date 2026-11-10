//! The real sensors. Reads only.

use platform::{Sensors, WindowsSensors};

#[test]
#[ignore = "needs a laptop with a battery"]
fn the_cpu_and_battery_are_read() {
    let mut sensors = WindowsSensors::default();

    let first = sensors.cpu_times().unwrap();
    let second = sensors.cpu_times().unwrap();
    let power = sensors.power().unwrap();
    eprintln!("cpu {first:?} then {second:?}, power {power:?}");

    assert!(second.total >= first.total && second.idle >= first.idle);
    assert!(first.idle <= first.total);
    let percent = power.battery_percent.expect("the battery charge is known");
    assert!(percent <= 100);
    assert!(power.charging.is_some(), "the charging flag is known");
}

#[test]
#[ignore = "needs a laptop with an NVIDIA GPU"]
fn the_gpu_is_read_only_when_awake() {
    let mut sensors = WindowsSensors::default();

    let awake = sensors.gpu_awake().unwrap();
    eprintln!("GPU awake: {awake}");
    if awake {
        let gpu = sensors.gpu().unwrap();
        eprintln!("GPU {gpu:?}");
        assert!((20..=105).contains(&gpu.celsius), "{gpu:?}");
        assert!(gpu.load_percent <= 100, "{gpu:?}");
    }
    // The check itself must not have woken a sleeping GPU.
    assert_eq!(sensors.gpu_awake().unwrap(), awake);
}
