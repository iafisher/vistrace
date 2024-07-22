use std::sync::mpsc;
use std::thread;

use cursive::reexports::crossbeam_channel::Sender;
use cursive::theme::{BorderStyle, Palette};
use cursive::traits::With;
use cursive::view::{Nameable, Resizable, Scrollable};
use cursive::views::TextView;
use cursive::Cursive;

use crate::strace;

pub fn main(rx: mpsc::Receiver<strace::Message>) {
    let mut siv = cursive::default();

    // from https://github.com/gyscos/cursive/blob/cursive-v0.20.0/cursive/examples/theme_manual.rs
    siv.set_theme(cursive::theme::Theme {
        shadow: true,
        borders: BorderStyle::Simple,
        palette: Palette::default().with(|palette| {
            use cursive::theme::BaseColor::*;
            use cursive::theme::Color::*;
            use cursive::theme::PaletteColor::*;

            palette[Background] = TerminalDefault;
            palette[View] = TerminalDefault;
            palette[Primary] = White.dark();
            palette[TitlePrimary] = Blue.light();
            palette[Secondary] = Blue.light();
            palette[Highlight] = Blue.dark();
        }),
    });

    // siv.add_layer(
    //     Dialog::around(TextView::new("Hello, dialog!"))
    //         .title("vistrace")
    //         .button("Quit", |s| s.quit()),
    // );
    siv.add_fullscreen_layer(
        TextView::new("")
            .with_name("content")
            .scrollable()
            .full_screen(),
    );
    siv.add_global_callback('q', |s| s.quit());

    siv.set_fps(10);

    let sink = siv.cb_sink().clone();
    let handle = thread::spawn(move || {
        read_messages(rx, sink);
    });

    siv.run();

    handle.join().unwrap();
}

fn read_messages(
    rx: mpsc::Receiver<strace::Message>,
    sink: Sender<Box<dyn FnOnce(&mut Cursive) + Send>>,
) {
    for msg in rx.iter() {
        match msg {
            strace::Message::Syscall(syscall) => {
                // TODO: handle error
                let _ = sink.send(Box::new(|s: &mut Cursive| {
                    s.call_on_name("content", |t: &mut TextView| {
                        t.append(syscall.name);
                        t.append("\n");
                    });
                }));
            }
        }
    }
}
