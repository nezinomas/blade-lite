use razer_proto::{
    BLADE_18, ModelProfile, RedKey, Rgb, TOUCHPAD_OFF, static_colour, static_colour_touchpad_off,
};

const GREEN: Rgb = Rgb::new(0x44, 0xD6, 0x2C);

/// The colour of one key in a frame's commands.
fn colour_at(commands: &[razer_proto::Command], row: u8, column: u8) -> Rgb {
    let args = &commands[1 + usize::from(row)].args;
    let at = 4 + 3 * usize::from(column);
    Rgb::new(args[at], args[at + 1], args[at + 2])
}

#[test]
fn the_blade_18_key_is_numpad_dot() {
    assert_eq!(
        BLADE_18.touchpad_key,
        Some(RedKey {
            row: 5,
            column: 17,
            plain_with_num_lock: true,
        })
    );
}

#[test]
fn only_the_touchpad_key_turns_red() {
    let plain = static_colour(GREEN, &BLADE_18);
    let off = static_colour_touchpad_off(GREEN, &BLADE_18);

    assert_eq!(off.len(), plain.len());
    assert_eq!(colour_at(&off, 5, 17), TOUCHPAD_OFF);
    for row in 0..BLADE_18.keys.rows {
        for column in 0..BLADE_18.keys.columns {
            if (row, column) != (5, 17) {
                assert_eq!(
                    colour_at(&off, row, column),
                    colour_at(&plain, row, column),
                    "({row}, {column})"
                );
            }
        }
    }
}

#[test]
fn a_model_without_the_key_gets_the_plain_frame() {
    let no_key = ModelProfile {
        touchpad_key: None,
        ..BLADE_18
    };

    assert_eq!(
        static_colour_touchpad_off(GREEN, &no_key),
        static_colour(GREEN, &no_key)
    );
}

#[test]
fn a_key_outside_the_grid_or_on_a_gap_is_not_painted() {
    for (row, column) in [(7, 0), (0, 19), (5, 4)] {
        let odd = ModelProfile {
            touchpad_key: Some(RedKey {
                row,
                column,
                plain_with_num_lock: false,
            }),
            ..BLADE_18
        };

        assert_eq!(
            static_colour_touchpad_off(GREEN, &odd),
            static_colour(GREEN, &odd),
            "({row}, {column})"
        );
    }
}
