//! Timesifter: "At the beginning of each upkeep, each player exiles the top
//! card of their library. The player who exiled the card with the greatest
//! mana value takes an extra turn after this one. If two or more players'
//! cards are tied for greatest, the tied players repeat this process until the
//! tie is broken."
//!
//! Drives the real upkeep trigger and checks who receives the extra turn
//! (CR 500.7) and which cards each round exiled (CR 701.13a). Mana values are
//! read from the exiled cards (CR 202.3).

use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::game_state::ExtraTurn;
use engine::types::identifiers::ObjectId;
use engine::types::mana::ManaCost;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

const TIMESIFTER: &str = "At the beginning of each upkeep, each player exiles the top card of \
    their library. The player who exiled the card with the greatest mana value takes an extra \
    turn after this one. If two or more players' cards are tied for greatest, the tied players \
    repeat this process until the tie is broken.";

/// Stages `player`'s library top-first with cards of the given mana values,
/// above two filler cards so the draw step cannot deck anyone.
fn stack_library(
    scenario: &mut GameScenario,
    player: PlayerId,
    mana_values: &[u32],
) -> Vec<ObjectId> {
    scenario.with_library_top(player, &["Filler", "Filler"]);
    let mut ids: Vec<ObjectId> = mana_values
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &mv)| {
            scenario
                .add_spell_to_library_top(player, &format!("P{} card {i}", player.0), false)
                .with_mana_cost(ManaCost::generic(mv))
                .id()
        })
        .collect();
    ids.reverse();
    ids
}

/// Runs P0's upkeep with Timesifter on the battlefield and returns the final
/// extra-turn queue plus a zone lookup for the staged cards.
fn run_upkeep(p0_values: &[u32], p1_values: &[u32]) -> (Vec<ExtraTurn>, Vec<Zone>, Vec<Zone>) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::Untap);
    scenario.add_artifact_from_oracle(P0, "Timesifter", TIMESIFTER);
    let p0_cards = stack_library(&mut scenario, P0, p0_values);
    let p1_cards = stack_library(&mut scenario, P1, p1_values);

    let mut runner = scenario.build();
    runner.auto_advance_to_main_phase();
    runner.advance_until_stack_empty();

    let state = runner.state();
    let zones = |ids: &[ObjectId]| ids.iter().map(|id| state.objects[id].zone).collect();
    (
        state.extra_turns.clone(),
        zones(&p0_cards),
        zones(&p1_cards),
    )
}

#[test]
fn greatest_mana_value_takes_an_extra_turn_after_this_one() {
    let (extra_turns, p0_zones, p1_zones) = run_upkeep(&[2, 9], &[5, 0]);

    // CR 500.7: P1's extra turn comes directly after P0's current turn.
    assert_eq!(
        extra_turns,
        vec![ExtraTurn {
            player: P1,
            anchor: P0,
        }]
    );
    // One round: only the top cards were exiled.
    assert_eq!(p0_zones[0], Zone::Exile);
    assert_eq!(p1_zones[0], Zone::Exile);
    assert_ne!(p0_zones[1], Zone::Exile);
    assert_ne!(p1_zones[1], Zone::Exile);
}

#[test]
fn tied_players_repeat_until_the_tie_is_broken() {
    let (extra_turns, p0_zones, p1_zones) = run_upkeep(&[3, 1, 4], &[3, 1, 2]);

    // Rounds one and two tie (3 vs 3, 1 vs 1); round three breaks it for P0.
    assert_eq!(
        extra_turns,
        vec![ExtraTurn {
            player: P0,
            anchor: P0,
        }]
    );
    assert_eq!(p0_zones, vec![Zone::Exile; 3]);
    assert_eq!(p1_zones, vec![Zone::Exile; 3]);
}
