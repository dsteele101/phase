//! CR 607.1 + CR 607.2a + CR 406.6 + CR 610.3: Tidehollow Sculler exile and return.
//!
//! "When this creature enters, target opponent reveals their hand and you choose a
//! nonland card from it. Exile that card.
//! When this creature leaves the battlefield, return the exiled card to its
//! owner's hand."
//!
//! Two-trigger linked exile-return: when Tidehollow Sculler leaves the battlefield,
//! the card exiled from the opponent's hand must be returned to its owner's hand.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::{ExileLinkKind, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const TIDEHOLLOW_SCULLER: &str = "When this creature enters, target opponent reveals their hand and you choose a nonland card from it. Exile that card.\n\
When this creature leaves the battlefield, return the exiled card to its owner's hand.";

const MESMERIC_FIEND: &str = "When this creature enters, target opponent reveals their hand and you choose a nonland card from it. Exile that card.\n\
When this creature leaves the battlefield, return the exiled card to its owner's hand.";

const MURDER: &str = "Destroy target creature.";

fn zone_of(runner: &GameRunner, id: ObjectId) -> Zone {
    runner.state().objects.get(&id).unwrap().zone
}

#[test]
fn tidehollow_sculler_returns_exiled_card_to_hand_when_it_leaves_play() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let sculler = scenario
        .add_creature_to_hand_from_oracle(P0, "Tidehollow Sculler", 2, 2, TIDEHOLLOW_SCULLER)
        .id();

    let target_card = scenario.add_card_to_hand(P1, "Doom Blade");
    let land_card = scenario.add_land_to_hand(P1, "Swamp").id();
    let destroy = scenario
        .add_spell_to_hand_from_oracle(P0, "Murder", true, MURDER)
        .id();

    let mut runner = scenario.build();

    let outcome = runner.cast(sculler).target_player(P1).resolve();

    let WaitingFor::RevealChoice { cards, .. } = outcome.final_waiting_for() else {
        panic!(
            "expected RevealChoice prompt after ETB resolves, got {:?}",
            outcome.final_waiting_for()
        );
    };
    assert!(cards.contains(&target_card));
    assert!(!cards.contains(&land_card));

    runner
        .act(GameAction::SelectCards {
            cards: vec![target_card],
        })
        .expect("SelectCards succeeds");

    assert_eq!(zone_of(&runner, target_card), Zone::Exile);
    assert_eq!(zone_of(&runner, land_card), Zone::Hand);
    assert!(runner.state().exile_links.iter().any(|link| {
        link.exiled_id == target_card
            && link.source_id == sculler
            && link.kind
                == ExileLinkKind::UntilSourceLeaves {
                    return_zone: Zone::Hand,
                }
    }));

    // Destroy Tidehollow Sculler so it leaves the battlefield.
    runner.cast(destroy).target_object(sculler).resolve();
    runner.advance_until_stack_empty();

    assert_eq!(zone_of(&runner, sculler), Zone::Graveyard);
    assert_eq!(
        zone_of(&runner, target_card),
        Zone::Hand,
        "Target card must return to hand when Tidehollow Sculler leaves the battlefield"
    );
    assert!(runner.state().players[P1.0 as usize]
        .hand
        .contains(&target_card));
    assert!(!runner.state().exile.contains(&target_card));
    assert!(
        runner.state().exile_links.is_empty(),
        "ExileLink must be consumed"
    );
}

#[test]
fn mesmeric_fiend_returns_exiled_card_to_hand_when_it_leaves_play() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let fiend = scenario
        .add_creature_to_hand_from_oracle(P0, "Mesmeric Fiend", 1, 1, MESMERIC_FIEND)
        .id();

    let target_card = scenario.add_card_to_hand(P1, "Counterspell");
    let destroy = scenario
        .add_spell_to_hand_from_oracle(P0, "Murder", true, MURDER)
        .id();

    let mut runner = scenario.build();

    let outcome = runner.cast(fiend).target_player(P1).resolve();

    let WaitingFor::RevealChoice { .. } = outcome.final_waiting_for() else {
        panic!(
            "expected RevealChoice prompt after ETB resolves, got {:?}",
            outcome.final_waiting_for()
        );
    };

    runner
        .act(GameAction::SelectCards {
            cards: vec![target_card],
        })
        .expect("SelectCards succeeds");

    assert_eq!(zone_of(&runner, target_card), Zone::Exile);

    runner.cast(destroy).target_object(fiend).resolve();
    runner.advance_until_stack_empty();

    assert_eq!(zone_of(&runner, fiend), Zone::Graveyard);
    assert_eq!(
        zone_of(&runner, target_card),
        Zone::Hand,
        "Target card must return to hand when Mesmeric Fiend leaves the battlefield"
    );
    assert!(runner.state().players[P1.0 as usize]
        .hand
        .contains(&target_card));
    assert!(!runner.state().exile.contains(&target_card));
    assert!(
        runner.state().exile_links.is_empty(),
        "ExileLink must be consumed"
    );
}

#[test]
fn tidehollow_sculler_with_no_nonland_cards_in_opponents_hand() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let sculler = scenario
        .add_creature_to_hand_from_oracle(P0, "Tidehollow Sculler", 2, 2, TIDEHOLLOW_SCULLER)
        .id();

    let land_card = scenario.add_land_to_hand(P1, "Plains").id();
    let destroy = scenario
        .add_spell_to_hand_from_oracle(P0, "Murder", true, MURDER)
        .id();

    let mut runner = scenario.build();

    let _outcome = runner.cast(sculler).target_player(P1).resolve();

    assert_eq!(zone_of(&runner, land_card), Zone::Hand);
    assert!(runner.state().exile_links.is_empty());

    runner.cast(destroy).target_object(sculler).resolve();
    runner.advance_until_stack_empty();

    assert_eq!(zone_of(&runner, sculler), Zone::Graveyard);
    assert_eq!(zone_of(&runner, land_card), Zone::Hand);
    assert!(runner.state().exile_links.is_empty());
}
