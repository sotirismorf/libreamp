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
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sdl3::event::{Event, WindowEvent};
use sdl3::keyboard::{Keycode, Mod};
use sdl3::mouse::MouseButton;
use sdl3::pixels::Color;
use sdl3::pixels::PixelFormat;
use sdl3::render::{BlendMode, FRect, ScaleMode, Texture, TextureCreator, WindowCanvas};
use sdl3::video::{HitTestResult, WindowContext, WindowFlags};

use app::App;
use audio::Audio;
use gfx::Framebuffer;
use skin::{Regions, Skin};
use sprites::{MAIN_H, MAIN_W};
use ui::menu::KeyResult;
use ui::{Action, ArtWindow, EqWindow, MainWindow, PlaylistWindow};

/// Reverse-DNS application ID: the Wayland app_id, desktop file and Flatpak ID.
pub const APP_ID: &str = "io.github.sotirismorf.LibreAmp";
const TICK: Duration = Duration::from_millis(33);
/// Cleared to this, so parts a skin's region.txt cuts away show what's behind the window.
const CLEAR: Color = Color::RGBA(0, 0, 0, 0);
/// Sizes offered, in percent of the classic skin size; 200 is Winamp's "double size".
pub const SCALES: [u32; 3] = [100, 150, 200];
pub const DEFAULT_SCALE: u32 = 200;
const USAGE: &str = "usage: libreamp [--scale N] [skin.wsz] [files, folders or playlists...]

  -s, --scale N    size in percent: 100, 150 or 200 (default 200)
  -h, --help       show this help
  -V, --version    show the version

Ctrl + / Ctrl - change the size while running.";

/// A size from the command line or the config file: "150" or "150%".
pub fn parse_scale(s: &str) -> Option<u32> {
    let n = s.trim().trim_end_matches('%').parse().ok()?;
    SCALES.contains(&n).then_some(n)
}

struct Args {
    skin: Option<PathBuf>,
    files: Vec<PathBuf>,
    /// Size in percent, one of SCALES.
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
                let n = it.next().as_deref().and_then(parse_scale);
                args.scale = Some(n.ok_or("--scale takes 100, 150 or 200")?);
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
    Menu,
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
                t.set_blend_mode(BlendMode::Blend);
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

/// Everything drawn each frame, kept between frames so textures are reused.
struct Scene<'a> {
    main: Layer<'a>,
    eq: Layer<'a>,
    pl: Layer<'a>,
    art: Layer<'a>,
    /// The open menu and its submenu.
    menu: [Layer<'a>; 2],
    /// GPU copies of the cover's size levels, uploaded as needed, for the art with this id.
    art_textures: (u64, Vec<Option<Texture<'a>>>),
}

/// Where the docked windows sit, in skin pixels from the top; None when hidden.
#[derive(Clone, Copy)]
struct Layout {
    eq: Option<i32>,
    pl: Option<i32>,
    art: Option<i32>,
}

impl<'a> Scene<'a> {
    fn new() -> Self {
        let layer = || Layer::new(1, 1);
        Scene {
            main: Layer::new(MAIN_W, MAIN_H),
            eq: layer(),
            pl: layer(),
            art: layer(),
            menu: [layer(), layer()],
            art_textures: (u64::MAX, Vec::new()),
        }
    }

    /// Draws the whole player into the canvas's current target, `k` pixels per skin pixel.
    fn draw(
        &mut self,
        canvas: &mut WindowCanvas,
        textures: &'a TextureCreator<WindowContext>,
        app: &App,
        layout: Layout,
        k: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        canvas.set_draw_color(CLEAR);
        canvas.clear();
        let f = k as f32;
        let regions = &app.skin.regions;
        app.main.draw(&mut self.main.fb, &app.skin);
        app.vis.draw(&mut self.main.fb, &app.skin.vis_colors);
        if let Some(r) = &regions.main {
            r.apply(&mut self.main.fb);
        }
        self.main.present(canvas, textures, 0, f)?;
        if let Some(y) = layout.eq {
            app.eq.draw(&mut self.eq.fb, &app.skin);
            if let Some(r) = &regions.eq {
                r.apply(&mut self.eq.fb);
            }
            self.eq.present(canvas, textures, y, f)?;
        }
        if let Some(y) = layout.pl {
            let playing = app.main.state != ui::PlayState::Stopped;
            let elapsed = playing.then_some(app.main.elapsed);
            app.pl
                .draw(&mut self.pl.fb, &app.skin, &app.playlist, elapsed);
            self.pl.present(canvas, textures, y, f)?;
        }
        if let Some(y) = layout.art {
            app.art_win
                .draw(&mut self.art.fb, &app.skin, app.art.is_some());
            self.art.present(canvas, textures, y, f)?;
            if let Some(art) = &app.art {
                // Drawn at full resolution rather than upscaled, so covers stay detailed.
                let area = app.art_win.inner();
                let (bw, bh) = (area.w as u32 * k, area.h as u32 * k);
                let (iw, ih) = art.levels[0].dimensions();
                let (w, h) = art::fit(iw, ih, bw, bh);
                // The GPU scales the nearest larger level the rest of the way.
                let (cached_id, cache) = &mut self.art_textures;
                if *cached_id != art.id {
                    *cached_id = art.id;
                    cache.clear();
                    cache.resize_with(art.levels.len(), || None);
                }
                let level = art.level_for(w, h);
                let texture = match &mut cache[level] {
                    Some(t) => t,
                    slot => {
                        let image = &art.levels[level];
                        let (lw, lh) = image.dimensions();
                        let mut t = textures.create_texture_static(PixelFormat::RGBA32, lw, lh)?;
                        t.update(None, image.as_raw(), lw as usize * 4)?;
                        t.set_scale_mode(ScaleMode::Linear);
                        slot.insert(t)
                    }
                };
                let x = area.x as u32 * k + (bw - w) / 2;
                let y = (y + area.y) as u32 * k + (bh - h) / 2;
                let dst = FRect::new(x as f32, y as f32, w as f32, h as f32);
                canvas.copy(texture, None, dst)?;
            }
        }
        if let Some(menu) = &app.menu {
            for (level, layer) in self.menu.iter_mut().enumerate() {
                if let Some((x, y)) = menu.draw(level, &mut layer.fb, app.skin.playlist_colors) {
                    layer.x = x;
                    layer.present(canvas, textures, y, f)?;
                }
            }
        }
        Ok(())
    }
}

/// False where the skin's region.txt makes (x, y) see-through: the main window at the top, the
/// equalizer at `eq_top` when shown.
fn solid_at(regions: &Regions, x: i32, y: i32, eq_top: Option<i32>) -> bool {
    if y < MAIN_H {
        return regions.main.as_ref().is_none_or(|r| r.contains(x, y));
    }
    match (eq_top, &regions.eq) {
        (Some(t), Some(r)) if (t..t + ui::eq_window::H).contains(&y) => r.contains(x, y - t),
        _ => true,
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
    // Transparent so skins can cut the window into shapes (region.txt). set_flags replaces any
    // flags set before it, so it comes first.
    let window = video
        .window("LibreAmp", MAIN_W as u32 * 2, MAIN_H as u32 * 2)
        .set_flags(WindowFlags::TRANSPARENT)
        .borderless()
        .high_pixel_density()
        .build()?;
    let mut canvas = window.into_canvas();
    let textures = canvas.texture_creator();
    let mut app = App::new(Audio::new(&sdl)?, &state, skin, skin_path);
    if args.files.is_empty() {
        app.restore_playlist(state.current, state.position);
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
    // The skin's window shapes: see-through pixels aren't title bar.
    let shapes = Arc::new(Mutex::new(app.skin.regions.clone()));
    {
        let (to_skin, eq_top, pl_top, art_top, shapes) = (
            to_skin.clone(),
            eq_top.clone(),
            pl_top.clone(),
            art_top.clone(),
            shapes.clone(),
        );
        let result = canvas.window_mut().set_hit_test(move |p| {
            let f = f32::from_bits(to_skin.load(Ordering::Relaxed));
            let (x, y) = ((p.x as f32 * f) as i32, (p.y as f32 * f) as i32);
            let top = |t: &AtomicI32| Some(t.load(Ordering::Relaxed)).filter(|t| *t >= 0);
            let solid = shapes
                .lock()
                .map_or(true, |s| solid_at(&s, x, y, top(&eq_top)));
            let draggable = solid
                && (MainWindow::is_draggable(x, y)
                    || top(&eq_top).is_some_and(|t| EqWindow::is_draggable(x, y - t))
                    || top(&pl_top).is_some_and(|t| PlaylistWindow::is_draggable(x, y - t))
                    || top(&art_top).is_some_and(|t| ArtWindow::is_draggable(x, y - t)));
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

    let mut scene = Scene::new();
    // For sizes that aren't a whole number of screen pixels per skin pixel: the player drawn at
    // the next whole size, then shrunk smoothly onto the window.
    let mut offscreen: Option<Texture> = None;
    if let Some(scale) = args.scale {
        app.scale = scale;
    }
    // A menu is placed for the window's current layout; a change of layout closes it.
    let mut menu_layout = 0;
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
        // A frameless art panel has no title bar to drag by.
        let art_drag = art_y.filter(|_| app.art_win.framed);
        art_top.store(art_drag.unwrap_or(-1), Ordering::Relaxed);
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

        if total_h != menu_layout {
            app.menu = None;
            menu_layout = total_h;
        }
        if let Ok(mut s) = shapes.lock()
            && !Arc::ptr_eq(&s, &app.skin.regions)
        {
            *s = app.skin.regions.clone();
        }
        let layout = Layout {
            eq: eq_y,
            pl: pl_y,
            art: art_y,
        };
        // The window is the skin at app.scale percent, in logical (display-independent) pixels.
        let density = canvas.window().pixel_density();
        let size = |skin_px: i32| (skin_px as f32 * app.scale as f32 / 100.0).ceil() as u32;
        let want = (size(MAIN_W), size(total_h));
        if requested_size != Some(want) {
            canvas.window_mut().set_size(want.0, want.1)?;
            requested_size = Some(want);
        }
        // Screen pixels per skin pixel. If the compositor gave less room than asked (e.g. a tiling
        // one), use the largest whole multiple that fits.
        let (pw, ph) = canvas.output_size()?;
        let mut px = app.scale as f32 / 100.0 * density;
        if MAIN_W as f32 * px > pw as f32 + 1.0 || total_h as f32 * px > ph as f32 + 1.0 {
            px = (pw / MAIN_W as u32).min(ph / total_h as u32).max(1) as f32;
        }
        let factor = density / px;
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
                    if let Some(menu) = &app.menu {
                        if menu.contains(x, y) {
                            captured = Some(Target::Menu);
                        } else {
                            app.menu = None;
                        }
                        continue;
                    }
                    // See-through parts of a shaped skin don't take clicks.
                    if !solid_at(&app.skin.regions, x, y, eq_y) {
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
                        Target::Menu => {}
                    }
                }
                Event::MouseButtonDown {
                    mouse_btn: MouseButton::Right,
                    x,
                    y,
                    ..
                } => {
                    let (x, y) = skin_pos(x, y);
                    captured = None;
                    if solid_at(&app.skin.regions, x, y, eq_y) {
                        app.open_menu(x, y, (MAIN_W, total_h));
                    }
                }
                Event::MouseMotion { x, y, .. } => {
                    let (x, y) = skin_pos(x, y);
                    if let Some(menu) = &mut app.menu {
                        menu.mouse_move(x, y);
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
                        Some(Target::Menu) => {
                            if let Some(c) = app.menu.as_mut().and_then(|m| m.mouse_up(x, y)) {
                                app.menu_command(c, canvas.window_mut());
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
                    // An open menu takes the keyboard.
                    if let Some(menu) = &mut app.menu {
                        match menu.key(key) {
                            KeyResult::Handled => {}
                            KeyResult::Close => app.menu = None,
                            KeyResult::Chosen(c) => app.menu_command(c, canvas.window_mut()),
                        }
                        continue;
                    }
                    let action = match key {
                        Keycode::Equals | Keycode::Plus | Keycode::KpPlus if ctrl => {
                            app.scale = SCALES
                                .into_iter()
                                .find(|s| *s > app.scale)
                                .unwrap_or(app.scale);
                            None
                        }
                        Keycode::Minus | Keycode::KpMinus if ctrl => {
                            app.scale = SCALES
                                .into_iter()
                                .rev()
                                .find(|s| *s < app.scale)
                                .unwrap_or(app.scale);
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
        app.update();
        let now = Instant::now();
        if now >= next_tick {
            app.tick();
            next_tick = (next_tick + TICK).max(now);
        }

        let whole = px.round();
        if (px - whole).abs() < 0.01 {
            // A whole number of screen pixels per skin pixel: drawn directly, nearest-neighbour.
            scene.draw(&mut canvas, &textures, &app, layout, whole as u32)?;
        } else {
            // Otherwise drawn crisply at the next whole size and shrunk with linear filtering
            // ("sharp bilinear"): pixels stay even and sharp, with at most a one-pixel soft edge.
            let k = px.ceil() as u32;
            let (w, h) = (MAIN_W as u32 * k, total_h as u32 * k);
            let target = match &mut offscreen {
                Some(t) if (t.query().width, t.query().height) == (w, h) => t,
                slot => {
                    let mut t = textures.create_texture_target(PixelFormat::ARGB8888, w, h)?;
                    t.set_scale_mode(ScaleMode::Linear);
                    t.set_blend_mode(BlendMode::Blend);
                    slot.insert(t)
                }
            };
            let mut result = Ok(());
            canvas.with_texture_canvas(target, |c| {
                result = scene.draw(c, &textures, &app, layout, k);
            })?;
            result?;
            canvas.set_draw_color(CLEAR);
            canvas.clear();
            let dst = FRect::new(0.0, 0.0, MAIN_W as f32 * px, total_h as f32 * px);
            canvas.copy(target, None, dst)?;
        }
        canvas.present();
    }

    app.audio.stop();
    app.save();
    Ok(())
}
