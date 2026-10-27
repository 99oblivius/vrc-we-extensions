//! The library: named tracks and named positions, kept in the
//! extension's store and offered to actions by key.

use serde::{Deserialize, Serialize};
use vrc_we_sdk::{Command, Item, Value};

use crate::motion::{
    clamp_step, key_optics, reclose, step_of, Loop, Optics, Step, OPTICS, PATH_MAX, SMOOTH, WIRE,
};
use crate::widgets::UNREADABLE;
use crate::{
    pose_list, pose_value, steps_of, steps_value, Camera, LIBRARY_KEY, NAME_MAX, POSITIONS, TRACKS,
};

/// A name as people type it: trimmed, at most [`NAME_MAX`] characters, no
/// control characters; none when nothing is left.
pub(crate) fn clean_name(name: &str) -> Option<String> {
    let name: String = name
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(NAME_MAX)
        .collect();
    (!name.is_empty()).then_some(name)
}

/// `base` when `taken` says it's free, else the first free `base (n)`,
/// `base` cut so it stays within [`NAME_MAX`] characters.
pub(crate) fn free_name(base: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(base) {
        return base.to_owned();
    }
    let cut: String = base.chars().take(NAME_MAX - 6).collect();
    vrc_we_sdk::unique_label(&cut, taken)
}

/// A named path.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Track {
    pub(crate) name: String,
    /// Its store key: `track_<n>`.
    pub(crate) key: String,
    pub(crate) steps: Vec<Step>,
    /// The world pose its first key was set at: where the editor draws it
    /// and adds keys from. Plays never read it, and it never leaves this
    /// client (a track's data is its steps).
    pub(crate) start: Option<[f32; 6]>,
    /// Closed, its last step is the way back to its start, kept so by
    /// every edit ([`reclose`]); looping, a play goes round until stopped.
    pub(crate) looping: Loop,
    /// The camera's optics at its start; each step holds those at the key
    /// it reaches. None for a track kept before keys held them: its plays
    /// leave the settings alone until a key is next set from the camera.
    pub(crate) optics: Option<Optics>,
}

impl Track {
    /// Each key's optics, when it holds them.
    pub(crate) fn key_optics(&self) -> Option<Vec<Optics>> {
        self.optics.map(|first| key_optics(first, &self.steps))
    }

    /// It holds optics from now on: a track without gives every key `now`.
    pub(crate) fn keep_optics(&mut self, now: Optics) {
        if self.optics.is_none() {
            self.optics = Some(now);
            for s in &mut self.steps {
                s[OPTICS..].copy_from_slice(&now);
            }
        }
    }

    /// A closed track's last step made the way back to its start again.
    pub(crate) fn reclose(&mut self) {
        if self.looping.closed() {
            reclose(&mut self.steps, self.optics);
        }
    }

    pub(crate) fn seconds(&self) -> f32 {
        self.steps.iter().map(|s| s[6]).sum()
    }

    /// Its keys: the start, then the end of each step; none until it has
    /// either.
    pub(crate) fn keys(&self) -> usize {
        match (self.start, self.steps.len()) {
            (None, 0) => 0,
            (_, n) => n + 1,
        }
    }
}

/// A named pose.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Position {
    pub(crate) name: String,
    /// What actions pick it by: `pos_<n>`; a library saved before keys
    /// gets them when it loads.
    #[serde(default)]
    pub(crate) key: String,
    pub(crate) pose: [f32; 6],
}

/// What the `library` key holds; each track's steps are under its key.
#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct Index {
    pub(crate) next: u32,
    pub(crate) tracks: Vec<TrackEntry>,
    pub(crate) positions: Vec<Position>,
    /// The handle lengths a new key takes (out, in): the last chosen key's.
    #[serde(default)]
    pub(crate) key_handles: Option<[f32; 2]>,
    /// The chosen track, by key.
    #[serde(default)]
    pub(crate) chosen: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct TrackEntry {
    pub(crate) name: String,
    pub(crate) key: String,
    #[serde(default)]
    pub(crate) start: Option<[f32; 6]>,
    #[serde(default)]
    pub(crate) optics: Option<Optics>,
    #[serde(default)]
    pub(crate) looping: Loop,
}

impl Camera {
    pub(crate) fn save_index(&mut self, out: &mut Vec<Command>) {
        if self.unreadable {
            return;
        }
        self.library_changed = true;
        self.kept_choice = self.chosen_key();
        let index = Index {
            next: self.next_key,
            tracks: self
                .tracks
                .iter()
                .map(|t| TrackEntry {
                    name: t.name.clone(),
                    key: t.key.clone(),
                    start: t.start,
                    looping: t.looping,
                    optics: t.optics,
                })
                .collect(),
            positions: self.positions.clone(),
            key_handles: Some(self.key_handles),
            chosen: self.kept_choice.clone(),
        };
        out.push(Command::Store {
            key: LIBRARY_KEY.to_owned(),
            value: serde_json::to_string(&index).expect("an index serializes"),
        });
    }

    pub(crate) fn save_track(&mut self, i: usize, out: &mut Vec<Command>) {
        if self.unreadable {
            return;
        }
        self.library_changed = true;
        let t = &self.tracks[i];
        out.push(Command::Store {
            key: t.key.clone(),
            value: serde_json::to_string(&t.steps).expect("steps serialize"),
        });
    }

    pub(crate) fn on_loaded(&mut self, key: &str, value: Option<String>, out: &mut Vec<Command>) {
        let Some(value) = value else { return };
        self.library_changed = true;
        if key == LIBRARY_KEY {
            let index: Index = match serde_json::from_str(&value) {
                Ok(index) => index,
                Err(_) => {
                    self.unreadable = true;
                    self.note = Some(UNREADABLE.to_owned());
                    return;
                }
            };
            self.next_key = index.next;
            self.positions = index.positions;
            self.key_handles = index.key_handles.unwrap_or([SMOOTH[0], SMOOTH[1]]);
            let unkeyed: Vec<usize> = (0..self.positions.len())
                .filter(|i| self.positions[*i].key.is_empty())
                .collect();
            for i in &unkeyed {
                self.positions[*i].key = self.fresh_key("pos");
            }
            self.tracks = index
                .tracks
                .into_iter()
                .map(|e| Track {
                    name: e.name,
                    key: e.key,
                    steps: Vec::new(),
                    start: e.start,
                    looping: e.looping,
                    optics: e.optics,
                })
                .collect();
            self.track = index
                .chosen
                .and_then(|key| self.tracks.iter().position(|t| t.key == key));
            self.key = None;
            self.kept_choice = self.chosen_key();
            if !unkeyed.is_empty() {
                self.save_index(out);
            }
            out.extend(
                self.tracks
                    .iter()
                    .map(|t| Command::Load { key: t.key.clone() }),
            );
            return;
        }
        let stored: Vec<Vec<f32>> = match serde_json::from_str(&value) {
            Ok(stored) => stored,
            Err(_) => {
                self.unreadable = true;
                self.note = Some(UNREADABLE.to_owned());
                return;
            }
        };
        if let Some(t) = self.tracks.iter_mut().find(|t| t.key == key) {
            t.steps = stored
                .iter()
                .filter_map(|s| step_of(s))
                .take(PATH_MAX)
                .map(clamp_step)
                .collect();
        }
    }

    /// The chosen track's key.
    pub(crate) fn chosen_key(&self) -> Option<String> {
        self.track
            .and_then(|i| self.tracks.get(i))
            .map(|t| t.key.clone())
    }

    /// The lists actions pick from, as they stand.
    pub(crate) fn publish_lists(&self, out: &mut Vec<Command>) {
        let item = |key: &str, label: &str| Item {
            key: key.to_owned(),
            label: label.to_owned(),
        };
        out.push(Command::Options {
            list: TRACKS.to_owned(),
            items: self.tracks.iter().map(|t| item(&t.key, &t.name)).collect(),
        });
        out.push(Command::Options {
            list: POSITIONS.to_owned(),
            items: self
                .positions
                .iter()
                .map(|p| item(&p.key, &p.name))
                .collect(),
        });
    }

    /// The data of item `key` of `list`: a track's steps or a position's
    /// pose; none once it's gone.
    pub(crate) fn resolve(&self, list: &str, key: &str) -> Value {
        match list {
            TRACKS => self
                .tracks
                .iter()
                .find(|t| t.key == key)
                .map_or(Value::None, |t| steps_value(&t.steps)),
            POSITIONS => self
                .positions
                .iter()
                .find(|p| p.key == key)
                .map_or(Value::None, |p| pose_list(&p.pose)),
            _ => Value::None,
        }
    }

    /// An item a mapping brought from another client: an identical one
    /// here is reused, else it's kept as `label`, or `label (n)` when a
    /// name is taken. The item it is now.
    pub(crate) fn on_import(
        &mut self,
        list: &str,
        label: &str,
        data: &Value,
        out: &mut Vec<Command>,
    ) -> Option<Item> {
        if self.unreadable {
            return None;
        }
        let name = || clean_name(label).unwrap_or_else(|| "Imported".to_owned());
        match list {
            TRACKS => {
                let steps: Vec<Step> = steps_of(data)
                    .into_iter()
                    .take(PATH_MAX)
                    .map(clamp_step)
                    .collect();
                let same = |t: &Track| {
                    t.steps.len() == steps.len()
                        && t.steps
                            .iter()
                            .zip(&steps)
                            .all(|(a, b)| a[..WIRE] == b[..WIRE])
                };
                if let Some(t) = self.tracks.iter().find(|t| same(t)) {
                    return Some(Item {
                        key: t.key.clone(),
                        label: t.name.clone(),
                    });
                }
                let name = free_name(&name(), |n| self.name_taken(n));
                let chosen = self.track;
                self.insert_track(name, steps, None, out);
                self.track = chosen;
                let t = self.tracks.last().expect("just added");
                Some(Item {
                    key: t.key.clone(),
                    label: t.name.clone(),
                })
            }
            POSITIONS => {
                let pose = pose_value(data)?;
                if let Some(p) = self.positions.iter().find(|p| p.pose == pose) {
                    return Some(Item {
                        key: p.key.clone(),
                        label: p.name.clone(),
                    });
                }
                let name = free_name(&name(), |n| self.position_taken(n));
                let key = self.fresh_key("pos");
                self.positions.push(Position {
                    name: name.clone(),
                    key: key.clone(),
                    pose,
                });
                self.save_index(out);
                Some(Item { key, label: name })
            }
            _ => None,
        }
    }

    pub(crate) fn name_taken(&self, name: &str) -> bool {
        self.tracks.iter().any(|t| t.name == name)
    }

    pub(crate) fn position_taken(&self, name: &str) -> bool {
        self.positions.iter().any(|p| p.name == name)
    }

    /// A store key no item has: `<prefix>_<n>`.
    pub(crate) fn fresh_key(&mut self, prefix: &str) -> String {
        let key = format!("{prefix}_{}", self.next_key);
        self.next_key += 1;
        key
    }

    /// Keeps `steps` from `start` as a new track `name`, chosen, in the
    /// store.
    pub(crate) fn insert_track(
        &mut self,
        name: String,
        steps: Vec<Step>,
        start: Option<[f32; 6]>,
        out: &mut Vec<Command>,
    ) {
        let key = self.fresh_key("track");
        self.tracks.push(Track {
            name,
            key,
            steps,
            start,
            looping: Loop::Open,
            optics: None,
        });
        let i = self.tracks.len() - 1;
        self.track = Some(i);
        self.save_track(i, out);
        self.save_index(out);
    }

    /// `base` when no track has it, else `base (n)`.
    pub(crate) fn copy_name(&self, base: &str) -> String {
        free_name(base, |n| self.name_taken(n))
    }

    /// Edits the chosen track's steps and stores them, a closed one's way
    /// back kept leading to its start.
    pub(crate) fn edit_track(&mut self, edit: impl FnOnce(&mut Vec<Step>), out: &mut Vec<Command>) {
        let Some(i) = self.track.filter(|i| *i < self.tracks.len()) else {
            return;
        };
        let t = &mut self.tracks[i];
        edit(&mut t.steps);
        t.reclose();
        self.save_track(i, out);
    }
}
