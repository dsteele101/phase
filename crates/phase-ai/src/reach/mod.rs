//! Lethal reach — "can I burn the opponent out from here, and with what?"
//!
//! ## The defect this closes
//!
//! The AI scored spells one candidate at a time. Holding Seal of Fire, Seal of
//! Fire, and Flame Rift with three lands against an opponent at 6 life, it cast
//! both Seals — each looked efficient on its own — for 4 damage and left the
//! opponent alive, while Seal + Flame Rift was exactly lethal. A Seal already on
//! the battlefield, whose damage costs no mana at all, was not folded into any
//! total either, so the AI could spend mana on new copies of an effect it
//! already held for free.
//!
//! ## Shape
//!
//! 1. [`sources`] prices every engine-issued cast or activation by the life it
//!    takes from one opponent (and from the AI), the mana it commits, and when
//!    it can be started. A permanent in hand whose own activated ability deals
//!    damage once it resolves (a Seal in hand) is priced as cast-then-activate;
//!    a permanent already on the battlefield is priced at its activation cost
//!    alone — for a sacrifice-only ability, no mana.
//! 2. [`solver`] searches SUBSETS of those sources — never hand order — for the
//!    cheapest combination whose total meets the opponent's life while the AI
//!    survives it (CR 104.4a: a simultaneous loss is a draw, not a win).
//! 3. [`driver`] certifies a proposed line by playing it on a cloned state
//!    through the engine reducer, assuming the opponent passes priority. The
//!    engine — not this module — decides what each spell actually does
//!    (prevention, protection, replacement, "can't lose life"), so an estimate
//!    can only propose a line; only a reducer-won game commits one.
//!
//! The certified line is consumed at the decision boundary in `search`: at
//! priority the AI takes the line's next step instead of scoring candidates
//! one at a time, and at the line's own prompts (mode, X, target) it takes the
//! answer that keeps the line lethal. A lethal line therefore always outranks
//! every non-lethal one, however efficient the alternative looks in isolation.
//!
//! Only a line that wins the game is committed (CR 104.2a): with more than one
//! opponent left, eliminating one of them is a strategic choice this module
//! does not make.

mod driver;
mod solver;
mod sources;

#[cfg(test)]
mod tests;

use engine::ai_support::flat_priority_actions;
use engine::game::engine::apply_as_current_for_simulation;
use engine::game::players;
use engine::game::static_abilities::player_has_cant_lose_life;
use engine::types::ability::TargetRef;
use engine::types::actions::GameAction;
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::player::PlayerId;

use driver::DriveOutcome;
use sources::ReachSource;

/// Most distinct candidate lines simulated per decision. Combinations are
/// tried cheapest-first, so the first few are the ones worth a reducer
/// simulation.
const MAX_CERTIFIED_LINES: usize = 4;

/// Most answers to one prompt simulated per decision — enough for every mode
/// pairing of a five-mode "choose two" spell.
const MAX_SIMULATED_ANSWERS: usize = 10;

/// The caller's per-action admission rule for engine-issued priority actions
/// (targeted-exchange and loop-safety gates in `search`). Re-applied to the
/// actions issued in a simulated, settled state so a line never relies on an
/// action the real decision boundary would refuse.
pub(crate) type ActionAdmission<'a> = &'a dyn Fn(&GameState, &GameAction) -> bool;

/// One engine action of a lethal line, plus the mode it commits to when the
/// action opens a modal spell (CR 700.2a).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LineStep {
    pub(crate) action: GameAction,
    pub(crate) mode: Option<usize>,
}

/// A reducer-certified sequence of actions that wins the game from the state
/// it was found in.
#[derive(Debug, Clone)]
pub(crate) struct LethalLine {
    /// Empty when what the AI already has on the stack wins once it resolves.
    pub(crate) steps: Vec<LineStep>,
}

/// The action that advances a certified lethal line from this priority
/// decision, or `None` when no line wins from here.
///
/// `issued` is the admitted, engine-issued priority domain of `state`.
pub(crate) fn lethal_priority_action(
    state: &GameState,
    ai_player: PlayerId,
    issued: &[GameAction],
    admission: ActionAdmission<'_>,
) -> Option<GameAction> {
    let line = find_lethal_line(state, ai_player, issued, admission)?;
    match line.steps.first() {
        Some(step) => issued_counterpart(issued, &step.action)
            .or_else(|| pass_to_resolve_stack(state, issued)),
        None => pass_to_resolve_stack(state, issued),
    }
}

/// The answer to one of the AI's own cast/activation prompts (mode, X, or
/// target) that keeps a game-winning line alive, or `None` when no answer does.
///
/// Each candidate answer is applied to a clone, the cast is driven back to the
/// AI's priority, and the result must either have won already or still hold a
/// certified lethal line.
pub(crate) fn lethal_prompt_action(
    state: &GameState,
    ai_player: PlayerId,
    issued: &[GameAction],
    admission: ActionAdmission<'_>,
) -> Option<GameAction> {
    let opponent = sole_opponent(state, ai_player)?;
    let answers = prompt_answers(state, ai_player, opponent, issued);
    if answers.is_empty() || !pending_source_could_finish(state, ai_player, opponent) {
        return None;
    }
    answers
        .into_iter()
        .take(MAX_SIMULATED_ANSWERS)
        .find(|answer| {
            let mut sim = state.clone();
            if apply_as_current_for_simulation(&mut sim, answer.clone()).is_err() {
                return false;
            }
            match driver::drive(sim, ai_player, opponent, &[], admission) {
                DriveOutcome::Won => true,
                DriveOutcome::Settled(settled) => {
                    let issued = admitted_priority_actions(&settled, admission);
                    find_lethal_line(&settled, ai_player, &issued, admission).is_some()
                }
                DriveOutcome::Stuck => false,
            }
        })
}

/// Search for and certify a game-winning line from an AI priority decision.
pub(crate) fn find_lethal_line(
    state: &GameState,
    ai_player: PlayerId,
    issued: &[GameAction],
    admission: ActionAdmission<'_>,
) -> Option<LethalLine> {
    if !matches!(state.waiting_for, WaitingFor::Priority { player } if player == ai_player) {
        return None;
    }
    let opponent = sole_opponent(state, ai_player)?;

    let current = sources::reach_sources(state, ai_player, opponent, issued);
    if state.stack.is_empty() {
        return certify_cheapest(state, state, ai_player, opponent, &current, admission);
    }

    // CR 117.4: with objects on the stack, the line is priced from the state
    // the stack settles into once every player passes — noninstant spells only
    // become castable there (CR 117.1a), and damage already on the stack has
    // landed. The settle is a reducer simulation, so gate it on an upper bound
    // that ignores mana and timing entirely.
    let ceiling = sources::potential_ceiling(state, ai_player, opponent, &current, None);
    if ceiling < life_of(state, opponent) {
        return None;
    }
    match driver::drive(state.clone(), ai_player, opponent, &[], admission) {
        DriveOutcome::Won => Some(LethalLine { steps: Vec::new() }),
        DriveOutcome::Settled(settled) => {
            let settled_issued = admitted_priority_actions(&settled, admission);
            let settled_sources =
                sources::reach_sources(&settled, ai_player, opponent, &settled_issued);
            certify_cheapest(
                state,
                &settled,
                ai_player,
                opponent,
                &settled_sources,
                admission,
            )
        }
        // Something on the stack needs a decision the driver will not make on
        // anyone's behalf; price only what can be done right now.
        DriveOutcome::Stuck => {
            certify_cheapest(state, state, ai_player, opponent, &current, admission)
        }
    }
}

/// Propose combinations priced against `priced` (the state whose life totals
/// and mana the sources were read from) and certify each from `origin` (the
/// real decision state), cheapest first.
fn certify_cheapest(
    origin: &GameState,
    priced: &GameState,
    ai_player: PlayerId,
    opponent: PlayerId,
    sources: &[ReachSource],
    admission: ActionAdmission<'_>,
) -> Option<LethalLine> {
    let opponent_life = life_of(priced, opponent);
    // Damage alone has to be able to get there before mana is worth pricing.
    let scales_with_x = sources.iter().any(|source| source.loss.opponent.per_x > 0);
    let fixed: u32 = sources
        .iter()
        .map(|source| source.loss.opponent.fixed)
        .sum();
    if !scales_with_x && fixed < opponent_life {
        return None;
    }
    let sources: Vec<_> = sources
        .iter()
        .filter_map(|source| source.priced(priced, ai_player))
        .collect();
    let budget = solver::Budget {
        opponent_life,
        controller_life: life_of(priced, ai_player),
        mana: sources::mana_capacity(priced, ai_player),
    };
    // Copies of one repeatable activation make many combinations that play
    // out identically; certify each distinct step sequence once.
    let mut attempted: Vec<Vec<LineStep>> = Vec::new();
    for members in solver::lethal_combinations(&sources, &budget) {
        let steps = solver::line_steps(&sources, &members);
        if attempted.contains(&steps) {
            continue;
        }
        if attempted.len() == MAX_CERTIFIED_LINES {
            break;
        }
        if matches!(
            driver::drive(origin.clone(), ai_player, opponent, &steps, admission),
            DriveOutcome::Won
        ) {
            return Some(LethalLine { steps });
        }
        attempted.push(steps);
    }
    None
}

/// CR 104.2a: burning out an opponent wins the game only when they are the
/// last opponent left. CR 119.8: an opponent who can't lose life can't be
/// burned out at all.
fn sole_opponent(state: &GameState, ai_player: PlayerId) -> Option<PlayerId> {
    match players::opponents(state, ai_player)[..] {
        [opponent] if !player_has_cant_lose_life(state, opponent) => Some(opponent),
        _ => None,
    }
}

fn life_of(state: &GameState, player: PlayerId) -> u32 {
    state.players[player.0 as usize].life.max(0) as u32
}

/// The engine-issued priority domain of a (simulated) state, filtered through
/// the caller's admission rule.
fn admitted_priority_actions(state: &GameState, admission: ActionAdmission<'_>) -> Vec<GameAction> {
    flat_priority_actions(state)
        .into_iter()
        .filter(|action| admission(state, action))
        .collect()
}

/// The issued action that performs `step` — matched by the object and ability
/// it starts rather than by exact payload, so a line found in one state still
/// recognises its cast when the engine issues it with a different payment
/// route in another.
pub(crate) fn issued_counterpart(issued: &[GameAction], step: &GameAction) -> Option<GameAction> {
    issued
        .iter()
        .find(|action| match (action, step) {
            (
                GameAction::CastSpell { object_id: a, .. },
                GameAction::CastSpell { object_id: b, .. },
            ) => a == b,
            (
                GameAction::ActivateAbility {
                    source_id: a,
                    ability_index: i,
                },
                GameAction::ActivateAbility {
                    source_id: b,
                    ability_index: j,
                },
            ) => a == b && i == j,
            _ => *action == step,
        })
        .cloned()
}

/// CR 117.4: when the line's next step is not available yet, the AI's own
/// objects on the stack have to resolve first — pass so they do.
fn pass_to_resolve_stack(state: &GameState, issued: &[GameAction]) -> Option<GameAction> {
    (!state.stack.is_empty() && issued.contains(&GameAction::PassPriority))
        .then_some(GameAction::PassPriority)
}

/// The issued answers to the AI's own pending cast prompt that a lethal line
/// could use, in issued order.
fn prompt_answers(
    state: &GameState,
    ai_player: PlayerId,
    opponent: PlayerId,
    issued: &[GameAction],
) -> Vec<GameAction> {
    match &state.waiting_for {
        // CR 601.2c: aim at the opponent.
        WaitingFor::TargetSelection { player, .. } if *player == ai_player => issued
            .iter()
            .filter(|action| {
                matches!(
                    action,
                    GameAction::ChooseTarget {
                        target: Some(TargetRef::Player(target)),
                    } if *target == opponent
                )
            })
            .cloned()
            .collect(),
        // CR 700.2a: any issued mode may be the damaging one.
        WaitingFor::ModeChoice { player, .. } if *player == ai_player => issued
            .iter()
            .filter(|action| matches!(action, GameAction::SelectModes { .. }))
            .cloned()
            .collect(),
        // CR 107.3a: an X that scales damage is announced at its maximum.
        WaitingFor::ChooseXValue { player, max, .. } if *player == ai_player => issued
            .iter()
            .filter(|action| matches!(action, GameAction::ChooseX { value } if value == max))
            .cloned()
            .collect(),
        _ => Vec::new(),
    }
}

/// The structural gate in front of the prompt simulation: the spell or ability
/// whose prompt is pending must be able to take life from `opponent`, and
/// everything the AI holds together must be able to reach their life total.
fn pending_source_could_finish(state: &GameState, ai_player: PlayerId, opponent: PlayerId) -> bool {
    let pending = match &state.waiting_for {
        WaitingFor::TargetSelection { pending_cast, .. }
        | WaitingFor::ModeChoice { pending_cast, .. }
        | WaitingFor::ChooseXValue { pending_cast, .. } => pending_cast,
        _ => return false,
    };
    state.objects.get(&pending.object_id).is_some_and(|object| {
        sources::object_reaches(state, ai_player, opponent, object)
            && sources::potential_ceiling(state, ai_player, opponent, &[], Some(object.id))
                >= life_of(state, opponent)
    })
}
