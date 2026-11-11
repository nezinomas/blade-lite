//! The log text of a device error.

use blade_lite::AppError;
use razer_device::DeviceError;

#[test]
fn a_device_error_does_not_say_the_controller_did_not_answer() {
    // The controller did answer a short reply: the prefix must fit every kind.
    let text = AppError::from(DeviceError::ShortReply).to_string();

    assert_eq!(
        text,
        "the controller call failed: the reply has too few args"
    );
}
