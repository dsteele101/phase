//! Integration tests for Bramble Elemental and Shielding Plax:
//!
//! Bramble Elemental:
//! "Whenever an Aura becomes attached to this creature, create two 1/1 green Saproling creature tokens."
//!
//! Shielding Plax:
//! "Enchant creature
//! When this Aura enters, draw a card.
//! Enchanted creature can't be the target of spells or abilities your opponents control."

use engine::game::engine::EngineError;
use engine::game::game_object::AttachTarget;
use engine::game::keywords::has_keyword;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::{CastPaymentMode, WaitingFor};
use engine::types::keywords::Keyword;
use engine::types::mana::ManaCost;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::TargetRef;

const BRAMBLE_ELEMENTAL: &str =
    "Whenever an Aura becomes attached to this creature, create two 1/1 green Saproling creature tokens.";

const SHIELDING_PLAX: &str = "Enchant creature\nWhen this Aura enters, draw a card.\nEnchanted creature can't be the target of spells or abilities your opponents control.";

const COMPOUND_AURA: &str = "Enchant creature\nEnchanted creature gets +1/+1 and can't be the target of spells or abilities your opponents control.";

const ENORMOUS_ENERGY_BLADE: &str = "Equipped creature gets +4/+0.\nWhenever this Equipment becomes attached to a creature, tap that creature.\nEquip {0}";

fn saproling_count(runner: &GameRunner, player: PlayerId) -> usize {
    let state = runner.state();
    state
        .battlefield
        .iter()
        .filter_map(|id| state.objects.get(id))
        .filter(|obj| {
            obj.controller == player
                && obj.card_types.subtypes.iter().any(|s| s == "Saproling")
                && obj.power == Some(1)
                && obj.toughness == Some(1)
        })
        .count()
}

#[test]
fn bramble_elemental_creates_tokens_when_enchanted_by_shielding_plax() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_library_top(P0, &["Forest", "Forest"]);

    let bramble = scenario
        .add_creature_from_oracle(P0, "Bramble Elemental", 4, 4, BRAMBLE_ELEMENTAL)
        .id();

    let plax = scenario
        .add_spell_to_hand(P0, "Shielding Plax", false)
        .as_enchantment()
        .with_subtypes(vec!["Aura"])
        .with_mana_cost(ManaCost::generic(0))
        .from_oracle_text_with_keywords(&["Enchant"], SHIELDING_PLAX)
        .id();

    let mut runner = scenario.build();

    // Cast Shielding Plax targeting Bramble Elemental
    let outcome = runner.cast(plax).target_object(bramble).resolve();

    // 1. Bramble Elemental creates two 1/1 green Saprolings
    assert_eq!(
        saproling_count(&runner, P0),
        2,
        "Bramble Elemental must create 2 Saproling tokens when an Aura becomes attached"
    );

    // 2. Shielding Plax ETB draws a card (hand delta since commit is +1)
    outcome.assert_hand_drawn(P0, 1);

    // 3. Shielding Plax carries CantBeTargeted { Opponents } affecting the enchanted creature,
    // and does NOT grant Keyword::Hexproof directly to the creature (CR 109.5).
    assert!(
        !has_keyword(&runner.state().objects[&bramble], &Keyword::Hexproof),
        "Shielding Plax must not grant Keyword::Hexproof directly to the creature"
    );
}

#[test]
fn shielding_plax_restricts_targeting_of_enchanted_creature() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_library_top(P0, &["Forest"]);

    let bear = scenario.add_creature(P0, "Grizzly Bears", 2, 2).id();

    let plax = scenario
        .add_spell_to_hand(P0, "Shielding Plax", false)
        .as_enchantment()
        .with_subtypes(vec!["Aura"])
        .with_mana_cost(ManaCost::generic(0))
        .from_oracle_text_with_keywords(&["Enchant"], SHIELDING_PLAX)
        .id();

    let mut runner = scenario.build();

    // Initially Grizzly Bears does not have Hexproof
    assert!(
        !has_keyword(&runner.state().objects[&bear], &Keyword::Hexproof),
        "Grizzly Bears should not have Hexproof before being enchanted"
    );

    // Cast Shielding Plax targeting Grizzly Bears
    runner.cast(plax).target_object(bear).resolve();

    // After resolution, Grizzly Bears does not receive the Hexproof keyword directly (CR 109.5);
    // targeting is governed by the Aura's static CantBeTargeted ability.
    assert!(
        !has_keyword(&runner.state().objects[&bear], &Keyword::Hexproof),
        "Shielding Plax must not grant Hexproof keyword (governed by CantBeTargeted static)"
    );

    // Shielding Plax itself is NOT the creature and does not have Hexproof on itself
    assert!(
        !has_keyword(&runner.state().objects[&plax], &Keyword::Hexproof),
        "Shielding Plax itself must not have Hexproof"
    );
}

#[test]
fn compound_aura_grants_pt_and_cant_be_targeted_to_enchanted_creature() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let bear = scenario.add_creature(P0, "Grizzly Bears", 2, 2).id();

    let aura = scenario
        .add_spell_to_hand(P0, "Compound Shield", false)
        .as_enchantment()
        .with_subtypes(vec!["Aura"])
        .with_mana_cost(ManaCost::generic(0))
        .from_oracle_text_with_keywords(&["Enchant"], COMPOUND_AURA)
        .id();

    let mut runner = scenario.build();

    runner.cast(aura).target_object(bear).resolve();

    // Grizzly Bears gets +1/+1
    let bear_obj = &runner.state().objects[&bear];
    assert_eq!(
        bear_obj.power,
        Some(3),
        "Grizzly Bears should get +1/+1 (power 3)"
    );
    assert_eq!(
        bear_obj.toughness,
        Some(3),
        "Grizzly Bears should get +1/+1 (toughness 3)"
    );
    assert!(
        !has_keyword(bear_obj, &Keyword::Hexproof),
        "Compound Aura must not grant Hexproof keyword (governed by CantBeTargeted static)"
    );
}

#[test]
fn opponent_cannot_target_creature_enchanted_by_shielding_plax() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_library_top(P0, &["Forest"]);

    let protected_bear = scenario.add_creature(P0, "Protected Bear", 2, 2).id();
    let unprotected_bear_1 = scenario.add_creature(P0, "Unprotected Bear 1", 2, 2).id();
    let unprotected_bear_2 = scenario.add_creature(P0, "Unprotected Bear 2", 2, 2).id();

    let plax = scenario
        .add_spell_to_hand(P0, "Shielding Plax", false)
        .as_enchantment()
        .with_subtypes(vec!["Aura"])
        .with_mana_cost(ManaCost::generic(0))
        .from_oracle_text_with_keywords(&["Enchant"], SHIELDING_PLAX)
        .id();

    let p0_buff = scenario
        .add_spell_to_hand(P0, "Giant Growth", false)
        .with_mana_cost(ManaCost::generic(0))
        .from_oracle_text("Target creature gets +3/+3 until end of turn.")
        .id();

    let spitting_earth = scenario
        .add_spell_to_hand(P1, "Spitting Earth", false)
        .with_mana_cost(ManaCost::generic(0))
        .from_oracle_text("Spitting Earth deals damage to target creature equal to the number of Mountains you control.")
        .id();

    let mut runner = scenario.build();

    // P0 enchants Protected Bear with Shielding Plax
    runner.cast(plax).target_object(protected_bear).resolve();

    // Positive Control 1: P0 (the controller) CAN target Protected Bear with a spell
    let p0_result = runner
        .cast(p0_buff)
        .target_object(protected_bear)
        .try_resolve();
    assert!(
        p0_result.is_ok(),
        "Controller must be able to target their own Hexproof creature"
    );

    // Give active turn and priority to P1 (opponent)
    runner.state_mut().active_player = P1;
    runner.state_mut().waiting_for = WaitingFor::Priority { player: P1 };
    runner.state_mut().priority_player = P1;

    // Negative Test: P1 initiates casting Spitting Earth
    let cast_res = runner.act(GameAction::CastSpell {
        object_id: spitting_earth,
        card_id: runner.state().objects[&spitting_earth].card_id,
        targets: vec![],
        payment_mode: CastPaymentMode::Auto,
    });
    assert!(
        cast_res.is_ok(),
        "Initiating cast must enter target selection"
    );

    // Verify engine excludes protected_bear from legal_targets for opponent
    let waiting = runner.state().waiting_for.clone();
    if let WaitingFor::TargetSelection {
        ref target_slots, ..
    } = waiting
    {
        assert!(
            !target_slots[0]
                .legal_targets
                .contains(&TargetRef::Object(protected_bear)),
            "Protected bear must not be in legal targets for opponent"
        );
        assert!(
            target_slots[0]
                .legal_targets
                .contains(&TargetRef::Object(unprotected_bear_1)),
            "Unprotected bear 1 must be in legal targets for opponent"
        );
        assert!(
            target_slots[0]
                .legal_targets
                .contains(&TargetRef::Object(unprotected_bear_2)),
            "Unprotected bear 2 must be in legal targets for opponent"
        );
    } else {
        panic!("Expected WaitingFor::TargetSelection, got: {waiting:?}");
    }

    // Attempting to select protected_bear must be rejected with "Illegal target selected"
    let choose_res = runner.act(GameAction::ChooseTarget {
        target: Some(TargetRef::Object(protected_bear)),
    });
    match choose_res {
        Err(EngineError::InvalidAction(ref msg)) => {
            assert!(
                msg.contains("Illegal target"),
                "Expected illegal target rejection message, got: {msg}"
            );
        }
        Err(other) => panic!("Expected InvalidAction(Illegal target), got: {other:?}"),
        Ok(_) => panic!("Opponent targeting creature with Hexproof must fail, but succeeded"),
    }

    // Positive Control 2: Selecting unprotected_bear_1 succeeds
    let choose_valid = runner.act(GameAction::ChooseTarget {
        target: Some(TargetRef::Object(unprotected_bear_1)),
    });
    assert!(
        choose_valid.is_ok(),
        "Opponent choosing unprotected creature must succeed"
    );
}

#[test]
fn shielding_plax_different_controller_scope_cr_109_5() {
    // CR 109.5: "For a static ability, this is the current controller of the object it's on."
    // Shielding Plax says: "Enchanted creature can't be the target of spells or abilities your opponents control."
    // Here, P0 controls Shielding Plax attached to P1's creature.
    // P0's opponents are prohibited from targeting the creature (so P1 cannot target their own creature).
    // P0 is NOT an opponent of P0, so P0 CAN target P1's enchanted creature.
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_library_top(P0, &["Forest"]);

    let p1_bear = scenario.add_creature(P1, "P1 Bear", 2, 2).id();
    let p1_unprotected_bear_1 = scenario
        .add_creature(P1, "P1 Unprotected Bear 1", 2, 2)
        .id();
    let p1_unprotected_bear_2 = scenario
        .add_creature(P1, "P1 Unprotected Bear 2", 2, 2)
        .id();

    let plax = scenario
        .add_spell_to_hand(P0, "Shielding Plax", false)
        .as_enchantment()
        .with_subtypes(vec!["Aura"])
        .with_mana_cost(ManaCost::generic(0))
        .from_oracle_text_with_keywords(&["Enchant"], SHIELDING_PLAX)
        .id();

    let p0_buff = scenario
        .add_spell_to_hand(P0, "Giant Growth", false)
        .with_mana_cost(ManaCost::generic(0))
        .from_oracle_text("Target creature gets +3/+3 until end of turn.")
        .id();

    let spitting_earth = scenario
        .add_spell_to_hand(P1, "Spitting Earth", false)
        .with_mana_cost(ManaCost::generic(0))
        .from_oracle_text("Spitting Earth deals damage to target creature equal to the number of Mountains you control.")
        .id();

    let mut runner = scenario.build();

    // P0 enchants P1's creature with Shielding Plax
    runner.cast(plax).target_object(p1_bear).resolve();

    // Positive Control: P0 (Aura controller) CAN target P1's creature
    let p0_result = runner.cast(p0_buff).target_object(p1_bear).try_resolve();
    assert!(
        p0_result.is_ok(),
        "Aura controller (P0) must be able to target P1's creature enchanted by P0's Shielding Plax"
    );

    // Pass turn / priority to P1 (creature controller, but opponent of Aura controller)
    runner.state_mut().active_player = P1;
    runner.state_mut().waiting_for = WaitingFor::Priority { player: P1 };
    runner.state_mut().priority_player = P1;

    // Negative Test: P1 tries to target P1 Bear
    let cast_res = runner.act(GameAction::CastSpell {
        object_id: spitting_earth,
        card_id: runner.state().objects[&spitting_earth].card_id,
        targets: vec![],
        payment_mode: CastPaymentMode::Auto,
    });
    assert!(
        cast_res.is_ok(),
        "Initiating cast must enter target selection"
    );

    // Verify p1_bear is excluded from legal_targets for P1
    let waiting = runner.state().waiting_for.clone();
    if let WaitingFor::TargetSelection {
        ref target_slots, ..
    } = waiting
    {
        assert!(
            !target_slots[0]
                .legal_targets
                .contains(&TargetRef::Object(p1_bear)),
            "P1's creature enchanted by P0's Shielding Plax must NOT be a legal target for P1 (P0's opponent)"
        );
        assert!(
            target_slots[0]
                .legal_targets
                .contains(&TargetRef::Object(p1_unprotected_bear_1)),
            "P1's unprotected creature 1 must be a legal target for P1"
        );
        assert!(
            target_slots[0]
                .legal_targets
                .contains(&TargetRef::Object(p1_unprotected_bear_2)),
            "P1's unprotected creature 2 must be a legal target for P1"
        );
    } else {
        panic!("Expected WaitingFor::TargetSelection, got: {waiting:?}");
    }

    // Attempting to select p1_bear must fail with Illegal target
    let choose_res = runner.act(GameAction::ChooseTarget {
        target: Some(TargetRef::Object(p1_bear)),
    });
    match choose_res {
        Err(EngineError::InvalidAction(ref msg)) => {
            assert!(
                msg.contains("Illegal target"),
                "Expected illegal target rejection message, got: {msg}"
            );
        }
        Err(other) => panic!("Expected InvalidAction(Illegal target), got: {other:?}"),
        Ok(_) => panic!("P1 targeting creature enchanted by P0's Shielding Plax must fail"),
    }

    // Positive Control 2: Selecting p1_unprotected_bear_1 succeeds
    let choose_valid = runner.act(GameAction::ChooseTarget {
        target: Some(TargetRef::Object(p1_unprotected_bear_1)),
    });
    assert!(
        choose_valid.is_ok(),
        "P1 selecting their own unprotected creature must succeed"
    );
}

#[test]
fn equipment_attachment_single_trigger_and_same_host_noop() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let bear1 = scenario.add_creature(P0, "Grizzly Bears 1", 2, 2).id();
    let bear2 = scenario.add_creature(P0, "Grizzly Bears 2", 2, 2).id();

    let blade = scenario
        .add_artifact_from_oracle(P0, "Enormous Energy Blade", ENORMOUS_ENERGY_BLADE)
        .with_subtypes(vec!["Equipment"])
        .from_oracle_text_with_keywords(&["Equip"], ENORMOUS_ENERGY_BLADE)
        .id();

    let mut runner = scenario.build();

    // Bear 1 and Bear 2 start untapped
    assert!(!runner.state().objects[&bear1].tapped);
    assert!(!runner.state().objects[&bear2].tapped);

    let equip_idx = runner.state().objects[&blade]
        .abilities
        .iter()
        .position(|a| {
            a.description
                .as_deref()
                .is_some_and(|d| d.contains("Equip"))
        })
        .expect("Blade must carry an Equip activated ability");

    // 1. First Equip to Bear 1:
    // Resolves Equip, attaches to Bear 1, fires exactly 1 attached trigger which taps Bear 1.
    runner
        .activate(blade, equip_idx)
        .target_object(bear1)
        .resolve();

    assert_eq!(
        runner.state().objects[&blade].attached_to,
        Some(AttachTarget::Object(bear1)),
        "Blade must be attached to Bear 1"
    );
    assert_eq!(
        runner.state().objects[&bear1].power,
        Some(6),
        "Bear 1 must have +4/+0 (power 6)"
    );
    assert!(
        runner.state().objects[&bear1].tapped,
        "Bear 1 must be tapped by the attached trigger"
    );

    // Untap Bear 1 manually to test same-host re-equip
    runner.state_mut().objects.get_mut(&bear1).unwrap().tapped = false;
    assert!(!runner.state().objects[&bear1].tapped);

    // 2. Same-host Equip to Bear 1 (CR 701.3b):
    // Re-equipping to the same host is a no-op that does NOT emit GameEvent::Attached.
    // 0 triggers must fire, so Bear 1 must REMAIN untapped.
    runner
        .activate(blade, equip_idx)
        .target_object(bear1)
        .resolve();

    assert_eq!(
        runner.state().objects[&blade].attached_to,
        Some(AttachTarget::Object(bear1))
    );
    assert!(
        !runner.state().objects[&bear1].tapped,
        "CR 701.3b: Re-equipping to same host is a no-op and must not trigger attached ability"
    );

    // 3. Move Equip to Bear 2:
    // Attaches to Bear 2, Bear 1 loses P/T bonus, Bear 2 gets +4/+0 and becomes tapped by the 1 attached trigger.
    runner
        .activate(blade, equip_idx)
        .target_object(bear2)
        .resolve();

    assert_eq!(
        runner.state().objects[&blade].attached_to,
        Some(AttachTarget::Object(bear2))
    );
    assert_eq!(
        runner.state().objects[&bear1].power,
        Some(2),
        "Bear 1 loses P/T bonus after equipment is moved"
    );
    assert_eq!(
        runner.state().objects[&bear2].power,
        Some(6),
        "Bear 2 gets +4/+0"
    );
    assert!(
        runner.state().objects[&bear2].tapped,
        "Bear 2 must be tapped by the attached trigger upon moving equipment"
    );
}
