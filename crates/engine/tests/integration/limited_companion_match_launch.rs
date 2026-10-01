use engine::game::companion::{
    can_activate_companion, check_companion_reveal, handle_declare_companion,
};
use engine::game::deck_loading::{load_deck_into_state, DeckEntry, DeckPayload, PlayerDeckPayload};
use engine::game::scenario::GameRunner;
use engine::types::actions::GameAction;
use engine::types::card::CardFace;
use engine::types::card_type::{CardType, CoreType};
use engine::types::events::GameEvent;
use engine::types::format::FormatConfig;
use engine::types::game_state::{
    CompanionChoiceSource, CompanionDeclaration, CompanionRevealChoice, GameState, WaitingFor,
};
use engine::types::keywords::{CompanionCondition, Keyword};
use engine::types::mana::{ManaCost, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;
use engine::types::{ObjectId, PlayerId};

const P0: PlayerId = PlayerId(0);

fn creature(name: &str, mv: u32) -> DeckEntry {
    DeckEntry {
        card: CardFace {
            name: name.to_string(),
            mana_cost: ManaCost::Cost {
                shards: vec![],
                generic: mv,
            },
            card_type: CardType {
                supertypes: vec![],
                core_types: vec![CoreType::Creature],
                subtypes: vec!["Human".to_string()],
            },
            ..Default::default()
        },
        count: 1,
    }
}

fn land(name: &str, count: u32) -> DeckEntry {
    DeckEntry {
        card: CardFace {
            name: name.to_string(),
            mana_cost: ManaCost::NoCost,
            card_type: CardType {
                supertypes: vec![],
                core_types: vec![CoreType::Land],
                subtypes: vec!["Plains".to_string()],
            },
            ..Default::default()
        },
        count,
    }
}

fn lurrus_companion() -> DeckEntry {
    DeckEntry {
        card: CardFace {
            name: "Lurrus of the Dream-Den".to_string(),
            mana_cost: ManaCost::Cost {
                shards: vec![],
                generic: 3,
            },
            card_type: CardType {
                supertypes: vec![],
                core_types: vec![CoreType::Creature],
                subtypes: vec!["Cat".to_string(), "Nightmare".to_string()],
            },
            keywords: vec![Keyword::Companion(
                CompanionCondition::MaxPermanentManaValue(2),
            )],
            ..Default::default()
        },
        count: 1,
    }
}

/// CR 702.139a: Limited matches support a designated companion loaded via
/// `PlayerDeckPayload.companion`. The companion is offered during pregame reveal,
/// is declared and moved to active companion status, and can be put to hand
/// for {3} special action at sorcery speed.
#[test]
fn limited_companion_match_launch_hydrates_dedicated_companion_and_allows_reveal_and_payment() {
    let mut state = GameState::new(FormatConfig::limited(), 2, 42);

    let mut main_deck: Vec<DeckEntry> = Vec::new();
    // 23 creatures with mana value <= 2
    for i in 0..23 {
        main_deck.push(creature(&format!("Small Creature {i}"), 2));
    }
    // 17 lands
    main_deck.push(land("Plains", 17));

    let companion_entry = lurrus_companion();

    let payload = DeckPayload {
        player: PlayerDeckPayload {
            main_deck: main_deck.clone(),
            companion: vec![companion_entry.clone()],
            ..Default::default()
        },
        opponent: PlayerDeckPayload {
            main_deck,
            ..Default::default()
        },
        ..Default::default()
    };

    load_deck_into_state(&mut state, &payload);

    // Verify deck pool loaded the dedicated companion for Limited format
    let p0_pool = state
        .deck_pools
        .iter()
        .find(|pool| pool.player == P0)
        .expect("P0 pool must exist");
    assert_eq!(p0_pool.registered_companion.len(), 1);
    assert_eq!(p0_pool.current_companion.len(), 1);
    assert_eq!(
        p0_pool.current_companion[0].card.name,
        "Lurrus of the Dream-Den"
    );

    // Pregame companion check must offer Lurrus from Dedicated source
    let wf = check_companion_reveal(&state, P0)
        .expect("P0 must have a companion offer for legal Lurrus deck in Limited");

    let eligible = match &wf {
        WaitingFor::CompanionReveal {
            player,
            eligible_companions,
        } => {
            assert_eq!(*player, P0);
            eligible_companions.clone()
        }
        other => panic!("expected CompanionReveal waiting state, got {other:?}"),
    };

    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].name, "Lurrus of the Dream-Den");
    assert_eq!(eligible[0].source, CompanionChoiceSource::Dedicated);

    state.waiting_for = wf;

    // Reveal companion
    let mut events = Vec::new();
    let choice = CompanionDeclaration::Reveal(CompanionRevealChoice {
        name: "Lurrus of the Dream-Den".to_string(),
        source: CompanionChoiceSource::Dedicated,
    });
    let _next_wf = handle_declare_companion(&mut state, P0, choice, &mut events)
        .expect("valid dedicated companion reveal must succeed");

    // Companion must now be assigned to P0
    assert!(state.players[P0.0 as usize]
        .companion
        .as_ref()
        .is_some_and(|c| c.card.card.name == "Lurrus of the Dream-Den" && !c.used));

    // Current companion pool is cleared after reveal
    let p0_pool_after = state.deck_pools.iter().find(|p| p.player == P0).unwrap();
    assert_eq!(p0_pool_after.current_companion.len(), 0);

    // Setup precombat main phase with 3 mana in pool to pay {3}
    state.phase = Phase::PreCombatMain;
    state.waiting_for = WaitingFor::Priority { player: P0 };
    state.priority_player = P0;
    state.active_player = P0;

    for _ in 0..3 {
        state.players[P0.0 as usize].mana_pool.add(ManaUnit::new(
            ManaType::White,
            ObjectId(999),
            false,
            vec![],
        ));
    }

    assert!(
        can_activate_companion(&state, P0),
        "P0 must be able to activate companion special action with 3 mana in pool"
    );

    let mut runner = GameRunner::from_state(state);
    let outcome = runner
        .act(GameAction::CompanionToHand)
        .expect("companion special action payment must succeed");

    assert!(runner.state().players[P0.0 as usize]
        .companion
        .as_ref()
        .is_some_and(|c| c.used));

    assert!(runner.state().objects.values().any(|obj| {
        obj.owner == P0 && obj.zone == Zone::Hand && obj.name == "Lurrus of the Dream-Den"
    }));

    assert!(outcome.events.iter().any(|event| matches!(
        event,
        GameEvent::CompanionMovedToHand { player, card_name }
            if *player == P0 && card_name == "Lurrus of the Dream-Den"
    )));
}

/// CR 702.139a: If a Limited deck violates the companion's restriction (e.g.
/// Lurrus with a 3+ mana value permanent in the deck), the companion is not offered.
#[test]
fn limited_companion_match_launch_rejects_invalid_companion_condition() {
    let mut state = GameState::new(FormatConfig::limited(), 2, 42);

    let mut main_deck: Vec<DeckEntry> = Vec::new();
    // 22 creatures with mana value <= 2
    for i in 0..22 {
        main_deck.push(creature(&format!("Small Creature {i}"), 2));
    }
    // 1 creature with mana value 3 (violates Lurrus restriction!)
    main_deck.push(creature("Expensive Creature", 3));
    // 17 lands
    main_deck.push(land("Plains", 17));

    let companion_entry = lurrus_companion();

    let payload = DeckPayload {
        player: PlayerDeckPayload {
            main_deck: main_deck.clone(),
            companion: vec![companion_entry],
            ..Default::default()
        },
        opponent: PlayerDeckPayload {
            main_deck,
            ..Default::default()
        },
        ..Default::default()
    };

    load_deck_into_state(&mut state, &payload);

    // Companion check must NOT offer Lurrus because deck has a 3-drop permanent
    let wf = check_companion_reveal(&state, P0);
    assert!(
        wf.is_none(),
        "invalid companion deck must produce no companion reveal offer"
    );
}

/// CR 702.139a: An unbacked companion declaration (e.g. declaring a companion not offered)
/// is rejected by `handle_declare_companion`.
#[test]
fn limited_companion_match_launch_rejects_unbacked_companion_selection() {
    let mut state = GameState::new(FormatConfig::limited(), 2, 42);

    let mut main_deck: Vec<DeckEntry> = Vec::new();
    for i in 0..23 {
        main_deck.push(creature(&format!("Small Creature {i}"), 2));
    }
    main_deck.push(land("Plains", 17));

    let payload = DeckPayload {
        player: PlayerDeckPayload {
            main_deck: main_deck.clone(),
            companion: vec![], // No companion registered!
            ..Default::default()
        },
        opponent: PlayerDeckPayload {
            main_deck,
            ..Default::default()
        },
        ..Default::default()
    };

    load_deck_into_state(&mut state, &payload);

    state.waiting_for = WaitingFor::CompanionReveal {
        player: P0,
        eligible_companions: vec![],
    };

    let mut events = Vec::new();
    let forged_choice = CompanionDeclaration::Reveal(CompanionRevealChoice {
        name: "Lurrus of the Dream-Den".to_string(),
        source: CompanionChoiceSource::Dedicated,
    });

    let result = handle_declare_companion(&mut state, P0, forged_choice, &mut events);
    assert!(
        result.is_err(),
        "unoffered/unbacked companion declaration must be rejected"
    );
}

/// Test that state with declared companion cleanly serializes/deserializes (replay/reload).
#[test]
fn limited_companion_state_roundtrip_preserves_companion_declaration() {
    let mut state = GameState::new(FormatConfig::limited(), 2, 42);
    state.players[P0.0 as usize].companion = Some(engine::types::player::CompanionInfo {
        card: lurrus_companion(),
        used: false,
    });

    let serialized = serde_json::to_string(&state).expect("state must serialize");
    let restored: GameState = serde_json::from_str(&serialized).expect("state must deserialize");

    assert!(restored.players[P0.0 as usize]
        .companion
        .as_ref()
        .is_some_and(|c| c.card.card.name == "Lurrus of the Dream-Den" && !c.used));
}
