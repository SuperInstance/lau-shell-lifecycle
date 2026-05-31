//! Shell lifecycle manager — from spawn to death, with self-assembling DNA.
//!
//! This crate provides a full lifecycle state machine for shell instances,
//! including self-assembling DNA pathways that grow with use and decay with disuse.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// ShellType
// ---------------------------------------------------------------------------

/// The type of shell being managed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum ShellType {
    Hermes,
    ZeroClaw,
    CUDAClaw,
    GitNative,
    Remote { address: String },
    Custom(String),
}

// ---------------------------------------------------------------------------
// ShellConfig
// ---------------------------------------------------------------------------

/// Configuration for spawning a new shell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShellConfig {
    pub id: String,
    pub name: String,
    pub shell_type: ShellType,
    pub universe_path: String,
    pub conservation_budget: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub max_children: usize,
    pub capabilities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl ShellConfig {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("ShellConfig serialization should not fail")
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

// ---------------------------------------------------------------------------
// ShellState
// ---------------------------------------------------------------------------

/// Lifecycle states a shell can be in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ShellState {
    Conceived,
    Spawning,
    Bootstrapping,
    Running,
    Suspended,
    Migrating,
    Dying,
    Dead,
}

impl ShellState {
    /// Returns the set of states that are legal targets from this state.
    fn legal_transitions(&self) -> &'static [ShellState] {
        match self {
            ShellState::Conceived => &[ShellState::Spawning],
            ShellState::Spawning => &[ShellState::Bootstrapping],
            ShellState::Bootstrapping => &[ShellState::Running],
            ShellState::Running => &[
                ShellState::Suspended,
                ShellState::Migrating,
                ShellState::Dying,
            ],
            ShellState::Suspended => &[ShellState::Running],
            ShellState::Migrating => &[ShellState::Running],
            ShellState::Dying => &[ShellState::Dead],
            ShellState::Dead => &[],
        }
    }
}

// ---------------------------------------------------------------------------
// LifecycleEvent
// ---------------------------------------------------------------------------

/// Events that can trigger lifecycle transitions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "payload")]
pub enum LifecycleEvent {
    Spawn {
        parent: String,
        config: ShellConfig,
    },
    Bootstrap,
    Ready,
    Suspend {
        reason: String,
    },
    Resume,
    Migrate {
        target: String,
    },
    Kill {
        reason: String,
    },
    HeartbeatReceived,
    Timeout,
    Error {
        message: String,
    },
}

// ---------------------------------------------------------------------------
// LifecycleError
// ---------------------------------------------------------------------------

/// Errors that can occur during lifecycle operations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LifecycleError {
    InvalidTransition {
        from: ShellState,
        to: ShellState,
    },
    ShellNotFound(String),
    AlreadyExists(String),
    MaxChildrenReached(usize),
    ParentNotRunning(String),
}

impl std::fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LifecycleError::InvalidTransition { from, to } => {
                write!(f, "invalid transition from {:?} to {:?}", from, to)
            }
            LifecycleError::ShellNotFound(id) => write!(f, "shell not found: {}", id),
            LifecycleError::AlreadyExists(id) => write!(f, "shell already exists: {}", id),
            LifecycleError::MaxChildrenReached(max) => {
                write!(f, "max children reached: {}", max)
            }
            LifecycleError::ParentNotRunning(id) => {
                write!(f, "parent not running: {}", id)
            }
        }
    }
}

impl std::error::Error for LifecycleError {}

// ---------------------------------------------------------------------------
// ShellLifecycle
// ---------------------------------------------------------------------------

/// The lifecycle state machine for a single shell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShellLifecycle {
    pub state: ShellState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kill_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suspend_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub migration_target: Option<String>,
}

impl ShellLifecycle {
    pub fn new() -> Self {
        Self {
            state: ShellState::Conceived,
            kill_reason: None,
            suspend_reason: None,
            migration_target: None,
        }
    }

    pub fn can_transition_to(&self, target: &ShellState) -> bool {
        self.state
            .legal_transitions()
            .contains(target)
    }

    /// Attempt a state transition based on the given event.
    pub fn transition(&mut self, event: LifecycleEvent) -> Result<(), LifecycleError> {
        let target = Self::target_state_for_event(&event);

        // Special case: Kill can be applied from several states, not just Running.
        // Timeout/Error map to Dead and can transition from Dying.
        let is_kill_to_dying = matches!(&event, LifecycleEvent::Kill { .. }) && target == ShellState::Dying;
        let is_terminal = matches!(&event, LifecycleEvent::Timeout | LifecycleEvent::Error { .. })
            && self.state == ShellState::Dying;

        if is_kill_to_dying {
            // Allow Kill from: Running, Suspended, Spawning, Bootstrapping, Migrating
            match self.state {
                ShellState::Running
                | ShellState::Suspended
                | ShellState::Spawning
                | ShellState::Bootstrapping
                | ShellState::Migrating => {}
                _ => {
                    return Err(LifecycleError::InvalidTransition {
                        from: self.state,
                        to: target,
                    });
                }
            }
        } else if is_terminal {
            // Allow Dying → Dead
        } else if !self.can_transition_to(&target) {
            return Err(LifecycleError::InvalidTransition {
                from: self.state,
                to: target,
            });
        }

        // Apply side-effects
        match event {
            LifecycleEvent::Suspend { reason } => self.suspend_reason = Some(reason),
            LifecycleEvent::Kill { reason } => self.kill_reason = Some(reason),
            LifecycleEvent::Migrate { target: t } => self.migration_target = Some(t),
            _ => {}
        }

        self.state = target;
        Ok(())
    }

    fn target_state_for_event(event: &LifecycleEvent) -> ShellState {
        match event {
            LifecycleEvent::Spawn { .. } => ShellState::Spawning,
            LifecycleEvent::Bootstrap => ShellState::Bootstrapping,
            LifecycleEvent::Ready => ShellState::Running,
            LifecycleEvent::Suspend { .. } => ShellState::Suspended,
            LifecycleEvent::Resume => ShellState::Running,
            LifecycleEvent::Migrate { .. } => ShellState::Migrating,
            LifecycleEvent::Kill { .. } => ShellState::Dying,
            LifecycleEvent::HeartbeatReceived => ShellState::Running,
            LifecycleEvent::Timeout => ShellState::Dead,
            LifecycleEvent::Error { .. } => ShellState::Dead,
        }
    }
}

impl Default for ShellLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Pathway
// ---------------------------------------------------------------------------

/// A single DNA pathway — grows with use, decays with disuse.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pathway {
    pub name: String,
    pub use_count: u64,
    pub strength: f64,
    pub last_used: u64,
    pub decay_rate: f64,
    pub growth_rate: f64,
    pub category: String,
}

impl Pathway {
    pub fn new(name: &str, category: &str) -> Self {
        Self {
            name: name.to_string(),
            use_count: 0,
            strength: 0.0,
            last_used: 0,
            decay_rate: 0.01,
            growth_rate: 0.1,
            category: category.to_string(),
        }
    }

    /// Record a single use — asymptotic growth toward 1.0.
    pub fn use_once(&mut self) {
        let growth = self.growth_rate * (1.0 - self.strength);
        if growth > 0.0 {
            self.strength += growth;
            if self.strength > 1.0 {
                self.strength = 1.0;
            }
        }
        self.use_count += 1;
    }

    /// Apply one tick of linear decay.
    pub fn decay(&mut self) {
        self.strength -= self.decay_rate;
        if self.strength < 0.0 {
            self.strength = 0.0;
        }
    }
}

// ---------------------------------------------------------------------------
// DNA
// ---------------------------------------------------------------------------

/// Self-assembling pathway collection — used pathways grow, unused get pruned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DNA {
    pub pathways: HashMap<String, Pathway>,
    pub tick_count: u64,
    pub prune_threshold: f64,
}

impl DNA {
    pub fn new() -> Self {
        Self {
            pathways: HashMap::new(),
            tick_count: 0,
            prune_threshold: 0.01,
        }
    }

    /// Record use of a pathway, creating it if it doesn't exist.
    pub fn record_use(&mut self, pathway: &str) {
        let category = infer_category(pathway);
        let p = self
            .pathways
            .entry(pathway.to_string())
            .or_insert_with(|| Pathway::new(pathway, &category));
        p.use_once();
        p.last_used = self.tick_count;
    }

    /// Tick all pathways — decay and prune.
    pub fn tick(&mut self) {
        self.tick_count += 1;
        for p in self.pathways.values_mut() {
            p.decay();
        }
        self.pathways
            .retain(|_, p| p.strength >= self.prune_threshold);
    }

    /// Get the top N strongest pathways.
    pub fn strongest(&self, n: usize) -> Vec<(&String, &Pathway)> {
        let mut v: Vec<_> = self.pathways.iter().collect();
        v.sort_by(|a, b| b.1.strength.partial_cmp(&a.1.strength).unwrap_or(std::cmp::Ordering::Equal));
        v.truncate(n);
        v
    }

    /// Sum of all pathway strengths.
    pub fn total_strength(&self) -> f64 {
        self.pathways.values().map(|p| p.strength).sum()
    }

    /// Shannon entropy of the pathway strength distribution.
    pub fn diversity(&self) -> f64 {
        if self.pathways.is_empty() {
            return 0.0;
        }
        let total = self.total_strength();
        if total <= 0.0 {
            return 0.0;
        }
        self.pathways
            .values()
            .map(|p| {
                let prob = p.strength / total;
                if prob > 0.0 {
                    -prob * prob.log2()
                } else {
                    0.0
                }
            })
            .sum()
    }
}

impl Default for DNA {
    fn default() -> Self {
        Self::new()
    }
}

fn infer_category(pathway: &str) -> String {
    let low = pathway.to_lowercase();
    if low.contains("rout") || low.contains("dispatch") {
        "routing".to_string()
    } else if low.contains("provider") || low.contains("model") {
        "provider".to_string()
    } else if low.contains("room") || low.contains("channel") {
        "room".to_string()
    } else if low.contains("tile") || low.contains("block") {
        "tile".to_string()
    } else if low.contains("circuit") || low.contains("wire") {
        "circuit".to_string()
    } else {
        "general".to_string()
    }
}

// ---------------------------------------------------------------------------
// ShellProfile
// ---------------------------------------------------------------------------

/// Accumulated identity and stats for a shell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShellProfile {
    pub shell_id: String,
    pub lifecycle: ShellLifecycle,
    pub dna: DNA,
    pub born_at: u64,
    pub total_ticks: u64,
    pub total_energy_used: f64,
    pub children_spawned: u64,
    pub messages_sent: u64,
    pub messages_received: u64,
    pub config: ShellConfig,
}

impl ShellProfile {
    pub fn new(config: ShellConfig, now: u64) -> Self {
        let id = config.id.clone();
        Self {
            shell_id: id,
            lifecycle: ShellLifecycle::new(),
            dna: DNA::new(),
            born_at: now,
            total_ticks: 0,
            total_energy_used: 0.0,
            children_spawned: 0,
            messages_sent: 0,
            messages_received: 0,
            config,
        }
    }

    pub fn pathway_count(&self) -> usize {
        self.dna.pathways.len()
    }

    pub fn age_seconds(&self, now: u64) -> u64 {
        now.saturating_sub(self.born_at)
    }

    pub fn efficiency(&self) -> f64 {
        if self.total_energy_used <= 0.0 {
            return 0.0;
        }
        (self.messages_sent + self.messages_received) as f64 / self.total_energy_used
    }

    /// Adaptation score: DNA diversity × total strength.
    pub fn adaptation_score(&self) -> f64 {
        self.dna.diversity() * self.dna.total_strength()
    }
}

// ---------------------------------------------------------------------------
// ShellNursery
// ---------------------------------------------------------------------------

/// Manages all shell profiles.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ShellNursery {
    pub shells: HashMap<String, ShellProfile>,
}

impl ShellNursery {
    pub fn new() -> Self {
        Self {
            shells: HashMap::new(),
        }
    }

    /// Spawn a new shell from the given config.
    pub fn spawn(&mut self, config: ShellConfig) -> Result<&mut ShellProfile, LifecycleError> {
        if self.shells.contains_key(&config.id) {
            return Err(LifecycleError::AlreadyExists(config.id.clone()));
        }

        // Check parent constraints if there is a parent
        if let Some(ref parent_id) = config.parent_id {
            let parent = self
                .shells
                .get(parent_id)
                .ok_or_else(|| LifecycleError::ParentNotRunning(parent_id.clone()))?;

            if parent.lifecycle.state != ShellState::Running {
                return Err(LifecycleError::ParentNotRunning(parent_id.clone()));
            }

            if parent.children_spawned >= parent.config.max_children as u64 {
                return Err(LifecycleError::MaxChildrenReached(
                    parent.config.max_children,
                ));
            }
        }

        let id = config.id.clone();
        let now = 0u64; // Nursery uses tick-based time
        let mut profile = ShellProfile::new(config, now);
        profile
            .lifecycle
            .transition(LifecycleEvent::Spawn {
                parent: profile.config.parent_id.clone().unwrap_or_default(),
                config: profile.config.clone(),
            })
            .map_err(|_e| LifecycleError::InvalidTransition {
                from: ShellState::Conceived,
                to: ShellState::Spawning,
            })?;

        // Increment parent's children count
        if let Some(ref parent_id) = profile.config.parent_id {
            if let Some(parent) = self.shells.get_mut(parent_id) {
                parent.children_spawned += 1;
            }
        }

        self.shells.insert(id.clone(), profile);
        Ok(self.shells.get_mut(&id).unwrap())
    }

    /// Kill a shell by id.
    pub fn kill(&mut self, id: &str, reason: &str) -> Result<ShellProfile, LifecycleError> {
        let profile = self
            .shells
            .get_mut(id)
            .ok_or_else(|| LifecycleError::ShellNotFound(id.to_string()))?;

        profile
            .lifecycle
            .transition(LifecycleEvent::Kill {
                reason: reason.to_string(),
            })
            .map_err(|_| LifecycleError::InvalidTransition {
                from: profile.lifecycle.state,
                to: ShellState::Dying,
            })?;

        // Now move to Dead
        profile
            .lifecycle
            .transition(LifecycleEvent::Timeout)
            .map_err(|_| LifecycleError::InvalidTransition {
                from: profile.lifecycle.state,
                to: ShellState::Dead,
            })?;

        Ok(self.shells.remove(id).unwrap())
    }

    /// Get a shell profile by id.
    pub fn get(&self, id: &str) -> Option<&ShellProfile> {
        self.shells.get(id)
    }

    /// Get a mutable reference to a shell profile by id.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut ShellProfile> {
        self.shells.get_mut(id)
    }

    /// Tick DNA for all running shells.
    pub fn tick_all(&mut self) {
        for profile in self.shells.values_mut() {
            if profile.lifecycle.state == ShellState::Running {
                profile.dna.tick();
                profile.total_ticks += 1;
            }
        }
    }

    /// Get all running shell profiles.
    pub fn running(&self) -> Vec<&ShellProfile> {
        self.shells
            .values()
            .filter(|p| p.lifecycle.state == ShellState::Running)
            .collect()
    }

    /// Get all children of a given parent.
    pub fn children_of(&self, parent_id: &str) -> Vec<&ShellProfile> {
        self.shells
            .values()
            .filter(|p| p.config.parent_id.as_deref() == Some(parent_id))
            .collect()
    }

    /// Trace ancestry from a shell to the root.
    pub fn lineage(&self, id: &str) -> Vec<String> {
        let mut lineage = Vec::new();
        let mut current_id = id.to_string();
        while let Some(profile) = self.shells.get(&current_id) {
            lineage.push(current_id.clone());
            match profile.config.parent_id {
                Some(ref pid) => current_id = pid.clone(),
                None => break,
            }
        }
        lineage
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn config(id: &str) -> ShellConfig {
        ShellConfig {
            id: id.to_string(),
            name: format!("shell-{}", id),
            shell_type: ShellType::Hermes,
            universe_path: "/universe".to_string(),
            conservation_budget: 100.0,
            parent_id: None,
            max_children: 10,
            capabilities: vec!["read".to_string(), "write".to_string()],
            model: None,
        }
    }

    fn config_with_parent(id: &str, parent_id: &str) -> ShellConfig {
        let mut c = config(id);
        c.parent_id = Some(parent_id.to_string());
        c
    }

    // --- ShellState tests ---

    #[test]
    fn test_state_legal_transitions_conceived() {
        let s = ShellState::Conceived;
        assert!(s.legal_transitions().contains(&ShellState::Spawning));
        assert!(!s.legal_transitions().contains(&ShellState::Running));
    }

    #[test]
    fn test_state_legal_transitions_running() {
        let s = ShellState::Running;
        assert!(s.legal_transitions().contains(&ShellState::Suspended));
        assert!(s.legal_transitions().contains(&ShellState::Migrating));
        assert!(s.legal_transitions().contains(&ShellState::Dying));
        assert!(!s.legal_transitions().contains(&ShellState::Conceived));
    }

    #[test]
    fn test_state_legal_transitions_dead() {
        let s = ShellState::Dead;
        assert!(s.legal_transitions().is_empty());
    }

    #[test]
    fn test_state_legal_transitions_suspended() {
        let s = ShellState::Suspended;
        assert!(s.legal_transitions().contains(&ShellState::Running));
        assert_eq!(s.legal_transitions().len(), 1);
    }

    #[test]
    fn test_state_legal_transitions_migrating() {
        let s = ShellState::Migrating;
        assert!(s.legal_transitions().contains(&ShellState::Running));
        assert_eq!(s.legal_transitions().len(), 1);
    }

    #[test]
    fn test_state_legal_transitions_spawning() {
        let s = ShellState::Spawning;
        assert!(s.legal_transitions().contains(&ShellState::Bootstrapping));
        assert_eq!(s.legal_transitions().len(), 1);
    }

    #[test]
    fn test_state_legal_transitions_bootstrapping() {
        let s = ShellState::Bootstrapping;
        assert!(s.legal_transitions().contains(&ShellState::Running));
        assert_eq!(s.legal_transitions().len(), 1);
    }

    #[test]
    fn test_state_legal_transitions_dying() {
        let s = ShellState::Dying;
        assert!(s.legal_transitions().contains(&ShellState::Dead));
        assert_eq!(s.legal_transitions().len(), 1);
    }

    // --- ShellLifecycle tests ---

    #[test]
    fn test_lifecycle_new() {
        let lc = ShellLifecycle::new();
        assert_eq!(lc.state, ShellState::Conceived);
    }

    #[test]
    fn test_lifecycle_can_transition_to() {
        let lc = ShellLifecycle::new();
        assert!(lc.can_transition_to(&ShellState::Spawning));
        assert!(!lc.can_transition_to(&ShellState::Running));
    }

    #[test]
    fn test_lifecycle_full_happy_path() {
        let mut lc = ShellLifecycle::new();
        lc.transition(LifecycleEvent::Spawn {
            parent: "root".to_string(),
            config: config("test"),
        })
        .unwrap();
        assert_eq!(lc.state, ShellState::Spawning);

        lc.transition(LifecycleEvent::Bootstrap).unwrap();
        assert_eq!(lc.state, ShellState::Bootstrapping);

        lc.transition(LifecycleEvent::Ready).unwrap();
        assert_eq!(lc.state, ShellState::Running);
    }

    #[test]
    fn test_lifecycle_invalid_transition() {
        let mut lc = ShellLifecycle::new();
        let result = lc.transition(LifecycleEvent::Ready);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            LifecycleError::InvalidTransition {
                from: ShellState::Conceived,
                to: ShellState::Running
            }
        );
    }

    #[test]
    fn test_lifecycle_suspend_resume() {
        let mut lc = ShellLifecycle::new();
        lc.transition(LifecycleEvent::Spawn {
            parent: "root".into(),
            config: config("x"),
        })
        .unwrap();
        lc.transition(LifecycleEvent::Bootstrap).unwrap();
        lc.transition(LifecycleEvent::Ready).unwrap();

        lc.transition(LifecycleEvent::Suspend {
            reason: "maintenance".into(),
        })
        .unwrap();
        assert_eq!(lc.state, ShellState::Suspended);
        assert_eq!(lc.suspend_reason.as_deref(), Some("maintenance"));

        lc.transition(LifecycleEvent::Resume).unwrap();
        assert_eq!(lc.state, ShellState::Running);
    }

    #[test]
    fn test_lifecycle_migrate() {
        let mut lc = ShellLifecycle::new();
        lc.transition(LifecycleEvent::Spawn {
            parent: "root".into(),
            config: config("x"),
        })
        .unwrap();
        lc.transition(LifecycleEvent::Bootstrap).unwrap();
        lc.transition(LifecycleEvent::Ready).unwrap();

        lc.transition(LifecycleEvent::Migrate {
            target: "gpu-01".into(),
        })
        .unwrap();
        assert_eq!(lc.state, ShellState::Migrating);
        assert_eq!(lc.migration_target.as_deref(), Some("gpu-01"));

        lc.transition(LifecycleEvent::Ready).unwrap();
        assert_eq!(lc.state, ShellState::Running);
    }

    #[test]
    fn test_lifecycle_kill() {
        let mut lc = ShellLifecycle::new();
        lc.transition(LifecycleEvent::Spawn {
            parent: "root".into(),
            config: config("x"),
        })
        .unwrap();
        lc.transition(LifecycleEvent::Bootstrap).unwrap();
        lc.transition(LifecycleEvent::Ready).unwrap();

        lc.transition(LifecycleEvent::Kill {
            reason: "evicted".into(),
        })
        .unwrap();
        assert_eq!(lc.state, ShellState::Dying);
        assert_eq!(lc.kill_reason.as_deref(), Some("evicted"));

        // Can't go anywhere but Dead from Dying
        let err = lc.transition(LifecycleEvent::Resume);
        assert!(err.is_err());

        // Dying → Dead via Timeout
        lc.transition(LifecycleEvent::Timeout).unwrap();
        assert_eq!(lc.state, ShellState::Dead);
    }

    #[test]
    fn test_lifecycle_cannot_transition_from_dead() {
        let mut lc = ShellLifecycle::new();
        lc.transition(LifecycleEvent::Spawn {
            parent: "root".into(),
            config: config("x"),
        })
        .unwrap();
        lc.transition(LifecycleEvent::Bootstrap).unwrap();
        lc.transition(LifecycleEvent::Ready).unwrap();
        // Kill from Running → Dying
        lc.transition(LifecycleEvent::Kill {
            reason: "done".into(),
        })
        .unwrap();
        // Dying → Dead
        lc.transition(LifecycleEvent::Timeout).unwrap();
        assert_eq!(lc.state, ShellState::Dead);

        assert!(lc
            .transition(LifecycleEvent::Spawn {
                parent: "root".into(),
                config: config("y"),
            })
            .is_err());
    }

    // --- ShellConfig tests ---

    #[test]
    fn test_config_json_roundtrip() {
        let c = config("abc");
        let json = c.to_json();
        let c2 = ShellConfig::from_json(&json).unwrap();
        assert_eq!(c, c2);
    }

    #[test]
    fn test_config_json_invalid() {
        assert!(ShellConfig::from_json("not json").is_err());
    }

    #[test]
    fn test_shell_type_serialization() {
        let types = vec![
            ShellType::Hermes,
            ShellType::ZeroClaw,
            ShellType::CUDAClaw,
            ShellType::GitNative,
            ShellType::Remote {
                address: "1.2.3.4".into(),
            },
            ShellType::Custom("special".into()),
        ];
        for t in types {
            let json = serde_json::to_string(&t).unwrap();
            let t2: ShellType = serde_json::from_str(&json).unwrap();
            assert_eq!(t, t2);
        }
    }

    // --- Pathway tests ---

    #[test]
    fn test_pathway_new() {
        let p = Pathway::new("test-path", "routing");
        assert_eq!(p.name, "test-path");
        assert_eq!(p.use_count, 0);
        assert_eq!(p.strength, 0.0);
        assert_eq!(p.category, "routing");
    }

    #[test]
    fn test_pathway_use_once() {
        let mut p = Pathway::new("p", "general");
        p.use_once();
        assert_eq!(p.use_count, 1);
        assert!(p.strength > 0.0);
        // First use: 0.0 + 0.1 * (1.0 - 0.0) = 0.1
        assert!((p.strength - 0.1).abs() < 1e-10);
    }

    #[test]
    fn test_pathway_asymptotic_growth() {
        let mut p = Pathway::new("p", "general");
        for _ in 0..1000 {
            let prev = p.strength;
            p.use_once();
            assert!(p.strength >= prev - 1e-15);
            assert!(p.strength <= 1.0);
        }
        // Should be very close to 1.0
        assert!(p.strength > 0.9999);
    }

    #[test]
    fn test_pathway_decay() {
        let mut p = Pathway::new("p", "general");
        p.strength = 0.5;
        p.decay();
        assert!((p.strength - 0.49).abs() < 1e-10);
    }

    #[test]
    fn test_pathway_decay_floor() {
        let mut p = Pathway::new("p", "general");
        p.strength = 0.005;
        p.decay();
        assert_eq!(p.strength, 0.0);
    }

    // --- DNA tests ---

    #[test]
    fn test_dna_new() {
        let dna = DNA::new();
        assert!(dna.pathways.is_empty());
        assert_eq!(dna.tick_count, 0);
    }

    #[test]
    fn test_dna_record_use_creates_pathway() {
        let mut dna = DNA::new();
        dna.record_use("routing/main");
        assert_eq!(dna.pathways.len(), 1);
        assert_eq!(dna.pathways["routing/main"].use_count, 1);
    }

    #[test]
    fn test_dna_record_use_increments() {
        let mut dna = DNA::new();
        dna.record_use("routing/main");
        dna.record_use("routing/main");
        dna.record_use("routing/main");
        assert_eq!(dna.pathways["routing/main"].use_count, 3);
    }

    #[test]
    fn test_dna_tick_decay_and_prune() {
        let mut dna = DNA::new();
        dna.record_use("weak");
        // After one use: strength ≈ 0.1
        // One tick: 0.1 - 0.01 = 0.09
        // Many ticks should prune it
        for _ in 0..20 {
            dna.tick();
        }
        assert!(!dna.pathways.contains_key("weak"));
    }

    #[test]
    fn test_dna_tick_preserves_strong() {
        let mut dna = DNA::new();
        // Use a pathway many times
        for _ in 0..50 {
            dna.record_use("strong");
        }
        // A few ticks shouldn't prune it
        for _ in 0..5 {
            dna.tick();
        }
        assert!(dna.pathways.contains_key("strong"));
    }

    #[test]
    fn test_dna_tick_count_increments() {
        let mut dna = DNA::new();
        dna.tick();
        dna.tick();
        dna.tick();
        assert_eq!(dna.tick_count, 3);
    }

    #[test]
    fn test_dna_strongest() {
        let mut dna = DNA::new();
        dna.record_use("b");
        dna.record_use("a");
        dna.record_use("a");
        let top = dna.strongest(2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].0, "a");
        assert_eq!(top[1].0, "b");
    }

    #[test]
    fn test_dna_strongest_limited() {
        let mut dna = DNA::new();
        dna.record_use("x");
        dna.record_use("y");
        dna.record_use("z");
        let top = dna.strongest(1);
        assert_eq!(top.len(), 1);
    }

    #[test]
    fn test_dna_total_strength() {
        let mut dna = DNA::new();
        dna.record_use("a");
        dna.record_use("b");
        dna.record_use("b");
        let total = dna.total_strength();
        // a: 0.1, b: 0.1 + 0.1*(1-0.1) = 0.1 + 0.09 = 0.19
        assert!((total - 0.29).abs() < 1e-10);
    }

    #[test]
    fn test_dna_diversity_empty() {
        let dna = DNA::new();
        assert_eq!(dna.diversity(), 0.0);
    }

    #[test]
    fn test_dna_diversity_single() {
        let mut dna = DNA::new();
        dna.record_use("only");
        // Single pathway: probability = 1.0, entropy = -1.0 * log2(1.0) = 0.0
        assert_eq!(dna.diversity(), 0.0);
    }

    #[test]
    fn test_dna_diversity_balanced() {
        let mut dna = DNA::new();
        dna.record_use("a");
        dna.record_use("b");
        // Both have same strength → maximum entropy for 2 items = 1.0
        let d = dna.diversity();
        assert!((d - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_dna_category_inference_routing() {
        let mut dna = DNA::new();
        dna.record_use("route-dispatch");
        assert_eq!(dna.pathways["route-dispatch"].category, "routing");
    }

    #[test]
    fn test_dna_category_inference_provider() {
        let mut dna = DNA::new();
        dna.record_use("model-provider");
        assert_eq!(dna.pathways["model-provider"].category, "provider");
    }

    #[test]
    fn test_dna_category_inference_room() {
        let mut dna = DNA::new();
        dna.record_use("room-lobby");
        assert_eq!(dna.pathways["room-lobby"].category, "room");
    }

    #[test]
    fn test_dna_category_inference_tile() {
        let mut dna = DNA::new();
        dna.record_use("tile-render");
        assert_eq!(dna.pathways["tile-render"].category, "tile");
    }

    #[test]
    fn test_dna_category_inference_circuit() {
        let mut dna = DNA::new();
        dna.record_use("circuit-board");
        assert_eq!(dna.pathways["circuit-board"].category, "circuit");
    }

    #[test]
    fn test_dna_category_inference_general() {
        let mut dna = DNA::new();
        dna.record_use("something-random");
        assert_eq!(dna.pathways["something-random"].category, "general");
    }

    // --- ShellProfile tests ---

    #[test]
    fn test_profile_new() {
        let c = config("p1");
        let p = ShellProfile::new(c, 100);
        assert_eq!(p.shell_id, "p1");
        assert_eq!(p.born_at, 100);
        assert_eq!(p.total_ticks, 0);
    }

    #[test]
    fn test_profile_pathway_count() {
        let c = config("p1");
        let mut p = ShellProfile::new(c, 0);
        assert_eq!(p.pathway_count(), 0);
        p.dna.record_use("a");
        assert_eq!(p.pathway_count(), 1);
        p.dna.record_use("b");
        assert_eq!(p.pathway_count(), 2);
    }

    #[test]
    fn test_profile_age_seconds() {
        let c = config("p1");
        let p = ShellProfile::new(c, 100);
        assert_eq!(p.age_seconds(200), 100);
        assert_eq!(p.age_seconds(50), 0); // saturating_sub
    }

    #[test]
    fn test_profile_efficiency_zero_energy() {
        let c = config("p1");
        let p = ShellProfile::new(c, 0);
        assert_eq!(p.efficiency(), 0.0);
    }

    #[test]
    fn test_profile_efficiency() {
        let c = config("p1");
        let mut p = ShellProfile::new(c, 0);
        p.messages_sent = 10;
        p.messages_received = 20;
        p.total_energy_used = 5.0;
        assert!((p.efficiency() - 6.0).abs() < 1e-10);
    }

    #[test]
    fn test_profile_adaptation_score() {
        let c = config("p1");
        let mut p = ShellProfile::new(c, 0);
        p.dna.record_use("a");
        p.dna.record_use("b");
        let score = p.adaptation_score();
        assert!(score > 0.0);
    }

    // --- ShellNursery tests ---

    #[test]
    fn test_nursery_new() {
        let n = ShellNursery::new();
        assert!(n.shells.is_empty());
    }

    #[test]
    fn test_nursery_spawn() {
        let mut n = ShellNursery::new();
        let profile = n.spawn(config("s1")).unwrap();
        assert_eq!(profile.shell_id, "s1");
        assert_eq!(profile.lifecycle.state, ShellState::Spawning);
    }

    #[test]
    fn test_nursery_spawn_duplicate() {
        let mut n = ShellNursery::new();
        n.spawn(config("s1")).unwrap();
        let err = n.spawn(config("s1")).unwrap_err();
        assert_eq!(err, LifecycleError::AlreadyExists("s1".into()));
    }

    #[test]
    fn test_nursery_spawn_with_parent() {
        let mut n = ShellNursery::new();
        let parent = n.spawn(config("parent")).unwrap();
        // Advance parent to Running
        parent.lifecycle.transition(LifecycleEvent::Bootstrap).unwrap();
        parent.lifecycle.transition(LifecycleEvent::Ready).unwrap();

        let child = n.spawn(config_with_parent("child", "parent")).unwrap();
        assert_eq!(child.config.parent_id, Some("parent".into()));
        assert_eq!(n.get("parent").unwrap().children_spawned, 1);
    }

    #[test]
    fn test_nursery_spawn_parent_not_found() {
        let mut n = ShellNursery::new();
        let err = n
            .spawn(config_with_parent("child", "nonexistent"))
            .unwrap_err();
        assert_eq!(err, LifecycleError::ParentNotRunning("nonexistent".into()));
    }

    #[test]
    fn test_nursery_spawn_parent_not_running() {
        let mut n = ShellNursery::new();
        n.spawn(config("parent")).unwrap();
        // Parent is still in Spawning state
        let err = n
            .spawn(config_with_parent("child", "parent"))
            .unwrap_err();
        assert_eq!(err, LifecycleError::ParentNotRunning("parent".into()));
    }

    #[test]
    fn test_nursery_spawn_max_children() {
        let mut n = ShellNursery::new();
        let mut small_config = config("parent");
        small_config.max_children = 1;
        let parent = n.spawn(small_config).unwrap();
        parent.lifecycle.transition(LifecycleEvent::Bootstrap).unwrap();
        parent.lifecycle.transition(LifecycleEvent::Ready).unwrap();

        n.spawn(config_with_parent("child1", "parent")).unwrap();
        let err = n
            .spawn(config_with_parent("child2", "parent"))
            .unwrap_err();
        assert_eq!(err, LifecycleError::MaxChildrenReached(1));
    }

    #[test]
    fn test_nursery_kill() {
        let mut n = ShellNursery::new();
        let s1 = n.spawn(config("s1")).unwrap();
        s1.lifecycle.transition(LifecycleEvent::Bootstrap).unwrap();
        s1.lifecycle.transition(LifecycleEvent::Ready).unwrap();
        let killed = n.kill("s1", "test").unwrap();
        assert_eq!(killed.lifecycle.state, ShellState::Dead);
        assert!(n.get("s1").is_none());
    }

    #[test]
    fn test_nursery_kill_not_found() {
        let mut n = ShellNursery::new();
        let err = n.kill("ghost", "test").unwrap_err();
        assert_eq!(err, LifecycleError::ShellNotFound("ghost".into()));
    }

    #[test]
    fn test_nursery_get() {
        let mut n = ShellNursery::new();
        n.spawn(config("s1")).unwrap();
        assert!(n.get("s1").is_some());
        assert!(n.get("s2").is_none());
    }

    #[test]
    fn test_nursery_tick_all() {
        let mut n = ShellNursery::new();
        let s1 = n.spawn(config("s1")).unwrap();
        s1.lifecycle.transition(LifecycleEvent::Bootstrap).unwrap();
        s1.lifecycle.transition(LifecycleEvent::Ready).unwrap();
        s1.dna.record_use("test-path");

        n.tick_all();
        assert_eq!(n.get("s1").unwrap().total_ticks, 1);
        assert_eq!(n.get("s1").unwrap().dna.tick_count, 1);
    }

    #[test]
    fn test_nursery_tick_all_skips_non_running() {
        let mut n = ShellNursery::new();
        n.spawn(config("s1")).unwrap();
        // s1 is in Spawning state, not Running
        n.tick_all();
        assert_eq!(n.get("s1").unwrap().total_ticks, 0);
    }

    #[test]
    fn test_nursery_running() {
        let mut n = ShellNursery::new();
        let s1 = n.spawn(config("s1")).unwrap();
        s1.lifecycle.transition(LifecycleEvent::Bootstrap).unwrap();
        s1.lifecycle.transition(LifecycleEvent::Ready).unwrap();
        n.spawn(config("s2")).unwrap(); // still Spawning

        let running = n.running();
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].shell_id, "s1");
    }

    #[test]
    fn test_nursery_children_of() {
        let mut n = ShellNursery::new();
        let parent = n.spawn(config("parent")).unwrap();
        parent.lifecycle.transition(LifecycleEvent::Bootstrap).unwrap();
        parent.lifecycle.transition(LifecycleEvent::Ready).unwrap();

        n.spawn(config_with_parent("c1", "parent")).unwrap();
        n.spawn(config_with_parent("c2", "parent")).unwrap();
        n.spawn(config("orphan")).unwrap();

        let children = n.children_of("parent");
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn test_nursery_children_of_none() {
        let n = ShellNursery::new();
        assert!(n.children_of("nobody").is_empty());
    }

    #[test]
    fn test_nursery_lineage() {
        let mut n = ShellNursery::new();
        let gp = n.spawn(config("grandparent")).unwrap();
        gp.lifecycle.transition(LifecycleEvent::Bootstrap).unwrap();
        gp.lifecycle.transition(LifecycleEvent::Ready).unwrap();

        let _parent_config = config_with_parent("parent", "grandparent");
        // Need to make sure parent can be spawned
        let parent = n.spawn(config_with_parent("parent", "grandparent")).unwrap();
        parent.lifecycle.transition(LifecycleEvent::Bootstrap).unwrap();
        parent.lifecycle.transition(LifecycleEvent::Ready).unwrap();

        n.spawn(config_with_parent("child", "parent")).unwrap();

        let lineage = n.lineage("child");
        assert_eq!(lineage, vec!["child", "parent", "grandparent"]);
    }

    #[test]
    fn test_nursery_lineage_not_found() {
        let n = ShellNursery::new();
        let lineage = n.lineage("ghost");
        assert!(lineage.is_empty());
    }

    // --- Serialization roundtrip tests ---

    #[test]
    fn test_lifecycle_serialize_roundtrip() {
        let lc = ShellLifecycle::new();
        let json = serde_json::to_string(&lc).unwrap();
        let lc2: ShellLifecycle = serde_json::from_str(&json).unwrap();
        assert_eq!(lc, lc2);
    }

    #[test]
    fn test_dna_serialize_roundtrip() {
        let mut dna = DNA::new();
        dna.record_use("a");
        dna.record_use("b");
        let json = serde_json::to_string(&dna).unwrap();
        let dna2: DNA = serde_json::from_str(&json).unwrap();
        assert_eq!(dna, dna2);
    }

    #[test]
    fn test_profile_serialize_roundtrip() {
        let p = ShellProfile::new(config("test"), 42);
        let json = serde_json::to_string(&p).unwrap();
        let p2: ShellProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(p, p2);
    }

    #[test]
    fn test_nursery_serialize_roundtrip() {
        let mut n = ShellNursery::new();
        n.spawn(config("s1")).unwrap();
        let json = serde_json::to_string(&n).unwrap();
        let n2: ShellNursery = serde_json::from_str(&json).unwrap();
        assert_eq!(n, n2);
    }

    #[test]
    fn test_error_display() {
        let e = LifecycleError::InvalidTransition {
            from: ShellState::Running,
            to: ShellState::Conceived,
        };
        assert!(e.to_string().contains("Running"));
        assert!(e.to_string().contains("Conceived"));

        let e = LifecycleError::ShellNotFound("abc".into());
        assert!(e.to_string().contains("abc"));

        let e = LifecycleError::AlreadyExists("x".into());
        assert!(e.to_string().contains("x"));

        let e = LifecycleError::MaxChildrenReached(5);
        assert!(e.to_string().contains("5"));

        let e = LifecycleError::ParentNotRunning("p".into());
        assert!(e.to_string().contains("p"));
    }

    #[test]
    fn test_nursery_default() {
        let n = ShellNursery::default();
        assert!(n.shells.is_empty());
    }

    #[test]
    fn test_lifecycle_default() {
        let lc = ShellLifecycle::default();
        assert_eq!(lc.state, ShellState::Conceived);
    }

    #[test]
    fn test_dna_default() {
        let dna = DNA::default();
        assert!(dna.pathways.is_empty());
    }

    // --- Edge case / stress tests ---

    #[test]
    fn test_pathway_many_uses() {
        let mut p = Pathway::new("stress", "general");
        for _ in 0..100_000 {
            p.use_once();
        }
        assert!(p.strength < 1.0);
        assert!(p.strength > 0.9999);
        assert_eq!(p.use_count, 100_000);
    }

    #[test]
    fn test_dna_many_pathways() {
        let mut dna = DNA::new();
        for i in 0..100 {
            dna.record_use(&format!("path-{}", i));
        }
        assert_eq!(dna.pathways.len(), 100);
        assert!(dna.total_strength() > 0.0);
        assert!(dna.diversity() > 0.0);
    }

    #[test]
    fn test_dna_strongest_empty() {
        let dna = DNA::new();
        assert!(dna.strongest(5).is_empty());
    }

    #[test]
    fn test_dna_total_strength_empty() {
        let dna = DNA::new();
        assert_eq!(dna.total_strength(), 0.0);
    }

    #[test]
    fn test_nursery_multiple_children_different_parents() {
        let mut n = ShellNursery::new();
        let p1 = n.spawn(config("p1")).unwrap();
        p1.lifecycle.transition(LifecycleEvent::Bootstrap).unwrap();
        p1.lifecycle.transition(LifecycleEvent::Ready).unwrap();

        let p2 = n.spawn(config("p2")).unwrap();
        p2.lifecycle.transition(LifecycleEvent::Bootstrap).unwrap();
        p2.lifecycle.transition(LifecycleEvent::Ready).unwrap();

        n.spawn(config_with_parent("c1", "p1")).unwrap();
        n.spawn(config_with_parent("c2", "p1")).unwrap();
        n.spawn(config_with_parent("c3", "p2")).unwrap();

        assert_eq!(n.children_of("p1").len(), 2);
        assert_eq!(n.children_of("p2").len(), 1);
    }
}
