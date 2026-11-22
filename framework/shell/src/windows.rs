use std::path::PathBuf;
use std::rc::Rc;

use gpui::{App, ForeignParent, Global, Pixels, Size, TitlebarOptions};
use nova_files_api::Connector;

use crate::opening::Opening;

#[derive(Default)]
pub struct WindowLook {
    pub size: Option<Size<Pixels>>,
    pub titlebar: Option<TitlebarOptions>,
    pub foreign_parent: Option<ForeignParent>,
    pub over_asker: bool,
}

pub trait WindowAsk: Send {
    fn folder(&mut self) -> Option<PathBuf>;
    fn look(&self, connector: &Connector, cx: &mut App) -> WindowLook;
    fn opening(self: Box<Self>) -> Opening;
}

pub type Asked = Box<dyn WindowAsk>;
pub type CloseWhen = Box<dyn Fn(&Opening) -> bool + Send>;

pub type OpenWith = Rc<dyn Fn(Asked, &mut App)>;

#[derive(Clone)]
pub struct Opener(pub OpenWith);

impl Global for Opener {}

pub fn open_window(ask: Asked, cx: &mut App) {
    if let Some(opener) = cx.try_global::<Opener>().cloned() {
        (opener.0)(ask, cx);
    }
}
