//! The Windows readings, over fake sensors.

use std::cell::Cell;
use std::io;

use blade_core::{Readings, ReadingsSampler};
use platform::{CpuTimes, GpuReading, PowerReading, Sensors};

/// Sensor values to hand out. `None` means that read fails.
struct FakeSensors {
    /// Handed out in turn, one per read.
    cpu: Vec<Option<CpuTimes>>,
    power: Option<PowerReading>,
    gpu_awake: bool,
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
            }),
            gpu_awake: true,
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

    fn gpu_awake(&mut self) -> io::Result<bool> {
        Ok(self.gpu_awake)
    }

    fn gpu(&mut self) -> io::Result<GpuReading> {
        self.gpu_reads.set(self.gpu_reads.get() + 1);
        failed(self.gpu)
    }
}

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
        }),
        ..FakeSensors::new()
    };

    assert_eq!(second_snapshot(&mut sensors).power_draw_mw, None);
}

#[test]
fn failed_reads_leave_their_values_empty() {
    let mut sensors = FakeSensors {
        cpu: vec![Some(times(0, 0)), None],
        power: None,
        gpu: None,
        ..FakeSensors::new()
    };

    assert_eq!(second_snapshot(&mut sensors), Readings::default());
}

#[test]
fn no_cpu_time_passing_gives_no_load() {
    let mut sensors = FakeSensors {
        cpu: vec![Some(times(10, 100)), Some(times(10, 100))],
        ..FakeSensors::new()
    };

    assert_eq!(second_snapshot(&mut sensors).cpu_load_percent, None);
}
