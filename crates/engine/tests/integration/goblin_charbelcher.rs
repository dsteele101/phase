//! CR 701.20a + CR 608.2c + CR 120.3: Goblin Charbelcher — "{3}, {T}: Reveal
//! cards from the top of your library until you reveal a land card. This
//! artifact deals damage equal to the number of nonland cards revealed this way
//! to any target. If the revealed land card was a Mountain, this artifact deals
//! double that damage instead. Put the revealed cards on the bottom of your
//! library in any order."
//!
//! The revealed land card is NOT put into a hand: every revealed card, the land
//! included, goes to the library bottom.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const CHARBELCHER: &str = "{3}, {T}: Reveal cards from the top of your library until you reveal a land card. Goblin Charbelcher deals damage equal to the number of nonland cards revealed this way to any target. If the revealed land card was a Mountain, Goblin Charbelcher deals double that damage instead. Put the revealed cards on the bottom of your library in any order.";

/// A library staged top-first as `nonland_count` nonland cards, then a land with
/// `land_subtype`, then one untouched card. Returns the revealed cards in
/// encounter order plus the untouched card.
fn stage(
    nonland_count: usize,
    land_name: &str,
    land_subtype: &str,
) -> (GameRunner, Vec<ObjectId>, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_artifact_from_oracle(P0, "Goblin Charbelcher", CHARBELCHER);
    scenario.with_mana_pool(
        P0,
        (0..3)
            .map(|_| ManaUnit::new(ManaType::Colorless, ObjectId(0), false, vec![]))
            .collect(),
    );

    // `library[0]` is the top: stage bottom-up.
    let deep = scenario.add_card_to_library_top(P0, "Deep Card");
    let land = scenario
        .add_spell_to_library_top(P0, land_name, false)
        .as_land()
        .with_subtypes(vec![land_subtype])
        .id();
    let mut revealed = vec![land];
    for i in 0..nonland_count {
        let spell = scenario
            .add_spell_to_library_top(P0, &format!("Spell {i}"), false)
            .id();
        revealed.insert(0, spell);
    }
    (scenario.build(), revealed, deep)
}

fn charbelcher(runner: &GameRunner) -> ObjectId {
    runner
        .state()
        .battlefield
        .iter()
        .copied()
        .find(|id| runner.state().objects[id].name == "Goblin Charbelcher")
        .expect("Goblin Charbelcher on the battlefield")
}

/// Activate targeting P1 and answer the library-bottom ordering prompt (if the
/// engine raises one) with the encounter order.
fn activate_and_finish(runner: &mut GameRunner, revealed: &[ObjectId]) {
    let source = charbelcher(runner);
    runner.activate(source, 0).target_player(P1).resolve();
    if let WaitingFor::RevealUntilBottomOrder { cards, .. } = &runner.state().waiting_for {
        assert_eq!(
            cards, revealed,
            "every revealed card is ordered onto the bottom"
        );
        runner
            .act(GameAction::SelectCards {
                cards: revealed.to_vec(),
            })
            .expect("bottom order is accepted");
    }
    runner.advance_until_stack_empty();
}

fn assert_all_on_bottom_and_none_in_hand(
    runner: &GameRunner,
    revealed: &[ObjectId],
    deep: ObjectId,
) {
    let library: Vec<ObjectId> = runner.state().players[P0.0 as usize]
        .library
        .iter()
        .copied()
        .collect();
    let mut expected = vec![deep];
    expected.extend_from_slice(revealed);
    assert_eq!(
        library, expected,
        "the revealed cards, land included, go to the bottom of the library"
    );
    for id in revealed {
        assert_eq!(runner.state().objects[id].zone, Zone::Library);
    }
    assert!(
        runner.state().players[P0.0 as usize].hand.is_empty(),
        "no revealed card is put into a hand"
    );
}

#[test]
fn deals_damage_equal_to_nonland_cards_revealed() {
    let (mut runner, revealed, deep) = stage(3, "Forest", "Forest");
    let life_before = runner.state().players[P1.0 as usize].life;

    activate_and_finish(&mut runner, &revealed);

    assert_eq!(
        runner.state().players[P1.0 as usize].life,
        life_before - 3,
        "three nonland cards were revealed before the Forest"
    );
    assert_all_on_bottom_and_none_in_hand(&runner, &revealed, deep);
}

#[test]
fn mountain_doubles_the_damage() {
    let (mut runner, revealed, deep) = stage(3, "Mountain", "Mountain");
    let life_before = runner.state().players[P1.0 as usize].life;

    activate_and_finish(&mut runner, &revealed);

    assert_eq!(
        runner.state().players[P1.0 as usize].life,
        life_before - 6,
        "a revealed Mountain doubles the three nonland cards' damage"
    );
    assert_all_on_bottom_and_none_in_hand(&runner, &revealed, deep);
}

#[test]
fn land_on_top_reveals_only_the_land_and_deals_no_damage() {
    let (mut runner, revealed, deep) = stage(0, "Mountain", "Mountain");
    let life_before = runner.state().players[P1.0 as usize].life;

    activate_and_finish(&mut runner, &revealed);

    assert_eq!(
        runner.state().players[P1.0 as usize].life,
        life_before,
        "zero nonland cards revealed: zero damage, doubled or not"
    );
    assert_all_on_bottom_and_none_in_hand(&runner, &revealed, deep);
}
