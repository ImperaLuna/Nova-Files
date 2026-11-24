use std::collections::HashSet;

use gpui::{App, AppContext as _, Context, Focusable as _, ScrollHandle, Window};
use nova_files_shell::Parts;

use super::MainWindow;
use crate::commands::{ACTIONS, BACK, CommandSpec, FORWARD, OPEN, SHORTCUTS, UP};
use crate::shortcuts::{Picking, Shortcut, Shortcuts};
use crate::text::{TextField, TextFieldEvent};

const PLACEHOLDER: &str = "Search commands";

impl MainWindow {
    pub(crate) fn toggle_shortcuts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.shortcuts.is_some() {
            self.close_shortcuts(window, cx);
        } else if !self.renaming(cx) {
            self.open_shortcuts(window, cx);
        }
    }

    fn open_shortcuts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menu(cx);
        let home = self.keys_home(cx);
        let scene = self.scene(cx);
        let list = command_specs(cx)
            .into_iter()
            .map(|spec| {
                let runs = window.is_action_available_in(&*spec.action(), &home)
                    && self.window_allows(spec)
                    && self.parts.applies(spec, &scene, cx);
                Shortcut::new(spec, runs)
            })
            .collect();
        let field = cx.new(|cx| TextField::new(cx).with_placeholder(PLACEHOLDER));
        let typing = cx.subscribe_in(
            &field,
            window,
            |this, field, event, window, cx| match event {
                TextFieldEvent::Changed => {
                    let typed = field.read(cx).text().to_owned();
                    if let Some(shortcuts) = this.shortcuts.as_mut() {
                        shortcuts.picking.refilter(&typed);
                        shortcuts.scroll.scroll_to_item(0);
                    }
                    cx.notify();
                }
                TextFieldEvent::Submitted => this.run_marked_shortcut(window, cx),
                TextFieldEvent::Cancelled => this.close_shortcuts(window, cx),
            },
        );
        window.focus(&field.focus_handle(cx), cx);
        self.shortcuts = Some(Shortcuts {
            picking: Picking::new(list),
            field,
            scroll: ScrollHandle::new(),
            _typing: typing,
        });
        cx.notify();
    }

    fn window_allows(&self, spec: &CommandSpec) -> bool {
        let allowed = [
            (&BACK, self.history.back_target().is_some()),
            (&FORWARD, self.history.forward_target().is_some()),
            (&UP, self.parent().is_some()),
            (&OPEN, self.selected_count() > 0),
        ];
        allowed
            .into_iter()
            .find(|(command, _)| *command == spec)
            .is_none_or(|(_, allows)| allows)
    }

    pub(crate) fn focus_shortcut_field(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(shortcuts) = self.shortcuts.as_ref() {
            window.focus(&shortcuts.field.focus_handle(cx), cx);
        }
    }

    pub(crate) fn close_shortcuts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.shortcuts.take().is_some() {
            window.focus(&self.keys_home(cx), cx);
            cx.notify();
        }
    }

    pub(crate) fn step_shortcut(&mut self, back: bool, cx: &mut Context<Self>) {
        if let Some(shortcuts) = self.shortcuts.as_mut() {
            shortcuts.picking.step(back);
            if let Some(at) = shortcuts.picking.marked {
                shortcuts.scroll.scroll_to_item(at);
            }
            cx.notify();
        }
    }

    pub(crate) fn mark_shortcut(&mut self, at: usize, cx: &mut Context<Self>) {
        if self
            .shortcuts
            .as_mut()
            .is_some_and(|shortcuts| shortcuts.picking.mark(at))
        {
            cx.notify();
        }
    }

    pub(crate) fn run_shortcut(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.mark_shortcut_for_run(at) {
            self.run_marked_shortcut(window, cx);
        }
    }

    fn mark_shortcut_for_run(&mut self, at: usize) -> bool {
        let Some(shortcuts) = self.shortcuts.as_mut() else {
            return false;
        };
        shortcuts.picking.mark(at);
        shortcuts.picking.marked == Some(at)
    }

    fn run_marked_shortcut(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let spec = self
            .shortcuts
            .as_ref()
            .and_then(|shortcuts| shortcuts.picking.marked_spec());
        self.close_shortcuts(window, cx);
        if let Some(spec) = spec {
            window.dispatch_action(spec.action(), cx);
        }
    }

    pub const fn shortcuts(&self) -> Option<&Shortcuts> {
        self.shortcuts.as_ref()
    }
}

fn command_specs(cx: &App) -> Vec<&'static CommandSpec> {
    let parts = cx
        .try_global::<Parts>()
        .map(Parts::specs)
        .unwrap_or_default();
    let mut seen = HashSet::new();
    ACTIONS
        .iter()
        .chain(parts)
        .filter(|spec| **spec != SHORTCUTS && seen.insert(spec.id))
        .collect()
}
