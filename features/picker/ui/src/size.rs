use gpui::{Pixels, Size, px, size};
use nova_files_api::WindowState as Keyed;
use nova_files_shell::theme;

pub const PICKER: &str = "picker";
const BY: char = 'x';
const MIN_WIDTH: Pixels = px(400.0);
const MIN_HEIGHT: Pixels = px(300.0);

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PickerState {
    picker: Option<(f32, f32)>,
}

impl PickerState {
    pub fn parse(bytes: &[u8]) -> Self {
        let state = Keyed::parse(bytes);
        Self {
            picker: state.get(PICKER).and_then(parse_size),
        }
    }

    pub fn value(&self) -> Option<String> {
        self.picker
            .map(|(width, height)| format!("{width}{BY}{height}"))
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut state = Keyed::default();
        if let Some(value) = self.value() {
            state.set(PICKER, &value);
        }
        state.to_bytes()
    }

    pub fn picker_size(&self, display: Option<Size<Pixels>>) -> Size<Pixels> {
        let Some((width, height)) = self.picker else {
            return size(theme::window::PICK_WIDTH, theme::window::PICK_HEIGHT);
        };
        let mut width = px(width).max(MIN_WIDTH);
        let mut height = px(height).max(MIN_HEIGHT);
        if let Some(display) = display {
            width = width.min(display.width);
            height = height.min(display.height);
        }
        size(width, height)
    }

    #[must_use]
    pub fn with_picker(mut self, picker: Size<Pixels>) -> Self {
        let whole = |pixels: Pixels| f32::from(pixels).round();
        self.picker = Some((whole(picker.width), whole(picker.height)));
        self
    }
}

fn parse_size(value: &str) -> Option<(f32, f32)> {
    let (width, height) = value.split_once(BY)?;
    let width: f32 = width.parse().ok()?;
    let height: f32 = height.parse().ok()?;
    let fits = |side: f32| side.is_finite() && side > 0.0;
    (fits(width) && fits(height)).then_some((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_reads_back_what_it_wrote() {
        let state = PickerState::parse(b"nova-files window-state 1\n\npicker 900x600\n");
        assert_eq!(state.picker, Some((900.0, 600.0)));
        let written = state.with_picker(size(px(1000.4), px(700.6))).to_bytes();
        assert_eq!(written, b"nova-files window-state 1\npicker 1000x701\n");
        assert_eq!(PickerState::parse(&written).picker, Some((1000.0, 701.0)));
        assert_eq!(
            PickerState::default().to_bytes(),
            b"nova-files window-state 1\n"
        );
    }

    #[test]
    fn anything_else_reads_as_no_state() {
        for bytes in [
            &b""[..],
            b"picker 900x600\n",
            b"nova-files window-state 2\npicker 900x600\n",
            b"nova-files window-state 1\npicker\n",
            b"\xff\xfe",
        ] {
            assert_eq!(
                PickerState::parse(bytes),
                PickerState::default(),
                "{bytes:?}"
            );
        }
        for value in [
            "900", "900x", "x600", "0x600", "900x0", "-1x600", "infx600", "NaNx600", "ninexsix",
        ] {
            assert_eq!(parse_size(value), None, "{value}");
        }
    }

    #[test]
    fn a_picker_opens_at_its_saved_size_within_bounds_or_the_themes() {
        let theme = size(theme::window::PICK_WIDTH, theme::window::PICK_HEIGHT);
        assert_eq!(PickerState::default().picker_size(None), theme);
        let saved = |width, height| PickerState {
            picker: Some((width, height)),
        };
        assert_eq!(
            saved(1000.0, 700.0).picker_size(None),
            size(px(1000.0), px(700.0))
        );
        assert_eq!(
            saved(10.0, 10.0).picker_size(None),
            size(MIN_WIDTH, MIN_HEIGHT)
        );
        let display = size(px(1280.0), px(720.0));
        assert_eq!(saved(5000.0, 5000.0).picker_size(Some(display)), display);
        assert_eq!(
            saved(1000.0, 700.0).picker_size(Some(display)),
            size(px(1000.0), px(700.0))
        );
    }
}
