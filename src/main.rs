// SPDX-License-Identifier: GPL-3.0-or-later
mod window;
use adw::prelude::*;
use gtk::{gio, glib};
use std::{cell::RefCell, rc::Rc};
fn main() -> glib::ExitCode {
    glib::set_application_name("Proton Pass GTK");
    let app = adw::Application::builder()
        .application_id("io.github.isyourbrainfoss.ProtonPassGtk")
        .flags(gio::ApplicationFlags::empty())
        .build();
    let controller = Rc::new(RefCell::new(None::<Rc<window::Window>>));
    let current = controller.clone();
    app.connect_activate(move |app| {
        if let Some(w) = app.active_window() {
            w.present();
            return;
        }
        let w = window::Window::new(app);
        let weak = Rc::downgrade(&current);
        w.window().connect_close_request(move |_| {
            if let Some(current) = weak.upgrade() {
                current.borrow_mut().take();
            }
            glib::Propagation::Proceed
        });
        *current.borrow_mut() = Some(w);
    });
    app.connect_shutdown(move |_| {
        controller.borrow_mut().take();
    });
    app.run()
}
