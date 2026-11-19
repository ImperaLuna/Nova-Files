use gpui::Pixels;

pub fn device_side(side: Pixels, scale_factor: f32) -> u32 {
    let device = (f32::from(side) * scale_factor).ceil();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to a small positive size first"
    )]
    let device = device.clamp(1.0, f32::from(u16::MAX)) as u32;
    device
}

#[cfg(test)]
mod tests {
    use gpui::px;

    use super::*;

    #[test]
    fn device_sides_round_up() {
        assert_eq!(device_side(px(24.0), 1.0), 24);
        assert_eq!(device_side(px(24.0), 1.25), 30);
        assert_eq!(device_side(px(24.0), 1.1), 27);
    }
}
