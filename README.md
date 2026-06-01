# lau-shell-lifecycle

**Shell lifecycle manager — from spawn to death, with self-assembling DNA pathways.**

A Rust library that models the complete lifecycle of shell instances as a deterministic state machine, layered with a biologically-inspired **DNA pathway system** that grows with use and decays with disuse. Ships with full serde support, 78 tests, and zero external runtime dependencies beyond serde/serde_json.

---

## What This Does

`lau-shell-lifecycle` manages the birth-to-death lifecycle of shell instances (think: agent processes, compute sessions, sandboxed workers). It provides:

1. **A strict lifecycle state machine** — shells progress through `Conceived → Spawning → Bootstrapping → Running` and can be suspended, migrated, or killed, with illegal transitions rejected at compile-time-enforced boundaries.
2. **Self-assembling DNA pathways** — every operation a shell performs is recorded as a "pathway" whose strength grows asymptotically toward 1.0 with repeated use and decays linearly when idle. Unused pathways are automatically pruned below a configurable threshold.
3. **A `ShellNursery`** — a container that enforces parent-child constraints (max children, parent must be running), tracks lineage, and batch-ticks DNA for all running shells.

Everything is `Serialize + Deserialize`, so you can snapshot an entire nursery to JSON and restore it.

---

## Key Idea

The core insight is that **shell behavior can be modeled as a living system**. Just as neural pathways in the brain strengthen with repetition and weaken with disuse, a shell's "DNA" adapts to its workload:

- **Used pathways grow**: Strength follows `s ← s + g·(1 − s)`, an asymptotic curve that approaches but never exceeds 1.0.
- **Unused pathways decay**: Each tick applies `s ← s − d`, clamped to 0.
- **Dead pathways are pruned**: Below `prune_threshold` (default 0.01), the pathway is removed entirely.

This creates a self-organizing profile: a shell that routes heavily will develop strong routing pathways, while its unused tile-rendering pathways fade away. The **Shannon entropy** of the pathway distribution (`DNA::diversity()`) measures how specialized vs. generalized a shell has become.

---

## Install

Add to your `Cargo.toml`:

```toml
[dependencies]
lau-shell-lifecycle = "0.1.0"
```

Or via `cargo add`:

```sh
cargo add lau-shell-lifecycle
```

### Requirements

- Rust 2021 edition (MSRV: 1.56+)
- `serde` 1.x with `derive` feature
- `serde_json` 1.x

---

## Quick Start

### Spawn and advance a shell through its lifecycle

```rust
use lau_shell_lifecycle::*;

// Configure a new shell
let config = ShellConfig {
    id: "shell-001".into(),
    name: "worker-alpha".into(),
    shell_type: ShellType::Hermes,
    universe_path: "/universe".into(),
    conservation_budget: 100.0,
    parent_id: None,
    max_children: 5,
    capabilities: vec!["read".into(), "write".into()],
    model: None,
};

// Create a nursery and spawn
let mut nursery = ShellNursery::new();
let shell = nursery.spawn(config).unwrap();

// Advance to Running
shell.lifecycle.transition(LifecycleEvent::Bootstrap).unwrap();
shell.lifecycle.transition(LifecycleEvent::Ready).unwrap();
assert_eq!(shell.lifecycle.state, ShellState::Running);
```

### Build DNA through usage

```rust
let shell = nursery.get_mut("shell-001").unwrap();

// Record pathway usage (creates pathways on first use)
shell.dna.record_use("routing/dispatch-main");
shell.dna.record_use("routing/dispatch-main");
shell.dna.record_use("provider/openai-gpt4");
shell.dna.record_use("room/lobby");

// Check adaptation
let top = shell.dna.strongest(3);
let diversity = shell.dna.diversity();    // Shannon entropy
let total = shell.dna.total_strength();    // Sum of all pathway strengths
let adaptation = shell.adaptation_score(); // diversity × total_strength
```

### Parent-child relationships with constraints

```rust
// Parent must be Running to accept children
let parent = nursery.spawn(parent_config).unwrap();
parent.lifecycle.transition(LifecycleEvent::Bootstrap).unwrap();
parent.lifecycle.transition(LifecycleEvent::Ready).unwrap();

// Spawn a child (parent_id links them)
let child_config = ShellConfig { parent_id: Some("parent".into()), ..child_base };
nursery.spawn(child_config).unwrap();

// Enforced constraints:
// - Parent must exist and be Running
// - Parent must not exceed max_children

// Trace ancestry
let lineage = nursery.lineage("child"); // ["child", "parent"]
```

### Suspend, migrate, kill

```rust
let shell = nursery.get_mut("shell-001").unwrap();

shell.lifecycle.transition(LifecycleEvent::Suspend { reason: "maintenance".into() }).unwrap();
assert_eq!(shell.lifecycle.state, ShellState::Suspended);
assert_eq!(shell.lifecycle.suspend_reason.as_deref(), Some("maintenance"));

shell.lifecycle.transition(LifecycleEvent::Resume).unwrap();

shell.lifecycle.transition(LifecycleEvent::Migrate { target: "gpu-node-03".into() }).unwrap();
assert_eq!(shell.lifecycle.state, ShellState::Migrating);

shell.lifecycle.transition(LifecycleEvent::Ready).unwrap(); // back to Running

// Kill via nursery (removes from nursery, returns the dead profile)
let killed = nursery.kill("shell-001", "evicted").unwrap();
assert_eq!(killed.lifecycle.state, ShellState::Dead);
```

### Serialize everything to JSON

```rust
let nursery_json = serde_json::to_string(&nursery).unwrap();
let restored: ShellNursery = serde_json::from_str(&nursery_json).unwrap();
assert_eq!(nursery, restored);
```

---

## API Reference

### Types

| Type | Role |
|---|---|
| `ShellType` | Enum: `Hermes`, `ZeroClaw`, `CUDAClaw`, `GitNative`, `Remote { address }`, `Custom(String)` |
| `ShellConfig` | Immutable configuration for a shell (id, type, budget, max children, capabilities, model) |
| `ShellState` | 8-state lifecycle enum: `Conceived → Spawning → Bootstrapping → Running → Suspended/Migrating/Dying → Dead` |
| `LifecycleEvent` | Events that trigger transitions: `Spawn`, `Bootstrap`, `Ready`, `Suspend`, `Resume`, `Migrate`, `Kill`, `HeartbeatReceived`, `Timeout`, `Error` |
| `LifecycleError` | Error enum: `InvalidTransition`, `ShellNotFound`, `AlreadyExists`, `MaxChildrenReached`, `ParentNotRunning` |
| `ShellLifecycle` | Per-shell state machine tracking current state, kill/suspend/migration reasons |
| `Pathway` | Single DNA pathway with name, use_count, strength ∈ [0, 1], decay/growth rates, category |
| `DNA` | Collection of pathways with tick-based decay, automatic pruning, strength ranking, and diversity metrics |
| `ShellProfile` | Full shell identity: lifecycle + DNA + stats (born_at, ticks, energy, messages, children) |
| `ShellNursery` | Container managing all shells with spawn/kill/tick/query operations |

### Key Methods

#### `ShellLifecycle`
- `new()` → starts at `Conceived`
- `can_transition_to(&ShellState)` → `bool`
- `transition(LifecycleEvent)` → `Result<(), LifecycleError>`

#### `DNA`
- `record_use(pathway: &str)` → creates or strengthens a pathway
- `tick()` → decay all pathways, prune below threshold
- `strongest(n)` → top-N pathways by strength
- `total_strength()` → `f64` sum
- `diversity()` → Shannon entropy in bits

#### `ShellProfile`
- `pathway_count()` → number of active pathways
- `age_seconds(now)` → age from birth
- `efficiency()` → `(messages_sent + messages_received) / total_energy_used`
- `adaptation_score()` → `diversity × total_strength`

#### `ShellNursery`
- `spawn(config)` → `Result<&mut ShellProfile, LifecycleError>` — validates parent, advances to Spawning
- `kill(id, reason)` → `Result<ShellProfile, LifecycleError>` — transitions Dying → Dead, removes from nursery
- `get(id)` / `get_mut(id)` → look up by id
- `tick_all()` — tick DNA for all Running shells
- `running()` → all running profiles
- `children_of(parent_id)` → child profiles
- `lineage(id)` → ancestry chain from id to root

---

## How It Works

### State Machine

The lifecycle is a **deterministic finite automaton** with 8 states:

```
Conceived ──► Spawning ──► Bootstrapping ──► Running ◄──► Suspended
                                                  │  ▲
                                                  │  │
                                                  ▼  │
                                              Migrating
                                                  │
                                                  ▼
                                              Dying ──► Dead
```

Legal transitions are defined by `ShellState::legal_transitions()` — each state returns a static slice of valid targets. The `transition()` method checks legality and returns `Err(InvalidTransition)` on violations.

**Special case — Kill**: The `Kill` event can be applied from `Running`, `Suspended`, `Spawning`, `Bootstrapping`, or `Migrating` — not just `Running`. This prevents shells from being stuck in intermediate states.

### DNA Pathway System

Each pathway has:
- `strength: f64` ∈ [0.0, 1.0]
- `growth_rate: f64` (default 0.1)
- `decay_rate: f64` (default 0.01)

**Growth** on each use:
```
s ← s + g · (1 − s)
```
This is an **asymptotic** function — early uses cause large jumps, later uses produce diminishing returns. A pathway can never exceed 1.0.

**Decay** on each tick:
```
s ← max(s − d, 0)
```
Linear decay with a floor at zero.

**Pruning**: After decay, any pathway with `strength < prune_threshold` (default 0.01) is removed from the collection.

**Category inference**: Pathways are auto-categorized by name matching — `routing`, `provider`, `room`, `tile`, `circuit`, or `general`.

### ShellNursery as Orchestrator

The nursery enforces **parent-child invariants**:
- A parent must exist and be in `Running` state to accept children
- A parent cannot exceed its `max_children` limit
- Killing a shell removes it from the nursery (but does NOT cascade to children — they become orphans)

---

## The Math

### Asymptotic Growth

For a pathway with growth rate `g` and `n` uses starting from strength 0:

```
s_n = 1 − (1 − g)^n
```

With `g = 0.1`:
- 1 use: s = 0.1
- 5 uses: s ≈ 0.41
- 10 uses: s ≈ 0.65
- 50 uses: s ≈ 0.995
- ∞ uses: s → 1.0

### Linear Decay

After `k` ticks without use, a pathway with strength `s₀` becomes:

```
s_k = max(s₀ − k · d, 0)
```

With `d = 0.01`, a pathway at strength 0.1 survives 10 ticks before being pruned (strength drops below 0.01 at tick 10).

### Shannon Entropy (Diversity)

The diversity metric is the **Shannon entropy** of the normalized pathway strength distribution:

```
H = − Σ (s_i / S) · log₂(s_i / S)
```

where `S = Σ s_i` is the total strength.

- `H = 0` when all strength is concentrated in one pathway (fully specialized)
- `H = log₂(n)` when all `n` pathways have equal strength (fully generalized)
- For two equal pathways: `H = 1.0` bit

### Adaptation Score

```
A = H × S
```

This combines specialization/diversity with raw capability. A shell with many strong, balanced pathways scores highest. A shell with one dominant pathway scores lower (low diversity) even if total strength is high.

---

## License

MIT
