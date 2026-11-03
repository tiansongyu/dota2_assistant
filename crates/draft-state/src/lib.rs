//! Conservative draft state. Temporal stability is not evidence of a locked pick.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Team {
    Radiant,
    Dire,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RecognitionState {
    Empty,
    Candidate,
    Confirmed,
    Uncertain,
    Invalid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LockEvidence {
    None,
    /// The profile's distinct lock indicator matched in this frame.
    VisualMarker,
    Manual,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Observation {
    /// 0..4 Radiant, 5..9 Dire. Stable slot IDs, not image-template indices.
    pub slot_id: u8,
    pub hero_id: Option<u32>,
    /// Similarity score only; not a calibrated probability.
    pub confidence: f64,
    pub margin: f64,
    pub empty: bool,
    pub valid: bool,
    pub lock_evidence: LockEvidence,
    pub timestamp: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlotState {
    pub slot_id: u8,
    pub hero_id: Option<u32>,
    pub confidence: f64,
    pub recognition_state: RecognitionState,
    pub timestamp: u64,
    pub source: LockEvidence,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DraftSnapshot {
    pub revision: u64,
    pub draft_screen_detected: bool,
    pub radiant: Vec<SlotState>,
    pub dire: Vec<SlotState>,
    pub own_team: Option<Team>,
    /// Manual bans only until a ban-specific calibrated detector is validated.
    pub banned: Vec<u32>,
}

impl DraftSnapshot {
    pub fn confirmed_heroes(&self, team: Team) -> Vec<u32> {
        let slots = match team {
            Team::Radiant => &self.radiant,
            Team::Dire => &self.dire,
        };
        slots
            .iter()
            .filter(|s| s.recognition_state == RecognitionState::Confirmed)
            .filter_map(|s| s.hero_id)
            .collect()
    }

    pub fn selected_heroes(&self) -> Vec<u32> {
        self.confirmed_heroes(Team::Radiant)
            .into_iter()
            .chain(self.confirmed_heroes(Team::Dire))
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DraftEvent {
    DraftScreenDetected,
    HeroCandidateDetected { slot_id: u8, hero_id: u32 },
    HeroConfirmed { slot_id: u8, hero_id: u32 },
    HeroRemoved { slot_id: u8, hero_id: u32 },
    DraftUpdated { revision: u64 },
    GameWindowLost,
}

#[derive(Debug, thiserror::Error)]
pub enum DraftError {
    #[error("slot_id 必须在 0..10 范围内")]
    InvalidSlot,
    #[error("英雄 ID 必须大于 0")]
    InvalidHero,
    #[error("同一英雄不能占用两个槽位或同时被禁用")]
    DuplicateHero,
    #[error("帧时间戳必须严格递增；重复帧不能增加确认次数")]
    StaleFrame,
    #[error("一帧中不能重复提交同一槽位")]
    DuplicateSlot,
}

#[derive(Clone, Debug)]
struct Memory {
    state: SlotState,
    consecutive: u8,
    pending: Option<u32>,
    empty_frames: u8,
}

#[derive(Clone)]
pub struct DraftTracker {
    slots: Vec<Memory>,
    required_frames: u8,
    last_timestamp: Option<u64>,
    draft_seen: bool,
    absent_frames: u8,
    snapshot: DraftSnapshot,
}

impl Default for DraftTracker {
    fn default() -> Self {
        Self::new(3)
    }
}

impl DraftTracker {
    pub fn new(required_frames: u8) -> Self {
        let slots = (0..10)
            .map(|slot_id| Memory {
                state: SlotState {
                    slot_id,
                    hero_id: None,
                    confidence: 0.0,
                    recognition_state: RecognitionState::Uncertain,
                    timestamp: 0,
                    source: LockEvidence::None,
                },
                consecutive: 0,
                pending: None,
                empty_frames: 0,
            })
            .collect();
        let mut out = Self {
            slots,
            required_frames: required_frames.max(2),
            last_timestamp: None,
            draft_seen: false,
            absent_frames: 0,
            snapshot: DraftSnapshot {
                revision: 0,
                draft_screen_detected: false,
                radiant: vec![],
                dire: vec![],
                own_team: None,
                banned: vec![],
            },
        };
        out.refresh_snapshot();
        out
    }

    pub fn snapshot(&self) -> DraftSnapshot {
        self.snapshot.clone()
    }

    pub fn set_team(&mut self, team: Option<Team>) {
        if self.snapshot.own_team != team {
            self.snapshot.own_team = team;
            self.snapshot.revision += 1;
        }
    }

    pub fn set_bans(&mut self, mut heroes: Vec<u32>) -> Result<(), DraftError> {
        if heroes.contains(&0) {
            return Err(DraftError::InvalidHero);
        }
        if heroes
            .iter()
            .any(|h| self.snapshot.selected_heroes().contains(h))
        {
            return Err(DraftError::DuplicateHero);
        }
        heroes.sort_unstable();
        heroes.dedup();
        if heroes != self.snapshot.banned {
            self.snapshot.banned = heroes;
            self.snapshot.revision += 1;
        }
        Ok(())
    }

    /// Manual correction remains authoritative until explicitly released or reset.
    pub fn manual_set(
        &mut self,
        slot_id: u8,
        hero_id: Option<u32>,
        timestamp: u64,
    ) -> Result<Vec<DraftEvent>, DraftError> {
        if slot_id >= 10 {
            return Err(DraftError::InvalidSlot);
        }
        if hero_id == Some(0) {
            return Err(DraftError::InvalidHero);
        }
        if let Some(hero) = hero_id
            && (self.snapshot.banned.contains(&hero)
                || self.slots.iter().any(|s| {
                    s.state.slot_id != slot_id
                        && s.state.hero_id == Some(hero)
                        && s.state.recognition_state == RecognitionState::Confirmed
                }))
        {
            return Err(DraftError::DuplicateHero);
        }
        let before = self.signature();
        let old = self.snapshot.clone();
        self.slots[usize::from(slot_id)].state = SlotState {
            slot_id,
            hero_id,
            confidence: 1.0,
            recognition_state: if hero_id.is_some() {
                RecognitionState::Confirmed
            } else {
                RecognitionState::Empty
            },
            timestamp,
            source: LockEvidence::Manual,
        };
        Ok(self.finish_update(before, old))
    }

    pub fn release_manual(&mut self, slot_id: u8) -> Result<(), DraftError> {
        let m = self
            .slots
            .get_mut(usize::from(slot_id))
            .ok_or(DraftError::InvalidSlot)?;
        m.state.source = LockEvidence::None;
        m.state.recognition_state = RecognitionState::Uncertain;
        m.consecutive = 0;
        self.snapshot.revision += 1;
        self.refresh_snapshot();
        Ok(())
    }

    /// Complete frame transaction. Missing slots become uncertain, never silently stale.
    pub fn apply_frame(
        &mut self,
        timestamp: u64,
        draft_screen_detected: bool,
        observations: &[Observation],
    ) -> Result<Vec<DraftEvent>, DraftError> {
        if self.last_timestamp.is_some_and(|t| timestamp <= t) {
            return Err(DraftError::StaleFrame);
        }
        let mut seen = BTreeSet::new();
        for observation in observations {
            if observation.slot_id >= 10 {
                return Err(DraftError::InvalidSlot);
            }
            if !seen.insert(observation.slot_id) {
                return Err(DraftError::DuplicateSlot);
            }
        }
        self.last_timestamp = Some(timestamp);
        if draft_screen_detected {
            self.draft_seen = true;
            self.absent_frames = 0;
        } else if self.draft_seen {
            self.absent_frames = self.absent_frames.saturating_add(1);
            if self.absent_frames >= 3 {
                // Leaving a detected draft ends manual overrides too, preventing
                // a previous match from leaking into the next draft in the same window.
                let events = self.reset();
                self.last_timestamp = Some(timestamp);
                return Ok(events);
            }
        }
        let before = self.signature();
        let old = self.snapshot.clone();
        self.snapshot.draft_screen_detected = draft_screen_detected;
        for memory in &mut self.slots {
            if memory.state.source == LockEvidence::Manual {
                continue;
            }
            memory.state.timestamp = timestamp;
            let obs = observations
                .iter()
                .find(|o| o.slot_id == memory.state.slot_id);
            let Some(obs) = obs.filter(|o| draft_screen_detected && o.timestamp == timestamp)
            else {
                memory.state.recognition_state = RecognitionState::Uncertain;
                memory.consecutive = 0;
                continue;
            };
            memory.state.confidence = if obs.confidence.is_finite() {
                obs.confidence.clamp(0.0, 1.0)
            } else {
                0.0
            };
            memory.state.source = LockEvidence::None;
            if !obs.valid
                || !obs.confidence.is_finite()
                || !obs.margin.is_finite()
                || obs.hero_id == Some(0)
            {
                memory.state.recognition_state = RecognitionState::Invalid;
                memory.consecutive = 0;
                continue;
            }
            if obs.empty {
                memory.empty_frames = memory.empty_frames.saturating_add(1);
                memory.consecutive = 0;
                memory.state.hero_id = None;
                memory.state.recognition_state = if memory.empty_frames >= self.required_frames {
                    RecognitionState::Empty
                } else {
                    RecognitionState::Uncertain
                };
                continue;
            }
            memory.empty_frames = 0;
            memory.state.hero_id = obs.hero_id;
            let Some(hero) = obs.hero_id else {
                memory.state.recognition_state = RecognitionState::Uncertain;
                memory.consecutive = 0;
                continue;
            };
            if self.snapshot.banned.contains(&hero) {
                memory.state.recognition_state = RecognitionState::Invalid;
                memory.consecutive = 0;
                continue;
            }
            let reliable = obs.confidence >= 0.80 && obs.margin >= 0.08;
            // Only a distinct, verified visual marker contributes to a confirmation streak.
            if reliable && obs.lock_evidence == LockEvidence::VisualMarker {
                memory.consecutive = if memory.pending == Some(hero) {
                    memory.consecutive.saturating_add(1)
                } else {
                    1
                };
                memory.pending = Some(hero);
                memory.state.source = LockEvidence::VisualMarker;
                memory.state.recognition_state = if memory.consecutive >= self.required_frames {
                    RecognitionState::Confirmed
                } else {
                    RecognitionState::Candidate
                };
            } else {
                memory.consecutive = 0;
                memory.pending = Some(hero);
                memory.state.recognition_state = if reliable {
                    RecognitionState::Candidate
                } else {
                    RecognitionState::Uncertain
                };
            }
        }
        let mut occurrences: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
        for (i, m) in self.slots.iter().enumerate() {
            if let Some(hero) = m.state.hero_id
                && matches!(
                    m.state.recognition_state,
                    RecognitionState::Candidate | RecognitionState::Confirmed
                )
            {
                occurrences.entry(hero).or_default().push(i);
            }
        }
        for indices in occurrences.values().filter(|indices| indices.len() > 1) {
            // Do not pick a winner by confidence: both automatic readings are unsafe.
            for &i in indices {
                if self.slots[i].state.source != LockEvidence::Manual {
                    self.slots[i].state.recognition_state = RecognitionState::Invalid;
                    self.slots[i].consecutive = 0;
                }
            }
        }
        Ok(self.finish_update(before, old))
    }

    pub fn reset(&mut self) -> Vec<DraftEvent> {
        let revision = self.snapshot.revision + 1;
        let team = self.snapshot.own_team;
        *self = Self::new(self.required_frames);
        self.snapshot.revision = revision;
        self.snapshot.own_team = team;
        vec![DraftEvent::DraftUpdated { revision }]
    }

    pub fn window_lost(&mut self) -> Vec<DraftEvent> {
        let mut events = self.reset();
        events.push(DraftEvent::GameWindowLost);
        events
    }

    fn signature(&self) -> Vec<(Option<u32>, RecognitionState, LockEvidence)> {
        self.slots
            .iter()
            .map(|m| (m.state.hero_id, m.state.recognition_state, m.state.source))
            .collect()
    }

    fn refresh_snapshot(&mut self) {
        self.snapshot.radiant = self.slots[..5].iter().map(|m| m.state.clone()).collect();
        self.snapshot.dire = self.slots[5..].iter().map(|m| m.state.clone()).collect();
    }

    fn finish_update(
        &mut self,
        before: Vec<(Option<u32>, RecognitionState, LockEvidence)>,
        old: DraftSnapshot,
    ) -> Vec<DraftEvent> {
        let changed = before != self.signature()
            || old.draft_screen_detected != self.snapshot.draft_screen_detected;
        self.refresh_snapshot();
        if !changed {
            return vec![];
        }
        self.snapshot.revision += 1;
        let mut events = vec![];
        if !old.draft_screen_detected && self.snapshot.draft_screen_detected {
            events.push(DraftEvent::DraftScreenDetected);
        }
        for (previous, current) in old
            .radiant
            .iter()
            .chain(&old.dire)
            .zip(self.slots.iter().map(|m| &m.state))
        {
            if previous.recognition_state == RecognitionState::Confirmed
                && (current.recognition_state != RecognitionState::Confirmed
                    || current.hero_id != previous.hero_id)
                && let Some(hero_id) = previous.hero_id
            {
                events.push(DraftEvent::HeroRemoved {
                    slot_id: current.slot_id,
                    hero_id,
                });
            }
            if (previous.hero_id != current.hero_id
                || previous.recognition_state != current.recognition_state)
                && let Some(hero_id) = current.hero_id
            {
                match current.recognition_state {
                    RecognitionState::Confirmed => events.push(DraftEvent::HeroConfirmed {
                        slot_id: current.slot_id,
                        hero_id,
                    }),
                    RecognitionState::Candidate => events.push(DraftEvent::HeroCandidateDetected {
                        slot_id: current.slot_id,
                        hero_id,
                    }),
                    _ => (),
                }
            }
        }
        events.push(DraftEvent::DraftUpdated {
            revision: self.snapshot.revision,
        });
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(time: u64, lock: LockEvidence) -> Observation {
        Observation {
            slot_id: 0,
            hero_id: Some(13),
            confidence: 0.92,
            margin: 0.2,
            empty: false,
            valid: true,
            lock_evidence: lock,
            timestamp: time,
        }
    }

    #[test]
    fn stable_hover_never_confirms() {
        let mut tracker = DraftTracker::default();
        for t in 1..10 {
            tracker
                .apply_frame(t, true, &[observation(t, LockEvidence::None)])
                .unwrap();
        }
        assert!(tracker.snapshot().selected_heroes().is_empty());
        assert_eq!(
            tracker.snapshot().radiant[0].recognition_state,
            RecognitionState::Candidate
        );
    }

    #[test]
    fn confirmation_requires_distinct_frames_and_lock_marker() {
        let mut tracker = DraftTracker::default();
        tracker
            .apply_frame(1, true, &[observation(1, LockEvidence::VisualMarker)])
            .unwrap();
        assert!(matches!(
            tracker.apply_frame(1, true, &[observation(1, LockEvidence::VisualMarker)]),
            Err(DraftError::StaleFrame)
        ));
        for t in 2..=3 {
            tracker
                .apply_frame(t, true, &[observation(t, LockEvidence::VisualMarker)])
                .unwrap();
        }
        assert_eq!(tracker.snapshot().selected_heroes(), vec![13]);
        assert!(
            tracker
                .apply_frame(4, true, &[observation(4, LockEvidence::VisualMarker)])
                .unwrap()
                .is_empty()
        );
        tracker
            .apply_frame(5, false, &[observation(5, LockEvidence::VisualMarker)])
            .unwrap();
        assert!(tracker.snapshot().selected_heroes().is_empty());
    }

    #[test]
    fn duplicate_and_missing_slots_cannot_enter_recommendations() {
        let mut tracker = DraftTracker::new(2);
        for t in 1..=3 {
            let first = observation(t, LockEvidence::VisualMarker);
            let mut second = first.clone();
            second.slot_id = 6;
            tracker.apply_frame(t, true, &[first, second]).unwrap();
        }
        assert!(tracker.snapshot().selected_heroes().is_empty());
        assert_eq!(
            tracker.snapshot().dire[1].recognition_state,
            RecognitionState::Invalid
        );
    }

    #[test]
    fn correction_persists_but_next_match_resets() {
        let mut tracker = DraftTracker::default();
        tracker.manual_set(0, Some(13), 1).unwrap();
        tracker.apply_frame(2, false, &[]).unwrap();
        assert_eq!(tracker.snapshot().selected_heroes(), vec![13]);
        assert!(tracker.manual_set(5, Some(13), 3).is_err());
        assert!(tracker.set_bans(vec![13]).is_err());
        tracker.window_lost();
        assert!(tracker.snapshot().selected_heroes().is_empty());
    }

    #[test]
    fn malformed_frame_does_not_partially_mutate() {
        let mut tracker = DraftTracker::default();
        let obs = observation(1, LockEvidence::VisualMarker);
        assert!(tracker.apply_frame(1, true, &[obs.clone(), obs]).is_err());
        assert_eq!(tracker.snapshot().revision, 0);
    }

    #[test]
    fn sustained_exit_resets_manual_picks_only_after_draft_was_detected() {
        let mut tracker = DraftTracker::default();
        tracker.manual_set(0, Some(13), 0).unwrap();
        for t in 1..=4 {
            tracker.apply_frame(t, false, &[]).unwrap();
        }
        assert_eq!(tracker.snapshot().selected_heroes(), vec![13]);
        tracker.apply_frame(5, true, &[]).unwrap();
        for t in 6..=7 {
            tracker.apply_frame(t, false, &[]).unwrap();
        }
        assert_eq!(tracker.snapshot().selected_heroes(), vec![13]);
        tracker.apply_frame(8, false, &[]).unwrap();
        assert!(tracker.snapshot().selected_heroes().is_empty());
        assert!(matches!(
            tracker.apply_frame(8, false, &[]),
            Err(DraftError::StaleFrame)
        ));
    }
}
