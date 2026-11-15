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
use crate::ui::{
    Action, ArtCommand, ArtWindow, EqCommand, EqWindow, MainWindow, PlCommand, PlayState,
    PlaylistWindow, Popup,
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
}

const AUDIO_FILTER: DialogFileFilter = DialogFileFilter {
    name: "Audio and playlists",
    pattern: "mp3;flac;ogg;oga;wav;m4a;mp4;aac;m3u;m3u8;pls",
};
const LIST_FILTER: DialogFileFilter = DialogFileFilter {
    name: "Playlists",
    pattern: "m3u;m3u8;pls",
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
    /// The EQ presets menu, when open.
    pub popup: Option<Popup>,
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
            art_win: ArtWindow::new(state.art_open, state.art_tiles),
            art: None,
            art_loader: ArtLoader::new(),
            popup: None,
            vis: Visualizer::new(state.vis_mode),
            vis_samples: Vec::new(),
            quit: false,
            dialog_tx,
            dialog_rx,
            drop_started: false,
            drop_first_new: 0,
        }
    }

    pub fn state(&self, scale: Option<u32>) -> State {
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
            art_tiles: self.art_win.tiles,
            current: self.playlist.current,
            skin: self.skin_path.clone(),
            scale,
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
        let index = self.playlist.current;
        let Some(entry) = self.playlist.entries.get_mut(index) else {
            return;
        };
        let ui = &mut self.main;
        match self.audio.play(&entry.path) {
            Ok(info) => {
                entry.title.get_or_insert(info.title);
                entry.duration = entry.duration.or(info.duration);
                ui.title = format!("{}. {}", index + 1, entry.display_title());
                ui.duration = info.duration;
                ui.kbps = info.kbps;
                ui.khz = Some((info.rate as f32 / 1000.0).round() as u32);
                ui.channels = info.channels;
                ui.elapsed = 0.0;
                ui.state = PlayState::Playing;
            }
            Err(e) => {
                eprintln!("libreamp: {}: {e}", entry.path.display());
                self.audio.stop();
                ui.title = format!("{}. error: {e}", index + 1);
                ui.state = PlayState::Stopped;
                ui.duration = None;
            }
        }
        self.pl.ensure_visible(index);
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
                let mut items = vec!["Reset (flat)".to_string()];
                items.extend(eq::PRESETS.iter().map(|(name, ..)| name.to_string()));
                let (w, h) = window_size;
                self.popup = Some(Popup::new(items, x, origin_y + y, w, h));
            }
        }
    }

    /// Applies the preset chosen from the presets menu (0 is "reset").
    pub fn choose_preset(&mut self, index: usize) {
        match index.checked_sub(1).and_then(|i| eq::PRESETS.get(i)) {
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
            match Skin::load(path) {
                Ok(s) => {
                    self.skin = s;
                    self.skin_path = std::fs::canonicalize(path).ok();
                }
                Err(e) => eprintln!("libreamp: {e}"),
            }
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
    pub fn restore_playlist(&mut self, current: usize) {
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
        }
    }

    pub fn save(&self, scale: Option<u32>) {
        let result =
            config::save(&self.state(scale)).and_then(|()| match config::playlist_path() {
                Some(path) => config::write_atomic(&path, &self.playlist.to_m3u()),
                None => Ok(()),
            });
        if let Err(e) = result {
            eprintln!("libreamp: saving state: {e}");
        }
    }
}
