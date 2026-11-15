//! Player logic tying the windows, the playlist and audio together.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use sdl3::dialog::{
    DialogCallback, DialogFileFilter, show_open_file_dialog, show_open_folder_dialog,
    show_save_file_dialog,
};
use sdl3::video::Window;

use crate::art::{Art, ArtLoader};
use crate::audio::Audio;
use crate::config::{self, State};
use crate::eq;
use crate::playlist::Playlist;
use crate::skin::Skin;
use crate::ui::menu::{Entry, Menu};
use crate::ui::{
    Action, ArtCommand, ArtWindow, EqCommand, EqWindow, MainWindow, PlCommand, PlayState,
    PlaylistWindow,
};
use crate::vis::{self, Visualizer};

#[derive(Clone, Copy, Debug)]
enum Dialog {
    /// Eject: replace the playlist and play.
    Open,
    AddFiles,
    AddDir,
    LoadList,
    SaveList,
    Skin,
}

/// What a menu item does.
#[derive(Clone, Copy, Debug)]
pub enum MenuCommand {
    ToggleEq,
    TogglePlaylist,
    ToggleArt,
    Vis(vis::Mode),
    /// Size in percent.
    Scale(u32),
    DefaultSkin,
    OpenSkin,
    Quit,
    /// An EQ preset; None resets to flat.
    Preset(Option<usize>),
}

const AUDIO_FILTER: DialogFileFilter = DialogFileFilter {
    name: "Audio and playlists",
    pattern: "mp3;flac;ogg;oga;wav;m4a;mp4;aac;m3u;m3u8;pls",
};
const LIST_FILTER: DialogFileFilter = DialogFileFilter {
    name: "Playlists",
    pattern: "m3u;m3u8;pls",
};
const SKIN_FILTER: DialogFileFilter = DialogFileFilter {
    name: "Winamp skins",
    pattern: "wsz;zip",
};

pub struct App {
    pub skin: Skin,
    pub skin_path: Option<PathBuf>,
    pub audio: Audio,
    pub playlist: Playlist,
    pub main: MainWindow,
    pub pl: PlaylistWindow,
    pub eq: EqWindow,
    pub art_win: ArtWindow,
    /// The current track's cover, if it has one.
    pub art: Option<Art>,
    art_loader: ArtLoader,
    /// The open popup menu: right-click menu or EQ presets.
    pub menu: Option<Menu<MenuCommand>>,
    /// Size in percent of the classic skin size, one of `crate::SCALES`.
    pub scale: u32,
    pub vis: Visualizer,
    vis_samples: Vec<f32>,
    pub quit: bool,
    dialog_tx: Sender<(Dialog, Vec<PathBuf>)>,
    dialog_rx: Receiver<(Dialog, Vec<PathBuf>)>,
    /// Set by the first audio file of a drag-and-drop.
    drop_started: bool,
    drop_first_new: usize,
}

impl App {
    pub fn new(audio: Audio, state: &State, skin: Skin, skin_path: Option<PathBuf>) -> App {
        let mut main = MainWindow::new();
        main.volume = state.volume;
        main.balance = state.balance;
        main.repeat = state.repeat;
        main.shuffle = state.shuffle;
        main.pl = state.playlist_open;
        main.eq = state.eq_open;
        let mut eq = EqWindow::new();
        eq.on = state.eq_on;
        eq.preamp = state.eq_preamp;
        eq.bands = state.eq_bands;
        audio.set_eq(eq.on, eq.preamp, &eq.bands);
        audio.set_volume(main.volume);
        audio.set_balance(main.balance);
        let (dialog_tx, dialog_rx) = channel();
        App {
            skin,
            skin_path,
            audio,
            playlist: Playlist::new(),
            main,
            pl: PlaylistWindow::new(state.playlist_tiles),
            eq,
            art_win: ArtWindow::new(state.art_open, state.art_height),
            art: None,
            art_loader: ArtLoader::new(),
            menu: None,
            scale: state.scale,
            vis: Visualizer::new(state.vis_mode),
            vis_samples: Vec::new(),
            quit: false,
            dialog_tx,
            dialog_rx,
            drop_started: false,
            drop_first_new: 0,
        }
    }

    pub fn state(&self) -> State {
        State {
            volume: self.main.volume,
            balance: self.main.balance,
            repeat: self.main.repeat,
            shuffle: self.main.shuffle,
            playlist_open: self.main.pl,
            eq_open: self.main.eq,
            vis_mode: self.vis.mode,
            eq_on: self.eq.on,
            eq_preamp: self.eq.preamp,
            eq_bands: self.eq.bands,
            playlist_tiles: self.pl.tiles,
            art_open: self.art_win.open,
            art_height: self.art_win.art_h,
            current: self.playlist.current,
            skin: self.skin_path.clone(),
            scale: self.scale,
            position: (self.main.state != PlayState::Stopped).then_some(self.main.elapsed),
        }
    }

    // Playback.

    pub fn play_index(&mut self, index: usize) {
        if index < self.playlist.len() {
            self.playlist.current = index;
            self.play_current();
        }
    }

    pub fn play_current(&mut self) {
        self.load_current(false);
    }

    /// Opens the current track, playing or paused at its start. False if it can't be played.
    fn load_current(&mut self, paused: bool) -> bool {
        let index = self.playlist.current;
        let Some(entry) = self.playlist.entries.get_mut(index) else {
            return false;
        };
        let ui = &mut self.main;
        let ok = match self.audio.load(&entry.path, paused) {
            Ok(info) => {
                entry.title.get_or_insert(info.title);
                entry.duration = entry.duration.or(info.duration);
                ui.title = format!("{}. {}", index + 1, entry.display_title());
                ui.duration = info.duration;
                ui.kbps = info.kbps;
                ui.khz = Some((info.rate as f32 / 1000.0).round() as u32);
                ui.channels = info.channels;
                ui.elapsed = 0.0;
                ui.state = if paused {
                    PlayState::Paused
                } else {
                    PlayState::Playing
                };
                true
            }
            Err(e) => {
                eprintln!("libreamp: {}: {e}", entry.path.display());
                self.audio.stop();
                ui.title = format!("{}. error: {e}", index + 1);
                ui.state = PlayState::Stopped;
                ui.duration = None;
                false
            }
        };
        self.pl.ensure_visible(index);
        ok
    }

    fn stop(&mut self) {
        self.audio.stop();
        self.main.state = PlayState::Stopped;
        self.main.elapsed = 0.0;
    }

    /// Moves to the next track: random with shuffle, otherwise sequential. False at the end.
    fn advance(&mut self, delta: isize) -> bool {
        let len = self.playlist.len();
        if self.main.shuffle && len > 1 {
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.subsec_nanos() as usize);
            self.playlist.current = (self.playlist.current + 1 + seed % (len - 1)) % len;
            return true;
        }
        self.playlist.step(delta, self.main.repeat || delta < 0)
    }

    pub fn action(&mut self, action: Action, window: &mut Window) {
        match action {
            Action::Close => self.quit = true,
            Action::Minimize => {
                window.minimize();
            }
            Action::Play if self.main.state == PlayState::Paused => {
                self.audio.set_paused(false);
                self.main.state = PlayState::Playing;
            }
            Action::Play => {
                // Like Winamp: play the selected track if one is selected, else the current one.
                if let Some(i) = self.playlist.entries.iter().position(|e| e.selected)
                    && self.main.state == PlayState::Stopped
                {
                    self.playlist.current = i;
                }
                self.play_current();
            }
            Action::Pause => match self.main.state {
                PlayState::Playing => {
                    self.audio.set_paused(true);
                    self.main.state = PlayState::Paused;
                }
                PlayState::Paused => {
                    self.audio.set_paused(false);
                    self.main.state = PlayState::Playing;
                }
                PlayState::Stopped => {}
            },
            Action::Stop => self.stop(),
            Action::Next | Action::Prev => {
                let delta = if matches!(action, Action::Next) {
                    1
                } else {
                    -1
                };
                if self.advance(delta) {
                    if self.main.state == PlayState::Stopped {
                        let index = self.playlist.current;
                        if let Some(e) = self.playlist.current() {
                            self.main.title = format!("{}. {}", index + 1, e.display_title());
                        }
                        self.pl.ensure_visible(index);
                    } else {
                        self.play_current();
                    }
                }
            }
            Action::Seek(frac) => {
                if let Some(d) = self.main.duration {
                    self.audio.seek(frac * d);
                }
            }
            Action::Volume(v) => self.audio.set_volume(v),
            Action::Balance(b) => self.audio.set_balance(b),
            Action::Eject => self.open_dialog(Dialog::Open, window),
            Action::CycleVis => self.vis.mode = self.vis.mode.next(),
        }
    }

    pub fn pl_command(&mut self, command: PlCommand, window: &mut Window) {
        match command {
            PlCommand::Play(i) => self.play_index(i),
            PlCommand::Close => self.main.pl = false,
            PlCommand::Transport(a) => self.action(a, window),
            PlCommand::AddFiles => self.open_dialog(Dialog::AddFiles, window),
            PlCommand::AddDir => self.open_dialog(Dialog::AddDir, window),
            PlCommand::LoadList => self.open_dialog(Dialog::LoadList, window),
            PlCommand::SaveList => self.open_dialog(Dialog::SaveList, window),
        }
    }

    pub fn art_command(&mut self, command: ArtCommand) {
        match command {
            ArtCommand::Close => self.art_win.open = false,
        }
    }

    /// Handles an EQ window request; `window_size` bounds where the presets menu can open.
    pub fn eq_command(&mut self, command: EqCommand, origin_y: i32, window_size: (i32, i32)) {
        match command {
            EqCommand::Changed => self.push_eq(),
            EqCommand::Close => self.main.eq = false,
            EqCommand::Presets { x, y } => {
                let mut entries = vec![Entry::item("Reset (flat)", MenuCommand::Preset(None))];
                entries.extend(
                    (eq::PRESETS.iter().enumerate())
                        .map(|(i, (name, ..))| Entry::item(*name, MenuCommand::Preset(Some(i)))),
                );
                let (w, h) = window_size;
                self.menu = Some(Menu::new(entries, x, origin_y + y, w, h));
            }
        }
    }

    /// Opens the right-click menu at (x, y), within a `window_size` window.
    pub fn open_menu(&mut self, x: i32, y: i32, window_size: (i32, i32)) {
        use MenuCommand::*;
        let vis = |label, mode| Entry::item(label, Vis(mode)).radio(self.vis.mode == mode);
        let scales = (crate::SCALES.iter())
            .map(|n| Entry::item(format!("{n}%"), Scale(*n)).radio(self.scale == *n))
            .collect();
        let entries = vec![
            Entry::item("Equalizer", ToggleEq)
                .checked(self.main.eq)
                .shortcut("Ctrl+G"),
            Entry::item("Playlist", TogglePlaylist)
                .checked(self.main.pl)
                .shortcut("Ctrl+E"),
            Entry::item("Album art", ToggleArt)
                .checked(self.art_win.open)
                .shortcut("Ctrl+I"),
            Entry::Separator,
            Entry::submenu(
                "Visualizer",
                vec![
                    vis("Spectrum", vis::Mode::Spectrum),
                    vis("Oscilloscope", vis::Mode::Oscilloscope),
                    vis("Off", vis::Mode::Off),
                ],
            ),
            Entry::submenu("Scale", scales),
            Entry::submenu(
                "Skin",
                vec![
                    Entry::item("Default skin", DefaultSkin).radio(self.skin_path.is_none()),
                    Entry::item("Open skin…", OpenSkin),
                ],
            ),
            Entry::Separator,
            Entry::item("Quit", Quit).shortcut("Ctrl+Q"),
        ];
        let (w, h) = window_size;
        self.menu = Some(Menu::new(entries, x, y, w, h));
    }

    pub fn menu_command(&mut self, command: MenuCommand, window: &mut Window) {
        self.menu = None;
        match command {
            MenuCommand::ToggleEq => self.main.eq = !self.main.eq,
            MenuCommand::TogglePlaylist => self.main.pl = !self.main.pl,
            MenuCommand::ToggleArt => self.art_win.open = !self.art_win.open,
            MenuCommand::Vis(mode) => self.vis.mode = mode,
            MenuCommand::Scale(scale) => self.scale = scale,
            MenuCommand::DefaultSkin => {
                self.skin = Skin::base();
                self.skin_path = None;
            }
            MenuCommand::OpenSkin => self.open_dialog(Dialog::Skin, window),
            MenuCommand::Quit => self.quit = true,
            MenuCommand::Preset(preset) => self.choose_preset(preset),
        }
    }

    /// Applies an EQ preset, or flat for None.
    fn choose_preset(&mut self, preset: Option<usize>) {
        match preset.and_then(|i| eq::PRESETS.get(i)) {
            Some((_, preamp, bands)) => {
                self.eq.preamp = eq::preset_value(*preamp);
                self.eq.bands = bands.map(eq::preset_value);
            }
            None => {
                self.eq.preamp = 0.5;
                self.eq.bands = [0.5; 10];
            }
        }
        self.push_eq();
    }

    fn push_eq(&self) {
        self.audio
            .set_eq(self.eq.on, self.eq.preamp, &self.eq.bands);
    }

    // File dialogs run asynchronously; results come back through a channel.

    fn open_dialog(&self, kind: Dialog, window: &Window) {
        let tx = self.dialog_tx.clone();
        let callback: DialogCallback = Box::new(move |result, _| match result {
            Ok(paths) if !paths.is_empty() => {
                let _ = tx.send((kind, paths));
            }
            Ok(_) => {}
            Err(e) => eprintln!("libreamp: file dialog: {e:?}"),
        });
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let result = match kind {
            Dialog::Open | Dialog::AddFiles => {
                show_open_file_dialog(&[AUDIO_FILTER], home, true, window, callback)
            }
            Dialog::AddDir => {
                show_open_folder_dialog(home, true, window, callback);
                Ok(())
            }
            Dialog::LoadList => {
                show_open_file_dialog(&[LIST_FILTER], home, false, window, callback)
            }
            Dialog::SaveList => show_save_file_dialog(&[LIST_FILTER], home, window, callback),
            Dialog::Skin => show_open_file_dialog(&[SKIN_FILTER], home, false, window, callback),
        };
        if let Err(e) = result {
            eprintln!("libreamp: file dialog: {e:?}");
        }
    }

    fn dialog_result(&mut self, kind: Dialog, paths: Vec<PathBuf>) {
        match kind {
            Dialog::Open => {
                self.playlist.clear();
                self.add_paths(&paths);
                self.play_index(0);
            }
            Dialog::AddFiles | Dialog::AddDir | Dialog::LoadList => {
                let start = self.playlist.len();
                self.add_paths(&paths);
                if self.main.state == PlayState::Stopped && start == 0 {
                    self.play_index(0);
                }
            }
            Dialog::SaveList => {
                let mut path = paths[0].clone();
                if path.extension().is_none() {
                    path.set_extension("m3u");
                }
                if let Err(e) = self.playlist.save_list(&path) {
                    eprintln!("libreamp: {}: {e}", path.display());
                }
            }
            Dialog::Skin => self.load_skin(&paths[0]),
        }
    }

    fn load_skin(&mut self, path: &Path) {
        match Skin::load(path) {
            Ok(s) => {
                self.skin = s;
                self.skin_path = std::fs::canonicalize(path).ok();
            }
            Err(e) => eprintln!("libreamp: {e}"),
        }
    }

    pub fn add_paths(&mut self, paths: &[PathBuf]) {
        for p in paths {
            self.playlist.add(p);
        }
    }

    // Drag and drop: onto the playlist it appends, onto the main window alone it replaces.

    pub fn drop_file(&mut self, path: &Path) {
        if crate::skin::is_skin(path) {
            self.load_skin(path);
            return;
        }
        if !self.drop_started {
            self.drop_started = true;
            if !self.main.pl {
                self.playlist.clear();
            }
            self.drop_first_new = self.playlist.len();
        }
        self.playlist.add(path);
    }

    pub fn drop_complete(&mut self) {
        if !std::mem::take(&mut self.drop_started) {
            return;
        }
        if !self.main.pl || self.main.state == PlayState::Stopped {
            self.play_index(self.drop_first_new);
        }
    }

    /// Advances animations: marquee and visualizer. Called at a fixed rate.
    pub fn tick(&mut self) {
        self.main.tick();
        let playing = self.main.state == PlayState::Playing
            && self.vis.mode != vis::Mode::Off
            && self.audio.vis_samples(&mut self.vis_samples);
        let samples = playing.then_some(self.vis_samples.as_slice());
        self.vis.update(samples, self.audio.rate());
    }

    /// Per-frame housekeeping: dialog results, tags, position, end of track.
    pub fn update(&mut self) {
        // The album art panel uses the skin's own gen.bmp frame, or none.
        self.art_win.framed = self.skin.has_own("gen");
        while let Ok((kind, paths)) = self.dialog_rx.try_recv() {
            self.dialog_result(kind, paths);
        }
        self.playlist.poll_tags();
        // Follows the current track however it changes: playing, skipping, editing the list.
        let current = self.playlist.current().map(|e| e.path.as_path());
        self.art_loader.request(current);
        if let Some(art) = self.art_loader.poll() {
            self.art = art;
        }
        if self.main.state != PlayState::Stopped {
            self.main.elapsed = self.audio.position();
        }
        if self.main.state == PlayState::Playing && self.audio.finished() {
            if self.advance(1) {
                self.play_current();
            } else {
                self.stop();
                self.playlist.current = 0;
            }
        }
    }

    /// Restores the saved playlist (without playing) when nothing was given on the command line.
    /// Restores the saved playlist, with the current track paused at `position` seconds if it
    /// was playing or paused when LibreAmp last closed.
    pub fn restore_playlist(&mut self, current: usize, position: Option<f32>) {
        if let Some(path) = config::playlist_path()
            && path.exists()
            && let Err(e) = self.playlist.load_list(&path)
        {
            eprintln!("libreamp: {}: {e}", path.display());
        }
        if current < self.playlist.len() {
            self.playlist.current = current;
            let title = self.playlist.entries[current].display_title();
            self.main.title = format!("{}. {}", current + 1, title);
            self.pl.ensure_visible(current);
            if let Some(secs) = position
                && self.load_current(true)
            {
                if secs > 0.0 && self.main.duration.is_none_or(|d| secs < d) {
                    self.audio.seek(secs);
                }
                self.main.elapsed = self.audio.position();
            }
        }
    }

    pub fn save(&self) {
        let result = config::save(&self.state()).and_then(|()| match config::playlist_path() {
            Some(path) => config::write_atomic(&path, &self.playlist.to_m3u()),
            None => Ok(()),
        });
        if let Err(e) = result {
            eprintln!("libreamp: saving state: {e}");
        }
    }
}
