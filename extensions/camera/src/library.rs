//! The library: named tracks and positions, kept in the extension's store and
//! offered to actions.

use serde::{Deserialize, Serialize};
use vrc_we_sdk::{Command, Item, Value};

use crate::motion::{
    clamp_step, join_ramps, key_optics, key_ramp, ramp_of_pace, reclose, step_of, Loop, Optics,
    Step, IN_RAMP, OPTICS, OUT_RAMP, PATH_MAX, SMOOTH, WIRE,
};
use crate::widgets::UNREADABLE;
use crate::{
    pose_list, pose_value, steps_of, steps_value, Camera, LIBRARY_KEY, NAME_MAX, POSITIONS, TRACKS,
};

/// A name as typed, trimmed and without control characters, at most
/// [`NAME_MAX`] characters. `None` if it is empty.
pub(crate) fn clean_name(name: &str) -> Option<String> {
    let name: String = name
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(NAME_MAX)
        .collect();
    (!name.is_empty()).then_some(name)
}

/// `base` if it is free, or the first free `base (n)`, at most [`NAME_MAX`]
/// characters.
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
    /// The store key: `track_<n>`.
    pub(crate) key: String,
    pub(crate) steps: Vec<Step>,
    /// The world pose of the first key, where the editor draws the track and
    /// adds keys from. It isn't used when playing and isn't shared.
    pub(crate) start: Option<[f32; 6]>,
    /// How it ends. Every edit keeps the last step of a closed track leading
    /// back to the start ([`reclose`]).
    pub(crate) looping: Loop,
    /// The camera's optics at the start; the steps hold those of the other
    /// keys. `None` if the track was saved before keys had optics.
    pub(crate) optics: Option<Optics>,
}

impl Track {
    /// The optics at each key, if it has any.
    pub(crate) fn key_optics(&self) -> Option<Vec<Optics>> {
        self.optics.map(|first| key_optics(first, &self.steps))
    }

    /// Gives every key the optics `now`, if the track has none.
    pub(crate) fn keep_optics(&mut self, now: Optics) {
        if self.optics.is_none() {
            self.optics = Some(now);
            for s in &mut self.steps {
                s[OPTICS..].copy_from_slice(&now);
            }
        }
    }

    /// Edits the track and then restores its invariants: one ramp per key, the
    /// start keeping its own, and a closed track leading back to its start.
    pub(crate) fn edit<T>(&mut self, edit: impl FnOnce(&mut Self) -> T) -> T {
        let start = key_ramp(&self.steps, 0);
        let result = edit(self);
        self.settle(start);
        result
    }

    /// Restores the track's invariants, where `start` is the ramp of the start.
    fn settle(&mut self, start: f32) {
        let closed = self.looping.closed();
        join_ramps(&mut self.steps, start, closed);
        if closed {
            reclose(&mut self.steps, self.optics);
        }
    }

    /// Sets the ramp of key `key`. The end of a closed track is its start.
    pub(crate) fn set_ramp(&mut self, key: usize, ramp: f32) {
        let start = match key {
            0 => ramp,
            k if k == self.steps.len() && self.looping.closed() => ramp,
            _ => key_ramp(&self.steps, 0),
        };
        if let Some(s) = key.checked_sub(1).and_then(|i| self.steps.get_mut(i)) {
            s[IN_RAMP] = ramp;
        }
        self.settle(start);
    }

    pub(crate) fn seconds(&self) -> f32 {
        self.steps.iter().map(|s| s[6]).sum()
    }

    /// The number of keys: the start and the end of each step, or 0 for an
    /// empty track.
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
    /// The key actions pick it by: `pos_<n>`. Assigned on load if it was saved
    /// without one.
    #[serde(default)]
    pub(crate) key: String,
    pub(crate) pose: [f32; 6],
}

/// The value of the `library` key. Each track's steps are under the track's own
/// key.
#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct Index {
    pub(crate) next: u32,
    pub(crate) tracks: Vec<TrackEntry>,
    pub(crate) positions: Vec<Position>,
    /// The handle lengths of a new key (out, in).
    #[serde(default)]
    pub(crate) key_handles: Option<[f32; 2]>,
    /// The key of the selected track.
    #[serde(default)]
    pub(crate) chosen: Option<String>,
}

/// The value of a track's key. A bare list of steps is the older format, with
/// pace dots in place of ramps ([`from_dots`]).
#[derive(Debug, Serialize, Deserialize)]
struct Kept<T> {
    steps: T,
}

/// Converts pace dots (a position along the step from 0 to 1, and the pace
/// there relative to the average) to the closest ramps. A dot near an end of
/// its step sets the pace of that key, a dot in the middle sets both, and a key
/// with two paces takes their mean.
fn from_dots(steps: &mut [Step]) {
    let asked = |[at, pace]: [f32; 2]| match (pace == 1.0, at) {
        (true, _) => [None, None],
        (false, at) if at < 1.0 / 3.0 => [Some(pace), None],
        (false, at) if at > 2.0 / 3.0 => [None, Some(pace)],
        _ => [Some(3.0 - 2.0 * pace); 2],
    };
    let asks: Vec<[Option<f32>; 2]> = steps
        .iter()
        .map(|s| asked([s[OUT_RAMP], s[IN_RAMP]]))
        .collect();
    let ramp = |key: usize| {
        let into = key.checked_sub(1).and_then(|i| asks[i][1]);
        let out = asks.get(key).and_then(|a| a[0]);
        let pace = match (into, out) {
            (Some(a), Some(b)) => (a + b) / 2.0,
            (a, b) => a.or(b).unwrap_or(1.0),
        };
        ramp_of_pace(pace)
    };
    let start = ramp(0);
    let reached: Vec<f32> = (1..=steps.len()).map(ramp).collect();
    for (s, ramp) in steps.iter_mut().zip(reached) {
        s[IN_RAMP] = ramp;
    }
    join_ramps(steps, start, false);
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
        let kept = Kept {
            steps: t.steps.as_slice(),
        };
        out.push(Command::Store {
            key: t.key.clone(),
            value: serde_json::to_string(&kept).expect("steps serialize"),
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
        let dotted = value.starts_with('[');
        let stored: Result<Vec<Vec<f32>>, _> = match dotted {
            true => serde_json::from_str(&value),
            false => serde_json::from_str(&value).map(|Kept { steps }| steps),
        };
        let Ok(stored) = stored else {
            self.unreadable = true;
            self.note = Some(UNREADABLE.to_owned());
            return;
        };
        let Some(i) = self.tracks.iter().position(|t| t.key == key) else {
            return;
        };
        let t = &mut self.tracks[i];
        t.steps = stored
            .iter()
            .filter_map(|s| {
                let mut step = step_of(s)?;
                // A step without a curve has an even pace.
                if dotted && s.len() < WIRE {
                    step[OUT_RAMP..=IN_RAMP].copy_from_slice(&[0.5, 1.0]);
                }
                Some(step)
            })
            .take(PATH_MAX)
            .collect();
        if dotted {
            from_dots(&mut t.steps);
        }
        t.steps.iter_mut().for_each(|s| *s = clamp_step(*s));
        t.settle(key_ramp(&t.steps, 0));
        if dotted {
            self.save_track(i, out);
        }
    }

    /// The key of the selected track.
    pub(crate) fn chosen_key(&self) -> Option<String> {
        self.track
            .and_then(|i| self.tracks.get(i))
            .map(|t| t.key.clone())
    }

    /// Publishes the lists that actions pick from.
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

    /// The data of an item: a track's steps or a position's pose. `None` if it
    /// no longer exists.
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

    /// Adds an item that came with a mapping from another client, or finds an
    /// identical one, and returns it. Its name is made unique.
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
                let mut steps: Vec<Step> = steps_of(data)
                    .into_iter()
                    .take(PATH_MAX)
                    .map(clamp_step)
                    .collect();
                let start = key_ramp(&steps, 0);
                join_ramps(&mut steps, start, false);
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

    /// A new store key: `<prefix>_<n>`.
    pub(crate) fn fresh_key(&mut self, prefix: &str) -> String {
        let key = format!("{prefix}_{}", self.next_key);
        self.next_key += 1;
        key
    }

    /// Adds and selects a track, and stores it.
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

    /// `base` if no track has that name, or the first free `base (n)`.
    pub(crate) fn copy_name(&self, base: &str) -> String {
        free_name(base, |n| self.name_taken(n))
    }

    /// Edits the steps of the selected track and stores it.
    pub(crate) fn edit_track(&mut self, edit: impl FnOnce(&mut Vec<Step>), out: &mut Vec<Command>) {
        let Some(i) = self.track.filter(|i| *i < self.tracks.len()) else {
            return;
        };
        self.tracks[i].edit(|t| edit(&mut t.steps));
        self.save_track(i, out);
    }
}
