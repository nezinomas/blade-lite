mod common;

use common::{SUCCESS, fakes, reply, request, sent};
use razer_device::Device;
use razer_proto::{BLADE_18, Brightness};

const SET_BRIGHTNESS: u16 = 0x0303;

#[test]
fn every_command_is_sent_in_order() {
    let (transport, clock, log) = fakes(vec![
        reply(SUCCESS, 0, SET_BRIGHTNESS),
        reply(SUCCESS, 1, SET_BRIGHTNESS),
    ]);

    let brightness = Brightness::new(50).unwrap();
    let mut commands = brightness.commands(&BLADE_18);
    commands.push(BLADE_18.vapor_light.unwrap().command(brightness, true));
    Device::new(transport, clock).run(&commands).unwrap();

    assert_eq!(
        sent(&log),
        [
            request(0, SET_BRIGHTNESS, &[0x01, 0x05, 128]),
            request(1, SET_BRIGHTNESS, &[0x01, 0x26, 128]),
        ]
    );
}

#[test]
fn a_failed_command_stops_the_rest() {
    let (transport, clock, log) = fakes(vec![reply(common::FAILURE, 0, SET_BRIGHTNESS)]);

    let brightness = Brightness::new(50).unwrap();
    let mut commands = brightness.commands(&BLADE_18);
    commands.push(BLADE_18.vapor_light.unwrap().command(brightness, true));
    let result = Device::new(transport, clock).run(&commands);

    assert!(result.is_err());
    assert_eq!(sent(&log).len(), 1);
}
