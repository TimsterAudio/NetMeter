//! The window icon, drawn in code: a black badge with the panel stripe, and
//! a scrolling download graph with the upload line over it.

use egui::IconData;

pub fn icon() -> IconData {
    const S: usize = 64;
    let mut rgba = vec![0u8; S * S * 4];
    let stripe = [
        [0xd8, 0x43, 0x2a],
        [0xec, 0x7b, 0x24],
        [0xef, 0xc2, 0x3b],
        [0xe7, 0xe0, 0xc9],
    ];
    // Download, from the left (oldest) to now, standing on y = 45.
    let down = |x: f32| 12.0 + 9.0 * (x * 0.21).sin() + 7.0 * (x * 0.53 + 1.0).sin().max(0.0);
    let up = |x: f32| 6.0 + 2.5 * (x * 0.37 + 2.0).sin();
    for y in 0..S {
        for x in 0..S {
            let i = (y * S + x) * 4;
            // Rounded square
            let (dx, dy) = (
                (x as f32 - 31.5).abs() - 24.0,
                (y as f32 - 31.5).abs() - 24.0,
            );
            let outside = dx.max(0.0).hypot(dy.max(0.0)) - 7.5;
            if outside > 0.5 {
                continue;
            }
            let mut c = [0x17, 0x17, 0x17];
            if (50..54).contains(&y) && (8..56).contains(&x) {
                c = stripe[((x - 8) / 12).min(3)];
            }
            if (8..56).contains(&x) && y < 45 {
                let fx = x as f32;
                let top = 45.0 - down(fx);
                if y as f32 >= top {
                    c = if (y as f32) < top + 2.0 {
                        [0xff, 0x7a, 0x4d]
                    } else {
                        [0xb8, 0x3a, 0x22]
                    };
                }
                if (y as f32 - (45.0 - up(fx))).abs() < 1.0 {
                    c = [0xe7, 0xe0, 0xc9];
                }
            }
            rgba[i..i + 3].copy_from_slice(&c);
            rgba[i + 3] = ((0.5 - outside).clamp(0.0, 1.0) * 255.0) as u8;
        }
    }
    IconData {
        rgba,
        width: S as u32,
        height: S as u32,
    }
}
