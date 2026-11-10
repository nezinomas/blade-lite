//! The Windows readings, over fake sensors.

use std::cell::Cell;
use std::io;

use blade_core::{HEALTH_EVERY, Readings, ReadingsSampler};
use platform::{BatteryHealth, CpuTimes, GpuReading, PowerReading, Sensors};

/// Sensor values to hand out. `None` means that read fails.
struct FakeSensors {
    /// Handed out in turn, one per read.
    cpu: Vec<Option<CpuTimes>>,
    power: Option<PowerReading>,
    health: Option<BatteryHealth>,
    /// How many times the battery health was read.
    health_reads: Cell<usize>,
    gpu_awake: bool,
    gpu_awake_fails: bool,
    gpu: Option<GpuReading>,
    /// How many times the GPU itself was read.
    gpu_reads: Cell<usize>,
}

impl FakeSensors {
    fn new() -> Self {
        Self {
            cpu: vec![Some(times(0, 0)), Some(times(25, 100))],
            power: Some(PowerReading {
                battery_percent: Some(80),
                battery_rate_mw: Some(-45_000),
                charging: Some(false),
            }),
            health: Some(HEALTH),
            health_reads: Cell::new(0),
            gpu_awake: true,
            gpu_awake_fails: false,
            gpu: Some(GpuReading {
                celsius: 61,
                load_percent: 37,
            }),
            gpu_reads: Cell::new(0),
        }
    }
}

fn failed<T>(value: Option<T>) -> io::Result<T> {
    value.ok_or(io::ErrorKind::Other.into())
}

impl Sensors for FakeSensors {
    fn cpu_times(&mut self) -> io::Result<CpuTimes> {
        failed(self.cpu.remove(0))
    }

    fn power(&mut self) -> io::Result<PowerReading> {
        failed(self.power)
    }

    fn battery_health(&mut self) -> io::Result<BatteryHealth> {
        self.health_reads.set(self.health_reads.get() + 1);
        failed(self.health)
    }

    fn gpu_awake(&mut self) -> io::Result<bool> {
        if self.gpu_awake_fails {
            return Err(io::ErrorKind::Other.into());
        }
        Ok(self.gpu_awake)
    }

    fn gpu(&mut self) -> io::Result<GpuReading> {
        self.gpu_reads.set(self.gpu_reads.get() + 1);
        failed(self.gpu)
    }
}

const HEALTH: BatteryHealth = BatteryHealth {
    percent: Some(99),
    cycles: Some(86),
};

const fn times(idle: u64, total: u64) -> CpuTimes {
    CpuTimes { idle, total }
}

/// The second snapshot: the first only gives the CPU a starting point.
fn second_snapshot(sensors: &mut FakeSensors) -> Readings {
    let mut sampler = ReadingsSampler::default();
    sampler.sample(sensors);
    sampler.sample(sensors)
}

#[test]
fn the_sensors_make_a_snapshot() {
    let mut sensors = FakeSensors::new();

    let readings = second_snapshot(&mut sensors);

    assert_eq!(
        readings,
        Readings {
            cpu_load_percent: Some(75),
            battery_percent: Some(80),
            power_draw_mw: Some(45_000),
            charging: Some(false),
            battery_health: HEALTH,
            gpu: Some(GpuReading {
                celsius: 61,
                load_percent: 37,
            }),
        }
    );
}

#[test]
fn the_first_snapshot_has_no_cpu_load() {
    let mut sensors = FakeSensors::new();

    let readings = ReadingsSampler::default().sample(&mut sensors);

    assert_eq!(readings.cpu_load_percent, None);
}

#[test]
fn the_gpu_is_skipped_while_it_is_asleep() {
    let mut sensors = FakeSensors {
        gpu_awake: false,
        ..FakeSensors::new()
    };

    let readings = second_snapshot(&mut sensors);

    assert_eq!(readings.gpu, None);
    assert_eq!(sensors.gpu_reads.get(), 0, "the GPU was read");
}

#[test]
fn charging_has_no_power_draw() {
    let mut sensors = FakeSensors {
        power: Some(PowerReading {
            battery_percent: Some(80),
            battery_rate_mw: Some(20_000),
            charging: Some(true),
        }),
        ..FakeSensors::new()
    };

    let readings = second_snapshot(&mut sensors);

    assert_eq!(readings.power_draw_mw, None);
    assert_eq!(readings.charging, Some(true));
}

#[test]
fn failed_reads_leave_their_values_empty() {
    let mut sensors = FakeSensors {
        cpu: vec![Some(times(0, 0)), None],
        power: None,
        health: None,
        gpu: None,
        ..FakeSensors::new()
    };

    assert_eq!(second_snapshot(&mut sensors), Readings::default());
}

#[test]
fn each_failed_read_is_named_in_the_errors() {
    let mut sensors = FakeSensors {
        cpu: vec![Some(times(0, 0)), None],
        power: None,
        health: None,
        gpu: None,
        ..FakeSensors::new()
    };
    let mut sampler = ReadingsSampler::default();

    sampler.sample(&mut sensors);
    let errors = sampler.errors();
    assert_eq!(errors.len(), 3, "{errors:?}");
    assert!(errors[0].starts_with("battery: "));
    assert!(errors[1].starts_with("battery health: "));
    assert!(errors[2].starts_with("GPU: "));

    sampler.sample(&mut sensors);
    let errors = sampler.errors();
    assert_eq!(errors.len(), 3, "{errors:?}");
    assert!(errors[0].starts_with("battery: "));
    assert!(errors[1].starts_with("CPU load: "));
    assert!(errors[2].starts_with("GPU: "));
}

#[test]
fn the_battery_health_is_read_once_per_health_every_snapshots() {
    let mut sensors = FakeSensors::new();
    sensors.cpu = vec![None; 2 * HEALTH_EVERY + 1];
    let mut sampler = ReadingsSampler::default();

    let first = sampler.sample(&mut sensors);
    sensors.health = Some(BatteryHealth {
        percent: Some(98),
        cycles: Some(87),
    });
    for _ in 1..HEALTH_EVERY {
        assert_eq!(sampler.sample(&mut sensors).battery_health, HEALTH);
    }
    let next = sampler.sample(&mut sensors);

    assert_eq!(first.battery_health, HEALTH);
    assert_eq!(next.battery_health.cycles, Some(87));
    assert_eq!(sensors.health_reads.get(), 2);
}

#[test]
fn a_failed_health_read_keeps_the_last_health() {
    let mut sensors = FakeSensors::new();
    sensors.cpu = vec![None; HEALTH_EVERY + 1];
    let mut sampler = ReadingsSampler::default();
    sampler.sample(&mut sensors);
    sensors.health = None;
    for _ in 1..HEALTH_EVERY {
        sampler.sample(&mut sensors);
    }

    let readings = sampler.sample(&mut sensors);

    assert_eq!(readings.battery_health, HEALTH);
    assert!(
        sampler
            .errors()
            .iter()
            .any(|e| e.starts_with("battery health: ")),
        "{:?}",
        sampler.errors()
    );
}

#[test]
fn a_good_snapshot_clears_the_errors() {
    let mut sensors = FakeSensors {
        cpu: vec![None, Some(times(0, 0))],
        ..FakeSensors::new()
    };
    let mut sampler = ReadingsSampler::default();
    sampler.sample(&mut sensors);
    assert_eq!(sampler.errors().len(), 1);

    sampler.sample(&mut sensors);

    assert!(sampler.errors().is_empty(), "{:?}", sampler.errors());
}

#[test]
fn a_failed_gpu_power_state_is_an_error() {
    let mut sensors = FakeSensors {
        gpu_awake_fails: true,
        ..FakeSensors::new()
    };
    let mut sampler = ReadingsSampler::default();

    sampler.sample(&mut sensors);

    assert_eq!(sampler.errors().len(), 1);
    assert!(sampler.errors()[0].starts_with("GPU power state: "));
}

#[test]
fn no_cpu_time_passing_gives_no_load() {
    let mut sensors = FakeSensors {
        cpu: vec![Some(times(10, 100)), Some(times(10, 100))],
        ..FakeSensors::new()
    };

    assert_eq!(second_snapshot(&mut sensors).cpu_load_percent, None);
}

#[test]
fn the_gpu_temperature_alone_is_read_when_it_is_awake() {
    let mut sensors = FakeSensors::new();
    sensors.cpu.clear();
    sensors.power = None;
    sensors.health = None;
    let mut sampler = ReadingsSampler::default();

    assert_eq!(sampler.gpu_celsius(&mut sensors), Some(61));
    assert_eq!(
        sampler.errors(),
        [] as [String; 0],
        "no CPU or battery read"
    );
}

#[test]
fn a_sleeping_gpu_is_not_woken_for_its_temperature() {
    let mut sensors = FakeSensors::new();
    sensors.gpu_awake = false;
    let mut sampler = ReadingsSampler::default();

    assert_eq!(sampler.gpu_celsius(&mut sensors), None);
    assert_eq!(sensors.gpu_reads.get(), 0);
}

#[test]
fn a_failed_gpu_temperature_read_is_named_alone() {
    let mut sensors = FakeSensors::new();
    sensors.gpu = None;
    let mut sampler = ReadingsSampler::default();
    sampler.sample(&mut FakeSensors {
        power: None,
        ..FakeSensors::new()
    });

    assert_eq!(sampler.gpu_celsius(&mut sensors), None);
    assert_eq!(sampler.errors().len(), 1, "{:?}", sampler.errors());
    assert!(sampler.errors()[0].starts_with("GPU: "));
}
