/// Preserve hue while ensuring readable syntax and icons on pale surfaces.
pub fn foreground(r: u8, g: u8, b: u8, light: bool) -> slint::Color {
    let mut rgb = [r, g, b];
    if light {
        while luminance(rgb) > 0.055 {
            rgb = rgb.map(|channel| (f32::from(channel) * 0.94) as u8);
        }
    } else {
        // Include the selected range surface, not only the editor canvas.
        let minimum = (luminance([35, 57, 78]) + 0.05) * 4.5 - 0.05;
        while luminance(rgb) < minimum {
            rgb = rgb.map(|channel| {
                channel.saturating_add(((255 - u16::from(channel)) / 16).max(1) as u8)
            });
        }
    }
    slint::Color::from_rgb_u8(rgb[0], rgb[1], rgb[2])
}

fn luminance(rgb: [u8; 3]) -> f32 {
    let linear = rgb.map(|channel| {
        let v = f32::from(channel) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    });
    linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722
}

/// Deliberate light-theme hues retain recognizable file-type colors.
pub fn icon_foreground(r: u8, g: u8, b: u8, light: bool) -> slint::Color {
    if !light {
        return slint::Color::from_rgb_u8(r, g, b);
    }
    let rgb = if b > r && b > g {
        [27, 82, 164]
    } else if g > r && b > r {
        [0, 101, 120]
    } else if g > r {
        [17, 109, 62]
    } else if r > b && g > b && r.abs_diff(g) < 75 {
        [117, 82, 0]
    } else if r > g && b > g {
        [112, 56, 158]
    } else {
        [159, 64, 26]
    };
    slint::Color::from_rgb_u8(rgb[0], rgb[1], rgb[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outdoor_colors_keep_contrast_on_white_and_selected_rows() {
        for r in (0..=255).step_by(17) {
            for g in (0..=255).step_by(17) {
                for b in (0..=255).step_by(17) {
                    let c = foreground(r, g, b, true);
                    for bg in [[255, 255, 255], [210, 228, 247], [228, 237, 246]] {
                        let ratio = (luminance(bg) + 0.05)
                            / (luminance([c.red(), c.green(), c.blue()]) + 0.05);
                        assert!(ratio >= 7.0, "Insufficient contrast: {ratio}");
                    }
                    let c = foreground(r, g, b, false);
                    for bg in [[19, 28, 38], [32, 50, 70], [35, 57, 78]] {
                        let ratio = (luminance([c.red(), c.green(), c.blue()]) + 0.05)
                            / (luminance(bg) + 0.05);
                        assert!(ratio >= 4.5, "Insufficient dark contrast: {ratio}");
                    }
                }
            }
        }
    }
}
