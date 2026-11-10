//! Whether the charger gives full power. A weak charge, such as a USB-C
//! monitor that powers the laptop, runs only the battery presets, as Synapse
//! does.

use razer_device::DeviceError;
use razer_proto::{Charger, ModelProfile};

use crate::Query;

/// Whether the model's own full power charger powers the laptop. A model
/// without the read counts any charger as full power. `None` if the reply
/// is not understood; the caller takes the safe side.
///
/// # Errors
///
/// See [`DeviceError`].
pub fn full_power_charger(
    controller: &mut impl Query,
    model: &ModelProfile,
) -> Result<Option<bool>, DeviceError> {
    let Some(charger) = model.charger else {
        return Ok(Some(true));
    };
    let args = controller.query(&Charger::query())?;
    Ok(charger.full_power(&args))
}
