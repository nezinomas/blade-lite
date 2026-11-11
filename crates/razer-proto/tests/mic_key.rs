use razer_proto::{
    BLADE_18, ModelProfile, RED_KEY, RedKey, RedKeys, Rgb, static_colour, static_colour_red,
};

const GREEN: Rgb = Rgb::new(0x44, 0xD6, 0x2C);
const MIC: RedKeys = RedKeys {
    touchpad: false,
    mic: true,
};

/// The colour of one key in a frame's commands.
fn colour_at(commands: &[razer_proto::Command], row: u8, column: u8) -> Rgb {
    let args = &commands[1 + usize::from(row)].args;
    let at = 4 + 3 * usize::from(column);
    Rgb::new(args[at], args[at + 1], args[at + 2])
}

/// The keys red in `frame` that are not red in the plain frame.
fn red_keys(frame: &[razer_proto::Command]) -> Vec<(u8, u8)> {
    let plain = static_colour(GREEN, &BLADE_18);
    let mut red = Vec::new();
    for row in 0..BLADE_18.keys.rows {
        for column in 0..BLADE_18.keys.columns {
            if colour_at(frame, row, column) != colour_at(&plain, row, column) {
                assert_eq!(colour_at(frame, row, column), RED_KEY);
                red.push((row, column));
            }
        }
    }
    red
}

#[test]
fn the_blade_18_mic_key_is_numpad_0() {
    assert_eq!(
        BLADE_18.mic_key,
        Some(RedKey {
            row: 5,
            column: 16,
            plain_with_num_lock: true,
        })
    );
}

#[test]
fn only_the_mic_key_turns_red() {
    let frame = static_colour_red(GREEN, &BLADE_18, MIC);

    assert_eq!(frame.len(), static_colour(GREEN, &BLADE_18).len());
    assert_eq!(red_keys(&frame), [(5, 16)]);
}

#[test]
fn both_keys_can_be_red_at_once() {
    let both = RedKeys {
        touchpad: true,
        mic: true,
    };

    assert_eq!(
        red_keys(&static_colour_red(GREEN, &BLADE_18, both)),
        [(5, 16), (5, 17)]
    );
}

#[test]
fn no_red_key_is_the_plain_frame() {
    assert_eq!(
        static_colour_red(GREEN, &BLADE_18, RedKeys::default()),
        static_colour(GREEN, &BLADE_18)
    );
}

#[test]
fn a_model_without_the_mic_key_gets_the_plain_frame() {
    let no_key = ModelProfile {
        mic_key: None,
        ..BLADE_18
    };

    assert_eq!(
        static_colour_red(GREEN, &no_key, MIC),
        static_colour(GREEN, &no_key)
    );
}

#[test]
fn a_mic_key_outside_the_grid_or_on_a_gap_is_not_painted() {
    for (row, column) in [(7, 0), (0, 19), (5, 4)] {
        let odd = ModelProfile {
            mic_key: Some(RedKey {
                row,
                column,
                plain_with_num_lock: false,
            }),
            ..BLADE_18
        };

        assert_eq!(
            static_colour_red(GREEN, &odd, MIC),
            static_colour(GREEN, &odd),
            "({row}, {column})"
        );
    }
}
