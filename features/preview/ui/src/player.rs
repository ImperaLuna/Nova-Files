use std::sync::Arc;
use std::time::Duration;

use gpui::{
    AnyElement, Context, Div, InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, ParentElement as _, Pixels, RenderImage, SharedString, Styled as _, div, img,
    px,
};
use nova_files_preview_api::{MAX_VOLUME, MediaTag, Played, PreviewId};
use nova_files_shell::assets::Icon;
use nova_files_shell::theme::{Colors, preview, radius, size, text};
use nova_files_shell::widgets::icon;

use crate::PreviewPart;
use crate::draw::{fitted, logical_side};

pub const JUMP: Duration = Duration::from_secs(10);
pub const BAR_HEIGHT: Pixels = px(44.0);
const BAR_MIN_WIDTH: Pixels = px(420.0);
const BAR_PAD: Pixels = px(8.0);
const BAR_GAP: Pixels = px(10.0);
const TRACK_HEIGHT: Pixels = px(20.0);
const LINE_HEIGHT: Pixels = px(4.0);
const VOLUME_WIDTH: Pixels = px(80.0);
const VOLUME_GAP: Pixels = px(1.0);
const SONG_COVER: Pixels = px(240.0);
const SONG_GAP: Pixels = px(18.0);
const TAG_GAP: Pixels = px(4.0);
const SEEK_STEPS: u16 = 160;
const VOLUME_STEPS: u8 = 10;
const SECONDS_IN_MINUTE: u64 = 60;
const MINUTES_IN_HOUR: u64 = 60;
const TIME_SEPARATOR: &str = " / ";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Paused,
    Playing,
    Ended,
    Unplayable,
}

pub struct Media {
    pub id: PreviewId,
    pub video: bool,
    pub frame: Option<Arc<RenderImage>>,
    pub at: Duration,
    pub length: Option<Duration>,
    pub state: State,
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    sought: Option<u16>,
}

impl Media {
    pub const fn new(id: PreviewId, video: bool) -> Self {
        Self {
            id,
            video,
            frame: None,
            at: Duration::ZERO,
            length: None,
            state: State::Paused,
            title: None,
            artist: None,
            album: None,
            sought: None,
        }
    }

    pub fn heard(&mut self, played: Played) {
        match played {
            Played::Frame(_) => {}
            Played::At(at) => self.at = at,
            Played::Length(length) => self.length = Some(length),
            Played::Paused(paused) => {
                self.state = match (self.state, paused) {
                    (State::Unplayable | State::Ended, true) => self.state,
                    (State::Unplayable, false) => State::Unplayable,
                    (_, true) => State::Paused,
                    (_, false) => State::Playing,
                };
            }
            Played::Tag(tag, words) => {
                let slot = match tag {
                    MediaTag::Title => &mut self.title,
                    MediaTag::Artist => &mut self.artist,
                    MediaTag::Album => &mut self.album,
                };
                *slot = Some(words).filter(|words| !words.is_empty());
            }
            Played::Ended => self.state = State::Ended,
            Played::Unplayable => self.state = State::Unplayable,
        }
    }

    pub fn jumped(&self, back: bool) -> Duration {
        let to = if back {
            self.at.saturating_sub(JUMP)
        } else {
            self.at.saturating_add(JUMP)
        };
        self.length.map_or(to, |length| to.min(length))
    }

    pub fn at_step(&mut self, step: u16) -> Option<Duration> {
        if self.sought == Some(step) {
            return None;
        }
        self.sought = Some(step);
        let length = self.length?;
        Some(length.mul_f64(f64::from(step) / f64::from(SEEK_STEPS - 1)))
    }

    pub const fn let_go(&mut self) {
        self.sought = None;
    }

    fn played_steps(&self) -> u16 {
        let Some(length) = self.length.filter(|length| !length.is_zero()) else {
            return 0;
        };
        let part = (self.at.as_secs_f64() / length.as_secs_f64()).clamp(0.0, 1.0);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a share from 0 to 1 of a few hundred steps"
        )]
        let steps = (part * f64::from(SEEK_STEPS)).round() as u16;
        steps
    }

    fn tags(&self) -> impl Iterator<Item = &str> {
        [&self.title, &self.artist, &self.album]
            .into_iter()
            .filter_map(|tag| tag.as_deref())
    }
}

pub fn clock(time: Duration) -> String {
    let seconds = time.as_secs();
    let (minutes, seconds) = (seconds / SECONDS_IN_MINUTE, seconds % SECONDS_IN_MINUTE);
    let (hours, minutes) = (minutes / MINUTES_IN_HOUR, minutes % MINUTES_IN_HOUR);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

pub fn volume_at(step: u8) -> u8 {
    (u16::from(step + 1) * u16::from(MAX_VOLUME) / u16::from(VOLUME_STEPS))
        .try_into()
        .unwrap_or(MAX_VOLUME)
}

pub fn bar_width(media: &Media, room: gpui::Size<Pixels>, scale_factor: f32) -> Pixels {
    let shown = media.frame.as_ref().filter(|_| media.video).map(|frame| {
        let size = frame.size(0);
        let logical = |device: i32| logical_side(device, scale_factor);
        fitted(
            gpui::size(logical(size.width.0), logical(size.height.0)),
            art_room(room, true),
        )
        .width
    });
    shown.unwrap_or(preview::CARD_WIDTH).min(room.width)
}

pub fn art_room(room: gpui::Size<Pixels>, with_bar: bool) -> gpui::Size<Pixels> {
    if with_bar {
        gpui::size(
            room.width,
            (room.height - BAR_HEIGHT - preview::GAP).max(px(0.0)),
        )
    } else {
        room
    }
}

pub fn media_art(
    media: &Media,
    room: gpui::Size<Pixels>,
    scale_factor: f32,
    colors: &Colors,
) -> Option<AnyElement> {
    let frame = media.frame.as_ref().map(|frame| {
        let size = frame.size(0);
        let logical = |device: i32| logical_side(device, scale_factor);
        let shown = gpui::size(logical(size.width.0), logical(size.height.0));
        (frame, shown)
    });
    if media.video {
        let (frame, shown) = frame?;
        let drawn = fitted(shown, room);
        return Some(
            img(Arc::clone(frame))
                .debug_selector(|| "preview-video".to_owned())
                .w(drawn.width)
                .h(drawn.height)
                .rounded(radius::CARD)
                .into_any_element(),
        );
    }
    let cover = frame.map_or_else(
        || {
            div()
                .flex()
                .items_center()
                .justify_center()
                .size(SONG_COVER)
                .child(icon(Icon::FileAudio, preview::BIG_ICON, colors.text_dim))
                .into_any_element()
        },
        |(frame, shown)| {
            let drawn = fitted(shown, gpui::size(SONG_COVER, SONG_COVER));
            img(Arc::clone(frame))
                .w(drawn.width)
                .h(drawn.height)
                .rounded(radius::ROW)
                .into_any_element()
        },
    );
    let mut tags = media.tags();
    Some(
        div()
            .debug_selector(|| "preview-song".to_owned())
            .flex()
            .flex_col()
            .items_center()
            .gap(SONG_GAP)
            .w(preview::CARD_WIDTH)
            .max_w_full()
            .py(preview::TEXT_PAD_Y)
            .rounded(radius::CARD)
            .border(size::LINE)
            .border_color(colors.line)
            .bg(colors.window)
            .child(cover)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(TAG_GAP)
                    .max_w_full()
                    .children(tags.next().map(|title| {
                        div()
                            .text_color(colors.text)
                            .font_weight(text::STRONG_WEIGHT)
                            .child(SharedString::from(title.to_owned()))
                    }))
                    .children(tags.map(|tag| {
                        div()
                            .text_color(colors.text_soft)
                            .child(SharedString::from(tag.to_owned()))
                    })),
            )
            .into_any_element(),
    )
}

pub fn bar(
    media: &Media,
    volume: u8,
    width: Pixels,
    colors: &Colors,
    cx: &Context<PreviewPart>,
) -> Div {
    let glyph = if media.state == State::Playing {
        Icon::Pause
    } else {
        Icon::Play
    };
    let time = media.length.map_or_else(
        || clock(media.at),
        |length| format!("{}{TIME_SEPARATOR}{}", clock(media.at), clock(length)),
    );
    div()
        .debug_selector(|| "preview-player".to_owned())
        .flex()
        .flex_none()
        .items_center()
        .gap(BAR_GAP)
        .w(width.max(BAR_MIN_WIDTH))
        .max_w_full()
        .h(BAR_HEIGHT)
        .px(BAR_PAD)
        .rounded(radius::CARD)
        .bg(colors.lifted)
        .border(size::LINE)
        .border_color(colors.line)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            div()
                .id("preview-play")
                .debug_selector(|| "preview-play".to_owned())
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(size::ICON_BUTTON)
                .rounded(radius::ROW)
                .cursor_pointer()
                .hover(|button| button.bg(colors.hover))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| this.play_or_pause(cx)),
                )
                .child(icon(glyph, size::BUTTON_ICON, colors.text)),
        )
        .child(
            div()
                .debug_selector(|| "preview-time".to_owned())
                .flex_none()
                .text_size(text::DETAIL)
                .text_color(colors.text_soft)
                .child(time),
        )
        .child(seek_track(media, colors, cx))
        .child(icon(Icon::Volume, size::BUTTON_ICON, colors.text_soft))
        .child(volume_track(volume, colors, cx))
}

fn seek_track(media: &Media, colors: &Colors, cx: &Context<PreviewPart>) -> Div {
    let played = media.played_steps();
    let track = div()
        .debug_selector(|| "preview-seek".to_owned())
        .flex()
        .flex_1()
        .min_w_0()
        .h(TRACK_HEIGHT)
        .cursor_pointer()
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _, _, _| this.let_go_of_seek()),
        );
    track.children((0..SEEK_STEPS).map(|step| {
        let tint = if step < played {
            colors.accept
        } else {
            colors.line
        };
        div()
            .flex_1()
            .h_full()
            .flex()
            .items_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| this.seek_step(step, cx)),
            )
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    this.seek_step(step, cx);
                }
            }))
            .child(div().w_full().h(LINE_HEIGHT).bg(tint))
    }))
}

fn volume_track(volume: u8, colors: &Colors, cx: &Context<PreviewPart>) -> Div {
    div()
        .debug_selector(|| "preview-volume".to_owned())
        .flex()
        .flex_none()
        .gap(VOLUME_GAP)
        .w(VOLUME_WIDTH)
        .h(TRACK_HEIGHT)
        .cursor_pointer()
        .children((0..VOLUME_STEPS).map(|step| {
            let tint = if volume_at(step) <= volume {
                colors.text_soft
            } else {
                colors.line
            };
            div()
                .flex_1()
                .h_full()
                .flex()
                .items_center()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.set_volume(volume_at(step), cx);
                    }),
                )
                .child(div().w_full().h(LINE_HEIGHT).bg(tint))
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clock_shows_minutes_and_hours_only_when_needed() {
        assert_eq!(clock(Duration::from_secs(7)), "0:07");
        assert_eq!(clock(Duration::from_secs(83)), "1:23");
        assert_eq!(clock(Duration::from_secs(3_725)), "1:02:05");
    }

    #[test]
    fn jumps_stay_inside_the_video() {
        let mut media = Media::new(PreviewId(1), true);
        media.heard(Played::Length(Duration::from_secs(30)));
        media.heard(Played::At(Duration::from_secs(4)));
        assert_eq!(media.jumped(true), Duration::ZERO);
        assert_eq!(media.jumped(false), Duration::from_secs(14));
        media.heard(Played::At(Duration::from_secs(25)));
        assert_eq!(media.jumped(false), Duration::from_secs(30));
    }

    #[test]
    fn a_seek_step_is_a_share_of_the_length_and_asked_once() {
        let mut media = Media::new(PreviewId(1), true);
        assert_eq!(media.at_step(3), None);
        media.let_go();
        media.heard(Played::Length(Duration::from_secs(159)));
        assert_eq!(
            media.at_step(SEEK_STEPS - 1),
            Some(Duration::from_secs(159))
        );
        assert_eq!(media.at_step(SEEK_STEPS - 1), None);
        assert_eq!(media.at_step(0), Some(Duration::ZERO));
        assert_eq!(media.played_steps(), 0);
        media.heard(Played::At(Duration::from_secs(159)));
        assert_eq!(media.played_steps(), SEEK_STEPS);
    }

    #[test]
    fn volume_steps_reach_the_top_and_tags_are_kept_in_order() {
        assert_eq!(volume_at(0), MAX_VOLUME / VOLUME_STEPS);
        assert_eq!(volume_at(VOLUME_STEPS - 1), MAX_VOLUME);
        let mut media = Media::new(PreviewId(1), false);
        media.heard(Played::Tag(MediaTag::Artist, "Someone".to_owned()));
        media.heard(Played::Tag(MediaTag::Title, "A Song".to_owned()));
        media.heard(Played::Tag(MediaTag::Album, String::new()));
        assert_eq!(media.tags().collect::<Vec<_>>(), ["A Song", "Someone"]);
        media.heard(Played::Paused(false));
        media.heard(Played::Ended);
        media.heard(Played::Paused(true));
        assert_eq!(media.state, State::Ended);
        media.heard(Played::Paused(false));
        assert_eq!(media.state, State::Playing);
    }
}
