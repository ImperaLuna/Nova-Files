//! A window and the real core, wired as the program wires them: the window
//! connects, pings, and shows the core as connected once the answer lands.

use gpui::TestAppContext;
use nova_files_ui::{CoreStatus, MainWindow};

#[gpui::test]
#[expect(
    clippy::future_not_send,
    reason = "GPUI test contexts live on one thread"
)]
async fn window_connects_to_the_core(cx: &mut TestAppContext) {
    // The core answers from its own thread.
    cx.executor().allow_parking();
    let core = nova_files_core::spawn().unwrap();

    let window = cx.add_window(|_, cx| MainWindow::new(&core.connector, cx));
    let view = window.root(cx).unwrap();

    view.condition::<CoreStatus>(cx, |view, _| view.status() == CoreStatus::Connected)
        .await;
}

#[gpui::test]
fn window_without_a_core_still_opens(cx: &mut TestAppContext) {
    let core = nova_files_core::spawn_with_idle_exit(std::time::Duration::ZERO).unwrap();
    core.thread.join().unwrap();

    let window = cx.add_window(|_, cx| MainWindow::new(&core.connector, cx));
    let view = window.root(cx).unwrap();

    view.read_with(cx, |view, _| assert_eq!(view.status(), CoreStatus::Gone));
}
