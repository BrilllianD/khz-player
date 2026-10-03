use std::collections::BTreeSet;

use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

use crate::library::Track;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

impl Repeat {
    pub fn cycle(self) -> Self {
        match self {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Repeat::Off => "REP",
            Repeat::All => "REP ALL",
            Repeat::One => "REP 1",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Playlist {
    /// Database row id; `None` until first saved.
    pub id: Option<i64>,
    pub name: String,
    pub tracks: Vec<Track>,
    pub current: Option<usize>,
    pub selected: BTreeSet<usize>,
    /// Anchor for shift-click range selection.
    pub anchor: Option<usize>,
    /// Set on any change that should be persisted.
    pub dirty: bool,
    /// Where playback continues after the playing row was removed: the index
    /// of the row that followed it (`len` when it was last).
    resume_at: Option<usize>,
    shuffle_order: Vec<usize>,
    shuffle_pos: usize,
}

impl Playlist {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn current_track(&self) -> Option<&Track> {
        self.current.and_then(|i| self.tracks.get(i))
    }

    pub fn total_duration_ms(&self) -> u64 {
        self.tracks.iter().filter_map(|t| t.duration_ms).sum()
    }

    pub fn add(&mut self, tracks: impl IntoIterator<Item = Track>) {
        self.tracks.extend(tracks);
        self.invalidate_shuffle();
        self.dirty = true;
    }

    pub fn clear(&mut self) {
        self.tracks.clear();
        self.current = None;
        self.resume_at = None;
        self.selected.clear();
        self.anchor = None;
        self.invalidate_shuffle();
        self.dirty = true;
    }

    /// Removes the given indices, keeping `current` pointing at the same track.
    /// If the current track is removed, `current` becomes `None` and the next
    /// track is the one that followed it.
    pub fn remove(&mut self, indices: &BTreeSet<usize>) {
        if indices.is_empty() {
            return;
        }
        let mut new_current = None;
        let mut resume_at = None;
        let mut kept = Vec::with_capacity(self.tracks.len());
        for (i, t) in std::mem::take(&mut self.tracks).into_iter().enumerate() {
            if self.resume_at == Some(i) {
                resume_at = Some(kept.len());
            }
            if indices.contains(&i) {
                if self.current == Some(i) {
                    resume_at = Some(kept.len());
                }
                continue;
            }
            if self.current == Some(i) {
                new_current = Some(kept.len());
            }
            kept.push(t);
        }
        if self.resume_at.is_some() && resume_at.is_none() {
            // It pointed past the end.
            resume_at = Some(kept.len());
        }
        self.tracks = kept;
        self.current = new_current;
        self.resume_at = resume_at;
        self.selected.clear();
        self.anchor = None;
        self.invalidate_shuffle();
        self.dirty = true;
    }

    pub fn remove_selected(&mut self) {
        let sel = std::mem::take(&mut self.selected);
        self.remove(&sel);
    }

    /// Keeps only selected tracks.
    pub fn crop_selected(&mut self) {
        let inverse: BTreeSet<usize> = (0..self.tracks.len())
            .filter(|i| !self.selected.contains(i))
            .collect();
        self.remove(&inverse);
    }

    /// Removes entries whose files no longer exist.
    pub fn remove_dead(&mut self) {
        let dead: BTreeSet<usize> = self
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.path.exists())
            .map(|(i, _)| i)
            .collect();
        self.remove(&dead);
    }

    pub fn remove_duplicates(&mut self) {
        let mut seen = std::collections::HashSet::new();
        let dups: BTreeSet<usize> = self
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| !seen.insert(t.path.clone()))
            .map(|(i, _)| i)
            .collect();
        self.remove(&dups);
    }

    /// Removes later copies of the same song in other files: same artist and
    /// title (case-insensitive) and durations within 2 s. Tracks without both
    /// tags are kept.
    pub fn remove_duplicate_songs(&mut self) {
        let mut kept: Vec<(String, String, Option<u64>)> = Vec::new();
        let mut dups = BTreeSet::new();
        for (i, t) in self.tracks.iter().enumerate() {
            let (Some(artist), Some(title)) = (&t.artist, &t.title) else {
                continue;
            };
            let key = (artist.trim().to_lowercase(), title.trim().to_lowercase());
            let same = kept.iter().any(|(a, ti, d)| {
                *a == key.0
                    && *ti == key.1
                    && match (d, t.duration_ms) {
                        (Some(a), Some(b)) => a.abs_diff(b) <= 2000,
                        _ => true,
                    }
            });
            if same {
                dups.insert(i);
            } else {
                kept.push((key.0, key.1, t.duration_ms));
            }
        }
        self.remove(&dups);
    }

    /// Reorders tracks with `cmp`, keeping `current` on the same track.
    pub fn sort_by<F>(&mut self, mut cmp: F)
    where
        F: FnMut(&Track, &Track) -> std::cmp::Ordering,
    {
        let cur_path = self.current_track().map(|t| t.path.clone());
        self.tracks.sort_by(|a, b| cmp(a, b));
        self.current = cur_path.and_then(|p| self.tracks.iter().position(|t| t.path == p));
        self.resume_at = None;
        self.selected.clear();
        self.anchor = None;
        self.invalidate_shuffle();
        self.dirty = true;
    }

    /// Moves the selected tracks as one block so it starts at gap `to`
    /// (0..=len, counted in the list before the move). Order inside the block
    /// is kept; `current`, selection and anchor follow their tracks.
    /// Returns false when nothing moved.
    pub fn move_selected(&mut self, to: usize) -> bool {
        let n = self.tracks.len();
        let to = to.min(n);
        let order: Vec<usize> = (0..to)
            .filter(|i| !self.selected.contains(i))
            .chain(self.selected.iter().copied().filter(|&i| i < n))
            .chain((to..n).filter(|i| !self.selected.contains(i)))
            .collect();
        if order.iter().enumerate().all(|(new, &old)| new == old) {
            return false;
        }
        let mut new_pos = vec![0; n];
        for (new, &old) in order.iter().enumerate() {
            new_pos[old] = new;
        }
        let mut old: Vec<Option<Track>> =
            std::mem::take(&mut self.tracks).into_iter().map(Some).collect();
        self.tracks = order
            .iter()
            .map(|&i| old[i].take().expect("index used once"))
            .collect();
        self.current = self.current.map(|c| new_pos[c]);
        self.resume_at = None;
        self.anchor = self.anchor.map(|a| new_pos[a]);
        self.selected = self.selected.iter().map(|&i| new_pos[i]).collect();
        self.invalidate_shuffle();
        self.dirty = true;
        true
    }

    pub fn reverse(&mut self) {
        self.tracks.reverse();
        let n = self.tracks.len();
        self.current = self.current.map(|c| n - 1 - c);
        self.resume_at = None;
        self.selected.clear();
        self.anchor = None;
        self.invalidate_shuffle();
        self.dirty = true;
    }

    pub fn randomize(&mut self) {
        let cur_path = self.current_track().map(|t| t.path.clone());
        self.tracks.shuffle(&mut rand::rng());
        self.current = cur_path.and_then(|p| self.tracks.iter().position(|t| t.path == p));
        self.resume_at = None;
        self.selected.clear();
        self.anchor = None;
        self.invalidate_shuffle();
        self.dirty = true;
    }

    pub fn select_all(&mut self) {
        self.selected = (0..self.tracks.len()).collect();
    }

    pub fn select_none(&mut self) {
        self.selected.clear();
    }

    pub fn invert_selection(&mut self) {
        self.selected = (0..self.tracks.len())
            .filter(|i| !self.selected.contains(i))
            .collect();
    }

    fn invalidate_shuffle(&mut self) {
        self.shuffle_order.clear();
        self.shuffle_pos = 0;
    }

    /// Builds a fresh shuffle order with the current track first.
    fn ensure_shuffle(&mut self) {
        if self.shuffle_order.len() == self.tracks.len() && !self.shuffle_order.is_empty() {
            return;
        }
        self.reshuffle();
    }

    fn reshuffle(&mut self) {
        let mut order: Vec<usize> = (0..self.tracks.len()).collect();
        order.shuffle(&mut rand::rng());
        if let Some(c) = self.current
            && let Some(p) = order.iter().position(|&i| i == c)
        {
            order.swap(0, p);
        }
        self.shuffle_order = order;
        self.shuffle_pos = 0;
    }

    /// Syncs the shuffle cursor with `current` after an explicit jump.
    fn sync_shuffle_pos(&mut self) {
        if let Some(c) = self.current
            && let Some(p) = self.shuffle_order.iter().position(|&i| i == c)
        {
            self.shuffle_pos = p;
        }
    }

    /// Computes the next index without changing state.
    /// `auto` is true when advancing because a track finished.
    pub fn peek_next(&mut self, repeat: Repeat, shuffle: bool, auto: bool) -> Option<usize> {
        if self.tracks.is_empty() {
            return None;
        }
        if auto && repeat == Repeat::One {
            return self.current.or(self.resume_row()).or(Some(0));
        }
        if shuffle {
            self.ensure_shuffle();
            self.sync_shuffle_pos();
            if self.current.is_none() {
                return self.shuffle_order.first().copied();
            }
            match self.shuffle_order.get(self.shuffle_pos + 1) {
                Some(&i) => Some(i),
                // A wrap reshuffles, which cannot be predicted ahead of time.
                None if repeat != Repeat::Off && !auto => Some(self.shuffle_order[0]),
                None => None,
            }
        } else {
            // The row after the current one, or the row that followed a
            // removed current one.
            let after = match (self.current, self.resume_at) {
                (Some(c), _) => c + 1,
                (None, Some(r)) => r,
                (None, None) => 0,
            };
            if after < self.tracks.len() {
                Some(after)
            } else if repeat == Repeat::All || (repeat == Repeat::One && !auto) {
                Some(0)
            } else {
                None
            }
        }
    }

    /// The row that followed a removed current track, if it still exists.
    fn resume_row(&self) -> Option<usize> {
        self.resume_at.filter(|&r| r < self.tracks.len())
    }

    /// Advances to the next track and returns its index.
    pub fn next(&mut self, repeat: Repeat, shuffle: bool, auto: bool) -> Option<usize> {
        if self.tracks.is_empty() {
            return None;
        }
        if auto && repeat == Repeat::One {
            self.current = self.current.or(self.resume_row()).or(Some(0));
            self.resume_at = None;
            return self.current;
        }
        if shuffle {
            self.ensure_shuffle();
            self.sync_shuffle_pos();
            if self.current.is_none() {
                self.resume_at = None;
                self.shuffle_pos = 0;
            } else if self.shuffle_pos + 1 < self.shuffle_order.len() {
                self.shuffle_pos += 1;
            } else if repeat != Repeat::Off {
                let last = self.current;
                self.current = None;
                self.reshuffle();
                // Avoid playing the same track twice in a row across the wrap.
                if self.shuffle_order.len() > 1 && self.shuffle_order[0] == last.unwrap_or(usize::MAX) {
                    self.shuffle_order.swap(0, 1);
                }
                self.shuffle_pos = 0;
            } else {
                return None;
            }
            self.current = Some(self.shuffle_order[self.shuffle_pos]);
            self.current
        } else {
            let n = self.peek_next(repeat, false, auto)?;
            self.current = Some(n);
            self.resume_at = None;
            Some(n)
        }
    }

    pub fn prev(&mut self, repeat: Repeat, shuffle: bool) -> Option<usize> {
        if self.tracks.is_empty() {
            return None;
        }
        if shuffle {
            self.ensure_shuffle();
            self.sync_shuffle_pos();
            if self.current.is_some() && self.shuffle_pos > 0 {
                self.shuffle_pos -= 1;
            }
            self.current = Some(self.shuffle_order[self.shuffle_pos]);
            self.resume_at = None;
            return self.current;
        }
        // A removed current track sat just before `resume_at`.
        let n = match (self.current, self.resume_at) {
            (Some(c), _) | (None, Some(c)) if c > 0 => c - 1,
            (Some(_), _) | (None, Some(_)) if repeat != Repeat::Off => self.tracks.len() - 1,
            _ => 0,
        };
        self.current = Some(n);
        self.resume_at = None;
        Some(n)
    }

    /// Explicitly jumps to `index` (double click, Enter, jump dialog).
    /// Like `next`/`prev`, this does not mark the list dirty: the current
    /// track is saved through the config, not the playlist table.
    pub fn set_current(&mut self, index: usize) {
        if index < self.tracks.len() {
            self.current = Some(index);
            self.resume_at = None;
            self.sync_shuffle_pos();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn pl(n: usize) -> Playlist {
        let mut p = Playlist::new("t");
        p.add((0..n).map(|i| Track::from_path(PathBuf::from(format!("/m/{i}.mp3")))));
        p
    }

    #[test]
    fn linear_no_repeat() {
        let mut p = pl(3);
        assert_eq!(p.next(Repeat::Off, false, true), Some(0));
        assert_eq!(p.next(Repeat::Off, false, true), Some(1));
        assert_eq!(p.next(Repeat::Off, false, true), Some(2));
        assert_eq!(p.next(Repeat::Off, false, true), None);
        assert_eq!(p.current, Some(2));
        assert_eq!(p.prev(Repeat::Off, false), Some(1));
        assert_eq!(p.prev(Repeat::Off, false), Some(0));
        assert_eq!(p.prev(Repeat::Off, false), Some(0));
    }

    #[test]
    fn repeat_all_wraps() {
        let mut p = pl(2);
        p.set_current(1);
        assert_eq!(p.peek_next(Repeat::All, false, true), Some(0));
        assert_eq!(p.next(Repeat::All, false, true), Some(0));
        assert_eq!(p.prev(Repeat::All, false), Some(1));
    }

    #[test]
    fn repeat_one_auto_stays_manual_moves() {
        let mut p = pl(3);
        p.set_current(1);
        assert_eq!(p.next(Repeat::One, false, true), Some(1));
        assert_eq!(p.next(Repeat::One, false, false), Some(2));
        assert_eq!(p.next(Repeat::One, false, false), Some(0));
    }

    #[test]
    fn shuffle_covers_all_once() {
        let mut p = pl(20);
        let mut seen = BTreeSet::new();
        while let Some(i) = p.next(Repeat::Off, true, true) {
            assert!(seen.insert(i), "track {i} repeated");
        }
        assert_eq!(seen.len(), 20);
    }

    #[test]
    fn shuffle_peek_matches_next() {
        let mut p = pl(10);
        for _ in 0..9 {
            let peek = p.peek_next(Repeat::Off, true, true);
            assert_eq!(p.next(Repeat::Off, true, true), peek);
        }
    }

    #[test]
    fn shuffle_prev_goes_back() {
        let mut p = pl(10);
        let a = p.next(Repeat::Off, true, false).unwrap();
        let b = p.next(Repeat::Off, true, false).unwrap();
        assert_ne!(a, b);
        assert_eq!(p.prev(Repeat::Off, true), Some(a));
    }

    #[test]
    fn remove_keeps_current() {
        let mut p = pl(5);
        p.set_current(3);
        p.remove(&[0, 1].into_iter().collect());
        assert_eq!(p.current, Some(1));
        assert_eq!(p.current_track().unwrap().path, PathBuf::from("/m/3.mp3"));
        p.remove(&[1].into_iter().collect());
        assert_eq!(p.current, None);
    }

    fn path_of(p: &Playlist) -> String {
        p.current_track().unwrap().path.display().to_string()
    }

    #[test]
    fn next_after_current_removed() {
        let mut p = pl(5);
        p.set_current(2);
        p.remove(&[2].into());
        assert_eq!(p.current, None);
        assert_eq!(p.peek_next(Repeat::Off, false, true), Some(2));
        assert_eq!(p.next(Repeat::Off, false, true), Some(2));
        assert_eq!(path_of(&p), "/m/3.mp3");
        assert_eq!(p.next(Repeat::Off, false, true), Some(3));
    }

    #[test]
    fn next_after_last_removed_repeat_modes() {
        let mut p = pl(3);
        p.set_current(2);
        p.remove(&[2].into());
        assert_eq!(p.peek_next(Repeat::Off, false, true), None);
        assert_eq!(p.next(Repeat::Off, false, true), None);
        assert_eq!(p.peek_next(Repeat::All, false, true), Some(0));
        assert_eq!(p.next(Repeat::All, false, true), Some(0));
    }

    #[test]
    fn resume_survives_more_removals_and_appends() {
        let mut p = pl(6);
        p.set_current(3);
        p.remove(&[3].into());
        // Removing rows before and at the resume point shifts it.
        p.remove(&[0, 3].into());
        assert_eq!(p.next(Repeat::Off, false, true), Some(2));
        assert_eq!(path_of(&p), "/m/5.mp3");

        let mut p = pl(2);
        p.set_current(1);
        p.remove(&[1].into());
        p.add([Track::from_path(PathBuf::from("/m/new.mp3"))]);
        assert_eq!(p.next(Repeat::Off, false, true), Some(1));
        assert_eq!(path_of(&p), "/m/new.mp3");
    }

    #[test]
    fn prev_after_current_removed() {
        let mut p = pl(5);
        p.set_current(2);
        p.remove(&[2].into());
        assert_eq!(p.prev(Repeat::Off, false), Some(1));
        assert_eq!(path_of(&p), "/m/1.mp3");

        let mut p = pl(3);
        p.set_current(0);
        p.remove(&[0].into());
        assert_eq!(p.prev(Repeat::All, false), Some(1));
    }

    #[test]
    fn peek_matches_next_linear() {
        for repeat in [Repeat::Off, Repeat::All, Repeat::One] {
            for auto in [true, false] {
                for start in 0..4 {
                    let mut p = pl(4);
                    p.set_current(start);
                    for _ in 0..6 {
                        let peek = p.peek_next(repeat, false, auto);
                        assert_eq!(p.next(repeat, false, auto), peek, "{repeat:?} {auto} {start}");
                    }
                }
            }
        }
    }

    #[test]
    fn remove_current_shuffle_continues() {
        let mut p = pl(5);
        p.set_current(2);
        p.next(Repeat::Off, true, true);
        let cur = p.current.unwrap();
        p.remove(&[cur].into());
        let peek = p.peek_next(Repeat::Off, true, true);
        let next = p.next(Repeat::Off, true, true);
        assert_eq!(next, peek);
        assert!(next.is_some_and(|i| i < 4));
    }

    #[test]
    fn reorder_clears_anchor() {
        let mut p = pl(4);
        p.anchor = Some(1);
        p.reverse();
        assert_eq!(p.anchor, None);
        p.anchor = Some(1);
        p.sort_by(|a, b| a.path.cmp(&b.path));
        assert_eq!(p.anchor, None);
        p.anchor = Some(1);
        p.randomize();
        assert_eq!(p.anchor, None);
    }

    #[test]
    fn set_current_does_not_dirty() {
        let mut p = pl(3);
        p.dirty = false;
        p.set_current(1);
        p.next(Repeat::Off, false, false);
        p.prev(Repeat::Off, false);
        assert!(!p.dirty);
    }

    #[test]
    fn empty_playlist() {
        let mut p = pl(0);
        assert_eq!(p.next(Repeat::All, true, true), None);
        assert_eq!(p.prev(Repeat::All, false), None);
    }

    fn names(p: &Playlist) -> Vec<String> {
        p.tracks.iter().map(|t| t.display_title()).collect()
    }

    #[test]
    fn move_selected_down_and_up() {
        let mut p = pl(5);
        p.selected = [1].into();
        assert!(p.move_selected(4));
        assert_eq!(names(&p), ["0", "2", "3", "1", "4"]);
        assert_eq!(p.selected, [3].into());

        assert!(p.move_selected(0));
        assert_eq!(names(&p), ["1", "0", "2", "3", "4"]);
        assert_eq!(p.selected, [0].into());
        assert!(p.dirty);
    }

    #[test]
    fn move_selected_block_keeps_order_and_current() {
        let mut p = pl(6);
        p.current = Some(4);
        p.selected = [0, 2, 4].into();
        p.anchor = Some(2);
        assert!(p.move_selected(6));
        assert_eq!(names(&p), ["1", "3", "5", "0", "2", "4"]);
        assert_eq!(p.selected, [3, 4, 5].into());
        assert_eq!(p.current, Some(5));
        assert_eq!(p.anchor, Some(4));
        assert_eq!(p.current_track().unwrap().display_title(), "4");
    }

    #[test]
    fn move_selected_noop() {
        let mut p = pl(4);
        p.dirty = false;
        p.selected = [1, 2].into();
        // Dropping inside or at either edge of the block changes nothing.
        for to in [1, 2, 3] {
            assert!(!p.move_selected(to));
        }
        p.selected.clear();
        assert!(!p.move_selected(0));
        assert!(!p.dirty);
    }

    #[test]
    fn duplicate_songs_by_tags() {
        let song = |path: &str, artist: Option<&str>, title: &str, ms: u64| {
            let mut t = Track::from_path(PathBuf::from(path));
            t.artist = artist.map(Into::into);
            t.title = Some(title.into());
            t.duration_ms = Some(ms);
            t
        };
        let mut p = Playlist::new("t");
        p.add([
            song("/a.mp3", Some("Ария"), "Беспечный ангел", 200_000),
            song("/b.mp3", Some("ария "), "беспечный ангел", 201_500),
            song("/c.mp3", Some("Ария"), "Беспечный ангел", 400_000),
            song("/d.mp3", None, "Беспечный ангел", 200_000),
            song("/e.mp3", Some("Kino"), "Gruppa krovi", 280_000),
        ]);
        p.current = Some(4);
        p.remove_duplicate_songs();
        let paths: Vec<_> = p.tracks.iter().map(|t| t.path.to_str().unwrap()).collect();
        assert_eq!(paths, ["/a.mp3", "/c.mp3", "/d.mp3", "/e.mp3"]);
        assert_eq!(p.current, Some(3));
    }
}
