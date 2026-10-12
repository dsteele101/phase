use crate::types::ability::{Effect, EffectError, ResolvedAbility};
use crate::types::contest::{ContestConfig, ContestState};
use crate::types::events::GameEvent;
use crate::types::game_state::{GameState, WaitingFor};

/// Eye to Eye / Mouth to Mouth: "You and [player] have a [staring | breath-holding]
/// contest."
///
/// The Comprehensive Rules define no physical contests, so there is no CR number for
/// the contest itself. The online adaptation (`types::contest`) pauses resolution on
/// `WaitingFor::PhysicalContest`; the host drives it through
/// `game::contest::{start_contest, advance_contest, contest_click, ...}` and the
/// settled result gates the printed "if you win" rider.
///
/// CR 608.2b: the second contestant is a player the ability references (a target's
/// controller, or a previously declared player target). If it no longer names a legal
/// player — an illegal target, or a player who left the game — the instruction cannot
/// be performed and nothing happens; the rider's `EventOutcomeWon` gate stays false.
/// CR 608.2c: a contest needs two distinct participants, so a contestant that
/// resolves to the controller likewise performs nothing.
pub fn resolve(
    state: &mut GameState,
    ability: &ResolvedAbility,
    events: &mut Vec<GameEvent>,
) -> Result<(), EffectError> {
    let Effect::PhysicalContest { kind, contestant } = &ability.effect else {
        return Ok(());
    };
    let controller = ability.controller;
    let Some(opponent) = super::resolve_player_for_context_ref(state, ability, contestant)
        .filter(|&player| player != controller && crate::game::players::is_alive(state, player))
    else {
        return Ok(());
    };

    // `Pending` until the host supplies a clock reading (`start_contest`); the
    // engine itself never reads time.
    // Identity: the state revision at resolution. Every contest resolves inside a
    // distinct action, and each action advances the revision, so two contests of one
    // game never share an id; the host stamps frames with it so a frame delayed from
    // an earlier contest is never counted in a later one. Offset by one so `0` is
    // never a valid identity.
    let contest = ContestState::new(
        state.state_revision.saturating_add(1),
        *kind,
        controller,
        opponent,
        ContestConfig::default(),
    );
    events.push(GameEvent::PhysicalContestStarted {
        kind: *kind,
        participants: contest.players(),
    });
    state.waiting_for = WaitingFor::PhysicalContest {
        contest: Box::new(contest),
        source_id: ability.source_id,
    };
    Ok(())
}
