//! The app icon: a rounded square in the accent colour with a white bolt, as
//! in the mockup. The tray and the window draw it at start; `build.rs`
//! includes this file to build the exe's `.ico` (no crate imports here).

/// The Windows default accent colour, blue `#0078D4`.
pub const DEFAULT_ACCENT: [u8; 3] = [0x00, 0x78, 0xD4];

/// The icon at `size` by `size` pixels, as RGBA bytes, row by row from the top.
#[must_use]
pub fn pixels([red, green, blue]: [u8; 3], size: u16) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(usize::from(size) * usize::from(size) * 4);
    for row in 0..size {
        for column in 0..size {
            // Centre of the pixel, in the mockup's 20-unit icon space.
            let x = (f32::from(column) + 0.5) * 20.0 / f32::from(size);
            let y = (f32::from(row) + 0.5) * 20.0 / f32::from(size);
            let pixel = if in_bolt(x, y) {
                [0xFF, 0xFF, 0xFF, 0xFF]
            } else if in_rounded_square(x, y) {
                [red, green, blue, 0xFF]
            } else {
                [0, 0, 0, 0]
            };
            pixels.extend(pixel);
        }
    }
    pixels
}

/// An `.ico` file with the icon at each of `sizes` (1 to 256 pixels), as
/// 32-bit bitmaps.
///
/// # Panics
///
/// If a size is outside 1 to 256: the file would be broken. Only the build
/// calls this.
#[must_use]
pub fn ico(accent: [u8; 3], sizes: &[u16]) -> Vec<u8> {
    assert!(
        sizes.iter().all(|size| (1..=256).contains(size)),
        "icon sizes are 1 to 256"
    );
    let images: Vec<Vec<u8>> = sizes.iter().map(|&size| bitmap(accent, size)).collect();
    let count = u16::try_from(sizes.len()).unwrap_or(u16::MAX);
    let mut file = Vec::new();
    for half in [0, 1, count] {
        file.extend(u16::to_le_bytes(half)); // Reserved, type icon, count.
    }
    let mut offset = 6 + 16 * u32::from(count);
    for (&size, image) in sizes.iter().zip(&images) {
        let length = u32::try_from(image.len()).unwrap_or(u32::MAX);
        // 256 is written as 0 in the one-byte width and height.
        let edge = u8::try_from(size).unwrap_or(0);
        file.extend([edge, edge, 0, 0]); // No palette, reserved.
        file.extend(u16::to_le_bytes(1)); // Planes.
        file.extend(u16::to_le_bytes(32)); // Bits per pixel.
        file.extend(length.to_le_bytes());
        file.extend(offset.to_le_bytes());
        offset = offset.saturating_add(length);
    }
    for image in images {
        file.extend(image);
    }
    file
}

/// One `.ico` image: a bitmap header, BGRA rows from the bottom, then an
/// all-clear mask (the alpha decides).
fn bitmap(accent: [u8; 3], size: u16) -> Vec<u8> {
    let edge = u32::from(size);
    let mut image = Vec::new();
    image.extend(40_u32.to_le_bytes()); // Header size.
    image.extend(edge.to_le_bytes());
    image.extend((2 * edge).to_le_bytes()); // The colours and the mask.
    image.extend(u16::to_le_bytes(1)); // Planes.
    image.extend(u16::to_le_bytes(32)); // Bits per pixel.
    image.extend([0; 24]); // No compression; the rest unused.
    let rows = pixels(accent, size);
    // At least 1, as a chunk of 0 bytes panics; a size of 0 has no rows.
    for row in rows.chunks_exact(usize::from(size.max(1)) * 4).rev() {
        for &[red, green, blue, alpha] in row.as_chunks::<4>().0 {
            image.extend([blue, green, red, alpha]);
        }
    }
    // One bit per pixel, each row padded to 4 bytes.
    let mask_row = usize::from(size).div_ceil(32) * 4;
    image.resize(image.len() + mask_row * usize::from(size), 0);
    image
}

/// The square from 1 to 19 with corners of radius 5.
fn in_rounded_square(x: f32, y: f32) -> bool {
    let (low, high, radius) = (1.0, 19.0, 5.0);
    let cx = x.clamp(low + radius, high - radius);
    let cy = y.clamp(low + radius, high - radius);
    (low..=high).contains(&x)
        && (low..=high).contains(&y)
        && (x - cx).powi(2) + (y - cy).powi(2) <= radius * radius
}

/// The bolt from the mockup's icon, by the even-odd rule.
fn in_bolt(x: f32, y: f32) -> bool {
    const BOLT: [(f32, f32); 6] = [
        (11.0, 2.5),
        (4.5, 11.0),
        (10.0, 11.0),
        (9.0, 17.5),
        (15.5, 9.0),
        (10.0, 9.0),
    ];
    let mut inside = false;
    for (i, &(x1, y1)) in BOLT.iter().enumerate() {
        let (x2, y2) = BOLT[(i + 1) % BOLT.len()];
        if (y1 > y) != (y2 > y) && x < x1 + (y - y1) * (x2 - x1) / (y2 - y1) {
            inside = !inside;
        }
    }
    inside
}
