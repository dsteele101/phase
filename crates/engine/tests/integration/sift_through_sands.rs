use engine::game::scenario::{GameScenario, P0};
use engine::game::scenario_db::GameScenarioDbExt;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaCost, ManaType, ManaUnit};
use engine::types::phase::Phase;

const SIFT_THROUGH_SANDS_ORACLE: &str =
    "Draw two cards, then discard a card.\nIf you've cast a spell named Peer Through Depths and a spell named Reach Through Mists this turn, you may search your library for a card named The Unspeakable, put it onto the battlefield, then shuffle.";

#[test]
fn sift_through_sands_draws_two_then_discards_one() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Sift Through Sands", true, SIFT_THROUGH_SANDS_ORACLE)
        .with_mana_cost(ManaCost::zero())
        .id();
    scenario.add_card_to_library_top(P0, "Card 1");
    scenario.add_card_to_library_top(P0, "Card 2");
    scenario.add_card_to_library_top(P0, "Card 3");
    let mut runner = scenario.build();

    let committed = runner.cast(spell).commit();
    let outcome = committed.resolve();

    println!("Events during Sift Through Sands resolution:");
    for (i, ev) in outcome.events().iter().enumerate() {
        println!("  Event {}: {:?}", i, ev);
    }
    println!("Final waiting for: {:?}", outcome.final_waiting_for());

    let chosen_card = match outcome.final_waiting_for() {
        WaitingFor::DiscardChoice { cards, count, .. } => {
            assert_eq!(*count, 1, "Must ask to discard 1 card");
            assert_eq!(cards.len(), 2, "Hand must contain the 2 drawn cards");
            cards[0]
        }
        other => panic!("Expected WaitingFor::DiscardChoice, got {:?}", other),
    };

    let outcome2 = runner
        .act(engine::types::actions::GameAction::SelectCards {
            cards: vec![chosen_card],
        })
        .unwrap();
    println!("Outcome 2 waiting for: {:?}", outcome2.waiting_for);
    println!(
        "Final P0 hand after discard: {:?}",
        runner.state().players[P0.0 as usize].hand
    );
    assert_eq!(
        runner.state().players[P0.0 as usize].hand.len(),
        1,
        "Hand must have 1 card after drawing 2 and discarding 1"
    );
}

const SIFT_THROUGH_SANDS_USER_ORACLE: &str =
    "Draw two cards, then discard a card.\n\nIf you’ve cast a spell named Peer Through Depths and a spell named Reach Through Mists this turn, you may search your library for a card named The Unspeakable, put it onto the battlefield, then shuffle.";

const SIFT_THROUGH_SANDS_SINGLE_NL_CURLY: &str =
    "Draw two cards, then discard a card.\nIf you’ve cast a spell named Peer Through Depths and a spell named Reach Through Mists this turn, you may search your library for a card named The Unspeakable, put it onto the battlefield, then shuffle.";

#[test]
fn sift_through_sands_single_nl_curly_inspect() {
    let parsed = engine::parser::parse_oracle_text(
        SIFT_THROUGH_SANDS_SINGLE_NL_CURLY,
        "Sift Through Sands",
        &[],
        &["Instant".to_string()],
        &["Arcane".to_string()],
    );
    println!(
        "SINGLE NL CURLY abilities count: {}",
        parsed.abilities.len()
    );
    for (i, a) in parsed.abilities.iter().enumerate() {
        println!("Ability {}: {:#?}", i, a);
    }
}

#[test]
fn sift_through_sands_parses_user_oracle() {
    let parsed = engine::parser::parse_oracle_text(
        SIFT_THROUGH_SANDS_USER_ORACLE,
        "Sift Through Sands",
        &[],
        &["Instant".to_string()],
        &["Arcane".to_string()],
    );
    println!("Parsed abilities count: {}", parsed.abilities.len());
    for (i, a) in parsed.abilities.iter().enumerate() {
        println!("Ability {}: {:?}", i, a);
    }
}

#[test]
fn sift_through_sands_user_oracle_draws_two() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let spell = scenario
        .add_spell_to_hand_from_oracle(
            P0,
            "Sift Through Sands",
            true,
            SIFT_THROUGH_SANDS_USER_ORACLE,
        )
        .with_mana_cost(ManaCost::zero())
        .id();
    scenario.add_card_to_library_top(P0, "Card 1");
    scenario.add_card_to_library_top(P0, "Card 2");
    scenario.add_card_to_library_top(P0, "Card 3");
    let mut runner = scenario.build();

    let committed = runner.cast(spell).commit();
    let outcome = committed.resolve();

    println!(
        "User oracle final waiting for: {:?}",
        outcome.final_waiting_for()
    );
    println!(
        "User oracle P0 hand: {:?}",
        runner.state().players[P0.0 as usize].hand
    );

    match outcome.final_waiting_for() {
        WaitingFor::DiscardChoice { cards, count, .. } => {
            assert_eq!(*count, 1, "Must ask to discard 1 card");
            assert_eq!(cards.len(), 2, "Hand must contain the 2 drawn cards");
        }
        other => panic!("Expected WaitingFor::DiscardChoice, got {:?}", other),
    }
}

#[test]
fn sift_through_sands_real_card_from_db_draws_two() {
    let Some(db) = crate::support::shared_card_db() else {
        return;
    };
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let sift_id = scenario.add_real_card(
        P0,
        "Sift Through Sands",
        engine::types::zones::Zone::Hand,
        db,
    );
    scenario.add_real_card(P0, "Island", engine::types::zones::Zone::Library, db);
    scenario.add_real_card(P0, "Island", engine::types::zones::Zone::Library, db);
    scenario.add_real_card(P0, "Island", engine::types::zones::Zone::Library, db);
    let mut runner = scenario.build();
    engine::game::rehydrate_game_from_card_db(runner.state_mut(), db);
    runner.state_mut().debug_mode = true;

    // Give P0 plenty of mana to cast Sift Through Sands (1UU)
    let mana = vec![
        ManaUnit::new(ManaType::Blue, ObjectId(0), false, vec![]),
        ManaUnit::new(ManaType::Blue, ObjectId(0), false, vec![]),
        ManaUnit::new(ManaType::Colorless, ObjectId(0), false, vec![]),
    ];
    for u in mana {
        runner.state_mut().players[P0.0 as usize].mana_pool.add(u);
    }

    let initial_hand_len = runner.state().players[P0.0 as usize].hand.len();
    assert_eq!(
        initial_hand_len, 1,
        "P0 should only have Sift Through Sands in hand"
    );

    let committed = runner.cast(sift_id).commit();
    let outcome = committed.resolve();

    println!(
        "Real card outcome waiting for: {:?}",
        outcome.final_waiting_for()
    );
    println!(
        "Real card P0 hand: {:?}",
        runner.state().players[P0.0 as usize].hand
    );

    match outcome.final_waiting_for() {
        WaitingFor::DiscardChoice { cards, count, .. } => {
            assert_eq!(*count, 1, "Must ask to discard 1 card");
            assert_eq!(cards.len(), 2, "Hand must contain the 2 drawn cards");
        }
        other => panic!("Expected WaitingFor::DiscardChoice, got {:?}", other),
    }
}

#[test]
fn sift_through_sands_without_prior_spells_does_not_search() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let spell = scenario
        .add_spell_to_hand_from_oracle(
            P0,
            "Sift Through Sands",
            true,
            SIFT_THROUGH_SANDS_USER_ORACLE,
        )
        .with_mana_cost(ManaCost::zero())
        .id();
    scenario.add_card_to_library_top(P0, "The Unspeakable");
    scenario.add_card_to_library_top(P0, "Card 2");
    scenario.add_card_to_library_top(P0, "Card 1");
    let mut runner = scenario.build();

    let committed = runner.cast(spell).commit();
    let outcome = committed.resolve();

    let discard_card = match outcome.final_waiting_for() {
        WaitingFor::DiscardChoice { cards, .. } => cards[0],
        other => panic!("Expected WaitingFor::DiscardChoice, got {:?}", other),
    };

    let outcome2 = runner
        .act(engine::types::actions::GameAction::SelectCards {
            cards: vec![discard_card],
        })
        .unwrap();

    // Because Peer Through Depths and Reach Through Mists were not cast,
    // the search ability must not trigger/resolve. It should return directly to Priority.
    println!("Without prior spells outcome: {:?}", outcome2.waiting_for);
    assert!(
        matches!(outcome2.waiting_for, WaitingFor::Priority { .. }),
        "Expected WaitingFor::Priority, got {:?}",
        outcome2.waiting_for
    );
    // The Unspeakable should still be in the library, not on the battlefield
    let unspeakable_obj = runner
        .state()
        .objects
        .values()
        .find(|obj| obj.name.eq_ignore_ascii_case("The Unspeakable"))
        .expect("The Unspeakable should exist");
    assert_eq!(
        unspeakable_obj.zone,
        engine::types::zones::Zone::Library,
        "The Unspeakable must remain in the library"
    );
}

#[test]
fn sift_through_sands_with_prior_spells_searches_the_unspeakable() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let reach = scenario
        .add_spell_to_hand_from_oracle(P0, "Reach Through Mists", true, "Draw a card.")
        .with_mana_cost(ManaCost::zero())
        .id();
    let peer = scenario
        .add_spell_to_hand_from_oracle(P0, "Peer Through Depths", true, "Draw a card.")
        .with_mana_cost(ManaCost::zero())
        .id();
    let sift = scenario
        .add_spell_to_hand_from_oracle(
            P0,
            "Sift Through Sands",
            true,
            SIFT_THROUGH_SANDS_USER_ORACLE,
        )
        .with_mana_cost(ManaCost::zero())
        .id();

    // Populate library: The Unspeakable deep in library, then filler cards on top
    let unspeakable_id = scenario.add_card_to_library_top(P0, "The Unspeakable");
    scenario.add_card_to_library_top(P0, "Card 1");
    scenario.add_card_to_library_top(P0, "Card 2");
    scenario.add_card_to_library_top(P0, "Card 3");
    scenario.add_card_to_library_top(P0, "Card 4");
    scenario.add_card_to_library_top(P0, "Card 5");
    scenario.add_card_to_library_top(P0, "Card 6");
    scenario.add_card_to_library_top(P0, "Card 7");

    let mut runner = scenario.build();

    // 1. Cast and resolve Reach Through Mists
    let r1 = runner.cast(reach).commit().resolve();
    assert!(
        matches!(r1.final_waiting_for(), WaitingFor::Priority { .. }),
        "Reach Through Mists should resolve cleanly to Priority"
    );

    // 2. Cast and resolve Peer Through Depths
    let r2 = runner.cast(peer).commit().resolve();
    assert!(
        matches!(r2.final_waiting_for(), WaitingFor::Priority { .. }),
        "Peer Through Depths should resolve cleanly to Priority"
    );

    // 3. Cast Sift Through Sands
    let r3 = runner.cast(sift).commit().resolve();
    println!("After Sift Through Sands: {:?}", r3.final_waiting_for());

    // Hand must contain the 2 drawn cards and prompt DiscardChoice
    let discard_card = match r3.final_waiting_for() {
        WaitingFor::DiscardChoice { cards, count, .. } => {
            assert_eq!(*count, 1);
            cards[0]
        }
        other => panic!("Expected DiscardChoice, got {:?}", other),
    };

    // Act to discard 1 card
    let r4 = runner
        .act(engine::types::actions::GameAction::SelectCards {
            cards: vec![discard_card],
        })
        .unwrap();

    println!("After discard: {:?}", r4.waiting_for);
    // Now both Reach Through Mists and Peer Through Depths were cast this turn!
    // The search ability should be triggered / prompted.
    let search_waiting = match &r4.waiting_for {
        WaitingFor::OptionalEffectChoice { .. } => {
            let res = runner
                .act(engine::types::actions::GameAction::DecideOptionalEffect { accept: true })
                .unwrap();
            res.waiting_for
        }
        other => other.clone(),
    };

    println!("Search waiting state: {:?}", search_waiting);
    match search_waiting {
        WaitingFor::SearchChoice { cards, .. } => {
            assert!(
                cards.contains(&unspeakable_id),
                "Library search should find The Unspeakable"
            );
            let r5 = runner
                .act(engine::types::actions::GameAction::SelectCards {
                    cards: vec![unspeakable_id],
                })
                .unwrap();
            println!("After search select: {:?}", r5.waiting_for);
        }
        other => panic!("Expected SearchChoice, got {:?}", other),
    }

    // Verify The Unspeakable is now on the battlefield!
    assert_eq!(
        runner.state().objects[&unspeakable_id].zone,
        engine::types::zones::Zone::Battlefield,
        "The Unspeakable must be on the battlefield!"
    );
}
