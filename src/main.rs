mod app;
mod art;
mod audio;
mod config;
mod eq;
mod font;
mod gfx;
mod playlist;
mod skin;
mod sprites;
mod ui;
mod vis;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use image::imageops::FilterType;
use sdl3::event::{Event, WindowEvent};
use sdl3::keyboard::{Keycode, Mod};
use sdl3::mouse::MouseButton;
use sdl3::pixels::PixelFormat;
use sdl3::render::{FRect, ScaleMode, Texture, TextureCreator, WindowCanvas};
use sdl3::video::{HitTestResult, WindowContext};

use app::App;
use audio::Audio;
use gfx::Framebuffer;
use skin::Skin;
use sprites::{MAIN_H, MAIN_W};
use ui::{Action, ArtWindow, EqWindow, MainWindow, PlaylistWindow};

/// Reverse-DNS application ID: the Wayland app_id, desktop file and Flatpak ID.
pub const APP_ID: &str = "io.github.sotirismorf.LibreAmp";
const TICK: Duration = Duration::from_millis(33);
pub const MAX_SCALE: u32 = 12;
const USAGE: &str = "usage: libreamp [--scale N] [skin.wsz] [files, folders or playlists...]

  -s, --scale N    upscale the UI N times (1 to 12); default follows the display
  -h, --help       show this help
  -V, --version    show the version

Ctrl + / Ctrl - change the scale while running.";

struct Args {
    skin: Option<PathBuf>,
    files: Vec<PathBuf>,
    /// Integer upscale in physical pixels; None picks one from the display scale.
    scale: Option<u32>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        skin: None,
        files: Vec::new(),
        scale: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-s" | "--scale" => {
                let n = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .filter(|n| (1..=MAX_SCALE).contains(n));
                args.scale =
                    Some(n.ok_or(format!("--scale takes a number from 1 to {MAX_SCALE}"))?);
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("libreamp {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            _ if arg.starts_with('-') && arg.len() > 1 && !Path::new(&arg).exists() => {
                return Err(format!("unknown option {arg}\n{USAGE}"));
            }
            _ if skin::is_skin(Path::new(&arg)) => args.skin = Some(arg.into()),
            _ => args.files.push(arg.into()),
        }
    }
    Ok(args)
}

/// Which window a mouse press started in; it keeps the pointer until release.
#[derive(Clone, Copy, PartialEq)]
enum Target {
    Main,
    Eq,
    Playlist,
    Art,
    Popup,
}

/// A 1x framebuffer and the texture it is uploaded to, drawn scaled at (x, y) skin pixels.
struct Layer<'a> {
    fb: Framebuffer,
    texture: Option<Texture<'a>>,
    x: i32,
}

impl<'a> Layer<'a> {
    fn new(w: i32, h: i32) -> Self {
        Layer {
            fb: Framebuffer::new(w, h),
            texture: None,
            x: 0,
        }
    }

    fn present(
        &mut self,
        canvas: &mut WindowCanvas,
        textures: &'a TextureCreator<WindowContext>,
        y: i32,
        scale: f32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (w, h) = (self.fb.w as u32, self.fb.h as u32);
        let texture = match &mut self.texture {
            Some(t) if (t.query().width, t.query().height) == (w, h) => t,
            slot => {
                let mut t = textures.create_texture_streaming(PixelFormat::ARGB8888, w, h)?;
                t.set_scale_mode(ScaleMode::Nearest);
                slot.insert(t)
            }
        };
        texture.with_lock(None, |buf: &mut [u8], pitch: usize| {
            for (row, src) in buf.chunks_mut(pitch).zip(self.fb.px.chunks(w as usize)) {
                for (dst, px) in row.as_chunks_mut::<4>().0.iter_mut().zip(src) {
                    dst.copy_from_slice(&px.to_ne_bytes());
                }
            }
        })?;
        let dst = FRect::new(
            self.x as f32 * scale,
            y as f32 * scale,
            w as f32 * scale,
            h as f32 * scale,
        );
        canvas.copy(texture, None, dst)?;
        Ok(())
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("libreamp: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args()?;
    let state = config::load();
    let skin_path = args.skin.clone().or(state.skin.clone());
    let (skin, skin_path) = match &skin_path {
        Some(path) => match Skin::load(path) {
            Ok(s) => (s, std::fs::canonicalize(path).ok()),
            // A saved skin that has since moved shouldn't stop the player from starting.
            Err(e) if args.skin.is_none() => {
                eprintln!("libreamp: {e}");
                (Skin::base(), None)
            }
            Err(e) => return Err(e.into()),
        },
        None => (Skin::base(), None),
    };

    // The identifier becomes the Wayland app_id, which must match the desktop file's name.
    sdl3::set_app_metadata(
        Some("LibreAmp"),
        Some(env!("CARGO_PKG_VERSION")),
        Some(APP_ID),
    )?;
    let sdl = sdl3::init()?;
    let video = sdl.video()?;
    let window = video
        .window("LibreAmp", MAIN_W as u32 * 2, MAIN_H as u32 * 2)
        .borderless()
        .high_pixel_density()
        .build()?;
    let mut canvas = window.into_canvas();
    let textures = canvas.texture_creator();
    let mut app = App::new(Audio::new(&sdl)?, &state, skin, skin_path);
    if args.files.is_empty() {
        app.restore_playlist(state.current);
    } else {
        app.add_paths(&args.files);
        app.play_index(0);
    }

    // Window coordinates -> skin pixels factor (density / scale), and where the EQ, playlist and
    // art panel sit (-1 when hidden), shared with the hit-test callback that lets title bars
    // move the window.
    let to_skin = Arc::new(AtomicU32::new(0.5f32.to_bits()));
    let eq_top = Arc::new(AtomicI32::new(-1));
    let pl_top = Arc::new(AtomicI32::new(-1));
    let art_top = Arc::new(AtomicI32::new(-1));
    {
        let (to_skin, eq_top, pl_top, art_top) = (
            to_skin.clone(),
            eq_top.clone(),
            pl_top.clone(),
            art_top.clone(),
        );
        let result = canvas.window_mut().set_hit_test(move |p| {
            let f = f32::from_bits(to_skin.load(Ordering::Relaxed));
            let (x, y) = ((p.x as f32 * f) as i32, (p.y as f32 * f) as i32);
            let top = |t: &AtomicI32| Some(t.load(Ordering::Relaxed)).filter(|t| *t >= 0);
            let draggable = MainWindow::is_draggable(x, y)
                || top(&eq_top).is_some_and(|t| EqWindow::is_draggable(x, y - t))
                || top(&pl_top).is_some_and(|t| PlaylistWindow::is_draggable(x, y - t))
                || top(&art_top).is_some_and(|t| ArtWindow::is_draggable(x, y - t));
            if draggable {
                HitTestResult::Draggable
            } else {
                HitTestResult::Normal
            }
        });
        // Without it the window just can't be dragged by its title bars.
        if let Err(e) = result {
            eprintln!("libreamp: window dragging unavailable: {e}");
        }
    }

    let mut main_layer = Layer::new(MAIN_W, MAIN_H);
    let mut eq_layer = Layer::new(MAIN_W, ui::eq_window::H);
    let mut pl_layer = Layer::new(MAIN_W, app.pl.height());
    let mut art_layer = Layer::new(MAIN_W, app.art_win.height());
    let mut popup_layer = Layer::new(1, 1);
    // The cover scaled to its on-screen size, keyed by (art id, width, height).
    let mut art_texture: Option<((u64, u32, u32), Texture)> = None;
    let mut scale = args.scale.or(state.scale);
    // The size last asked of the compositor; asked again only when the layout changes, so a
    // tiling compositor that ignores it isn't sent a resize every frame.
    let mut requested_size = None;
    let mut captured: Option<Target> = None;
    let mut events = sdl.event_pump()?;
    let keyboard = sdl.keyboard();
    let mut next_tick = Instant::now();

    while !app.quit {
        // Windows stack Winamp-style: main, then EQ, playlist and album art, each when shown.
        let mut total_h = MAIN_H;
        let mut stack = |shown: bool, h: i32| {
            shown.then(|| {
                total_h += h;
                total_h - h
            })
        };
        let eq_y = stack(app.main.eq, ui::eq_window::H);
        let pl_y = stack(app.main.pl, app.pl.height());
        let art_y = stack(app.art_win.open, app.art_win.height());
        eq_top.store(eq_y.unwrap_or(-1), Ordering::Relaxed);
        pl_top.store(pl_y.unwrap_or(-1), Ordering::Relaxed);
        art_top.store(art_y.unwrap_or(-1), Ordering::Relaxed);
        let target_at = |y: i32| {
            [
                (Target::Art, art_y),
                (Target::Playlist, pl_y),
                (Target::Eq, eq_y),
                (Target::Main, Some(0)),
            ]
            .into_iter()
            .find_map(|(t, top)| top.filter(|top| y >= *top).map(|top| (t, y - top)))
        };

        // Keep the window an exact integer multiple of the skin in physical pixels.
        let density = canvas.window().pixel_density();
        let n = scale.unwrap_or_else(|| ((2.0 * density).round() as u32).max(1));
        let want = (
            (MAIN_W as f32 * n as f32 / density).ceil() as u32,
            (total_h as f32 * n as f32 / density).ceil() as u32,
        );
        if requested_size != Some(want) {
            canvas.window_mut().set_size(want.0, want.1)?;
            requested_size = Some(want);
        }
        // The compositor may not honour the size (e.g. tiling); fit whatever we got.
        let (pw, ph) = canvas.output_size()?;
        let fit = n.min(pw / MAIN_W as u32).min(ph / total_h as u32).max(1);
        let factor = density / fit as f32;
        to_skin.store(factor.to_bits(), Ordering::Relaxed);
        let skin_pos = |x: f32, y: f32| ((x * factor).floor() as i32, (y * factor).floor() as i32);

        let timeout = next_tick.saturating_duration_since(Instant::now());
        let first = events.wait_event_timeout(timeout);
        for event in first
            .into_iter()
            .chain(events.poll_iter().collect::<Vec<_>>())
        {
            let mods = keyboard.mod_state();
            let ctrl = mods.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD);
            let shift = mods.intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD);
            match event {
                Event::Quit { .. } => app.quit = true,
                Event::Window { win_event, .. } => match win_event {
                    WindowEvent::FocusGained | WindowEvent::FocusLost => {
                        let focused = matches!(win_event, WindowEvent::FocusGained);
                        app.main.focused = focused;
                        app.eq.focused = focused;
                        app.pl.focused = focused;
                        app.art_win.focused = focused;
                    }
                    _ => {}
                },
                Event::MouseButtonDown {
                    mouse_btn: MouseButton::Left,
                    x,
                    y,
                    clicks,
                    ..
                } => {
                    let (x, y) = skin_pos(x, y);
                    // An open menu takes the click; clicking elsewhere just closes it.
                    if let Some(popup) = &app.popup {
                        if popup.contains(x, y) {
                            captured = Some(Target::Popup);
                        } else {
                            app.popup = None;
                        }
                        continue;
                    }
                    let Some((target, ly)) = target_at(y) else {
                        continue;
                    };
                    captured = Some(target);
                    match target {
                        Target::Main => {
                            if let Some(a) = app.main.mouse_down(x, ly) {
                                app.action(a, canvas.window_mut());
                            }
                        }
                        Target::Eq => {
                            if let Some(c) = app.eq.mouse_down(x, ly) {
                                app.eq_command(c, y - ly, (MAIN_W, total_h));
                            }
                        }
                        Target::Playlist => {
                            let cmd =
                                app.pl
                                    .mouse_down(x, ly, clicks, ctrl, shift, &mut app.playlist);
                            if let Some(c) = cmd {
                                app.pl_command(c, canvas.window_mut());
                            }
                        }
                        Target::Art => app.art_win.mouse_down(x, ly),
                        Target::Popup => {}
                    }
                }
                Event::MouseMotion { x, y, .. } => {
                    let (x, y) = skin_pos(x, y);
                    if let Some(popup) = &mut app.popup {
                        popup.mouse_move(x, y);
                        continue;
                    }
                    match captured {
                        Some(Target::Main) => {
                            if let Some(a) = app.main.mouse_move(x, y) {
                                app.action(a, canvas.window_mut());
                            }
                        }
                        Some(Target::Eq) => {
                            let ey = eq_y.unwrap_or(MAIN_H);
                            if let Some(c) = app.eq.mouse_move(x, y - ey) {
                                app.eq_command(c, ey, (MAIN_W, total_h));
                            }
                        }
                        Some(Target::Art) => {
                            app.art_win.mouse_move(x, y - art_y.unwrap_or(MAIN_H));
                        }
                        // The playlist also tracks hover over its open menus without a press.
                        _ => {
                            if let Some(py) = pl_y {
                                app.pl.mouse_move(x, y - py, &app.playlist);
                            }
                        }
                    }
                }
                Event::MouseButtonUp {
                    mouse_btn: MouseButton::Left,
                    x,
                    y,
                    ..
                } => {
                    let (x, y) = skin_pos(x, y);
                    match captured.take() {
                        Some(Target::Popup) => {
                            if let Some(i) = app.popup.as_ref().and_then(|p| p.mouse_up(x, y)) {
                                app.popup = None;
                                app.choose_preset(i);
                            }
                        }
                        Some(Target::Main) => {
                            if let Some(a) = app.main.mouse_up(x, y) {
                                app.action(a, canvas.window_mut());
                            }
                        }
                        Some(Target::Eq) => {
                            let ey = eq_y.unwrap_or(MAIN_H);
                            if let Some(c) = app.eq.mouse_up(x, y - ey) {
                                app.eq_command(c, ey, (MAIN_W, total_h));
                            }
                        }
                        Some(Target::Playlist) => {
                            let py = pl_y.unwrap_or(MAIN_H);
                            if let Some(c) = app.pl.mouse_up(x, y - py, &mut app.playlist) {
                                app.pl_command(c, canvas.window_mut());
                            }
                        }
                        Some(Target::Art) => {
                            let ay = art_y.unwrap_or(MAIN_H);
                            if let Some(c) = app.art_win.mouse_up(x, y - ay) {
                                app.art_command(c);
                            }
                        }
                        None => {}
                    }
                }
                Event::MouseWheel {
                    y,
                    mouse_x,
                    mouse_y,
                    ..
                } => {
                    let (mx, my) = skin_pos(mouse_x, mouse_y);
                    let notches = if y > 0.0 { 1 } else { -1 };
                    match target_at(my) {
                        Some((Target::Playlist, _)) => app.pl.wheel(notches * 3, &app.playlist),
                        Some((Target::Art, _)) => {}
                        Some((Target::Eq, ly)) => {
                            if let Some(c) = app.eq.wheel(mx, ly, notches) {
                                app.eq_command(c, my - ly, (MAIN_W, total_h));
                            }
                        }
                        _ => {
                            let v = app.main.volume + notches as f32 * 0.05;
                            app.main.volume = v.clamp(0.0, 1.0);
                            app.audio.set_volume(app.main.volume);
                        }
                    }
                }
                Event::KeyDown {
                    keycode: Some(key), ..
                } => {
                    let action = match key {
                        Keycode::Escape if app.popup.is_some() => {
                            app.popup = None;
                            None
                        }
                        Keycode::Equals | Keycode::Plus | Keycode::KpPlus if ctrl => {
                            scale = Some((n + 1).min(MAX_SCALE));
                            None
                        }
                        Keycode::Minus | Keycode::KpMinus if ctrl => {
                            scale = Some(n.saturating_sub(1).max(1));
                            None
                        }
                        Keycode::Q if ctrl => {
                            app.quit = true;
                            None
                        }
                        Keycode::E if ctrl => {
                            app.main.pl = !app.main.pl;
                            None
                        }
                        Keycode::G if ctrl => {
                            app.main.eq = !app.main.eq;
                            None
                        }
                        Keycode::I if ctrl => {
                            app.art_win.open = !app.art_win.open;
                            None
                        }
                        Keycode::Z if !ctrl => Some(Action::Prev),
                        Keycode::X if !ctrl => Some(Action::Play),
                        Keycode::C if !ctrl => Some(Action::Pause),
                        Keycode::V if !ctrl => Some(Action::Stop),
                        Keycode::B if !ctrl => Some(Action::Next),
                        Keycode::L if !ctrl => Some(Action::Eject),
                        _ if app.main.pl => {
                            if let Some(c) = app.pl.key(key, ctrl, shift, &mut app.playlist) {
                                app.pl_command(c, canvas.window_mut());
                            }
                            None
                        }
                        _ => None,
                    };
                    if let Some(a) = action {
                        app.action(a, canvas.window_mut());
                    }
                }
                Event::DropFile { filename, .. } => app.drop_file(Path::new(&filename)),
                Event::DropComplete { .. } => app.drop_complete(),
                _ => {}
            }
        }
        if !app.main.eq {
            app.popup = None;
        }

        app.update();
        let now = Instant::now();
        if now >= next_tick {
            app.tick();
            next_tick = (next_tick + TICK).max(now);
        }

        canvas.set_draw_color((0, 0, 0));
        canvas.clear();
        let f = fit as f32;
        app.main.draw(&mut main_layer.fb, &app.skin);
        app.vis.draw(&mut main_layer.fb, &app.skin.vis_colors);
        main_layer.present(&mut canvas, &textures, 0, f)?;
        if let Some(y) = eq_y {
            app.eq.draw(&mut eq_layer.fb, &app.skin);
            eq_layer.present(&mut canvas, &textures, y, f)?;
        }
        if let Some(y) = pl_y {
            let playing = app.main.state != ui::PlayState::Stopped;
            let elapsed = playing.then_some(app.main.elapsed);
            app.pl
                .draw(&mut pl_layer.fb, &app.skin, &app.playlist, elapsed);
            pl_layer.present(&mut canvas, &textures, y, f)?;
        }
        if let Some(y) = art_y {
            app.art_win
                .draw(&mut art_layer.fb, &app.skin, app.art.is_some());
            art_layer.present(&mut canvas, &textures, y, f)?;
            if let Some(art) = &app.art {
                // Drawn at screen resolution rather than upscaled, so covers stay detailed.
                let area = app.art_win.inner();
                let (bw, bh) = (area.w as u32 * fit, area.h as u32 * fit);
                let (iw, ih) = art.image.dimensions();
                let (w, h) = art::fit(iw, ih, bw, bh);
                let key = (art.id, w, h);
                let texture = match &mut art_texture {
                    Some((k, t)) if *k == key => t,
                    slot => {
                        let scaled =
                            image::imageops::resize(&art.image, w, h, FilterType::CatmullRom);
                        let mut t = textures.create_texture_static(PixelFormat::RGBA32, w, h)?;
                        t.update(None, scaled.as_raw(), w as usize * 4)?;
                        &mut slot.insert((key, t)).1
                    }
                };
                let x = area.x as u32 * fit + (bw - w) / 2;
                let y = (y + area.y) as u32 * fit + (bh - h) / 2;
                let dst = FRect::new(x as f32, y as f32, w as f32, h as f32);
                canvas.copy(texture, None, dst)?;
            }
        }
        if let Some(popup) = &app.popup {
            popup.draw(&mut popup_layer.fb, app.skin.playlist_colors);
            popup_layer.x = popup.x;
            popup_layer.present(&mut canvas, &textures, popup.y, f)?;
        }
        canvas.present();
    }

    app.audio.stop();
    app.save(scale);
    Ok(())
}
