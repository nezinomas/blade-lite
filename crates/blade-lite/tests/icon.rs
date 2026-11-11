//! The app icon: the tray's rounded square with a white bolt, as pixels and
//! as an `.ico` file for the exe.

use blade_lite::icon::{ico, pixels};

const BLUE: [u8; 3] = [0x00, 0x78, 0xD4];

/// The RGBA pixel at `(column, row)` of a `size`-wide picture.
fn at(picture: &[u8], size: usize, column: usize, row: usize) -> [u8; 4] {
    let start = (row * size + column) * 4;
    picture[start..start + 4].try_into().expect("4 bytes")
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"))
}

fn index(value: u32) -> usize {
    usize::try_from(value).expect("fits")
}

#[test]
fn it_is_a_square_of_rgba_pixels() {
    assert_eq!(pixels(BLUE, 32).len(), 32 * 32 * 4);
    assert_eq!(pixels(BLUE, 64).len(), 64 * 64 * 4);
}

#[test]
fn the_corners_are_clear_the_square_is_the_accent_and_the_bolt_is_white() {
    let picture = pixels(BLUE, 32);
    assert_eq!(at(&picture, 32, 0, 0), [0, 0, 0, 0]);
    assert_eq!(at(&picture, 32, 31, 31), [0, 0, 0, 0]);
    // Left middle edge: inside the square, left of the bolt.
    assert_eq!(at(&picture, 32, 3, 16), [0x00, 0x78, 0xD4, 0xFF]);
    // The bolt's middle bar.
    assert_eq!(at(&picture, 32, 16, 16), [0xFF, 0xFF, 0xFF, 0xFF]);
}

#[test]
fn a_bigger_icon_is_the_same_picture() {
    let small = pixels(BLUE, 32);
    let big = pixels(BLUE, 64);
    for (column, row) in [(0, 0), (3, 16), (16, 16), (10, 4), (25, 28)] {
        assert_eq!(
            at(&small, 32, column, row),
            at(&big, 64, column * 2 + 1, row * 2 + 1),
            "at ({column}, {row})"
        );
    }
}

#[test]
fn the_ico_file_lists_each_size() {
    let file = ico(BLUE, &[16, 256]);
    assert_eq!(u16_at(&file, 0), 0, "reserved");
    assert_eq!(u16_at(&file, 2), 1, "type: icon");
    assert_eq!(u16_at(&file, 4), 2, "count");
    // 256 is written as 0 in the one-byte width and height.
    assert_eq!([file[6], file[7]], [16, 16]);
    assert_eq!([file[22], file[23]], [0, 0]);
    for entry in [6, 22] {
        assert_eq!(u16_at(&file, entry + 4), 1, "planes");
        assert_eq!(u16_at(&file, entry + 6), 32, "bits per pixel");
    }
    let (first_size, first_at) = (u32_at(&file, 14), u32_at(&file, 18));
    let (second_size, second_at) = (u32_at(&file, 30), u32_at(&file, 34));
    assert_eq!(first_at, 6 + 2 * 16);
    assert_eq!(second_at, first_at + first_size);
    assert_eq!(index(second_at + second_size), file.len());
}

#[test]
fn each_image_is_a_bottom_up_bgra_bitmap_with_a_clear_mask() {
    let size = 16;
    let file = ico(BLUE, &[size]);
    let image = &file[index(u32_at(&file, 18))..];
    assert_eq!(u32_at(image, 0), 40, "header size");
    assert_eq!(u32_at(image, 4), 16, "width");
    assert_eq!(u32_at(image, 8), 32, "height: the colours and the mask");
    assert_eq!(u16_at(image, 12), 1, "planes");
    assert_eq!(u16_at(image, 14), 32, "bits per pixel");
    assert_eq!(u32_at(image, 16), 0, "no compression");

    let colours = &image[40..40 + 16 * 16 * 4];
    let picture = pixels(BLUE, size);
    for row in 0..16 {
        for column in 0..16 {
            let [r, g, b, a] = at(&picture, 16, column, row);
            let stored = at(colours, 16, column, 15 - row);
            assert_eq!(stored, [b, g, r, a], "at ({column}, {row})");
        }
    }
    // One bit per pixel, rows padded to 4 bytes, all zero: alpha decides.
    let mask = &image[40 + 16 * 16 * 4..];
    assert_eq!(mask, [0; 16 * 4]);
}
