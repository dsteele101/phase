//! Tests for Electrostatic Bolt:
//! "Electrostatic Bolt deals 2 damage to target creature. If it's an artifact creature,
//! Electrostatic Bolt deals 4 damage to it instead."
//!
//! CR 608.2c + CR 614.1a + CR 614.6:
//! An "instead" clause with target-anaphoric subject "it" referring to a declared target
//! lowers as a conditional sub-ability with `AbilityCondition::ConditionInstead`
//! wrapping `TargetMatchesFilter`. At runtime, `apply_instead_swap` checks the
//! target's characteristics: artifact creatures take 4 damage, non-artifact creatures take 2.

use engine::game::scenario::GameScenario;
use engine::parser::oracle::parse_oracle_text;
use engine::types::ability::{AbilityCondition, Effect, EffectKind, TargetFilter, TypeFilter};
use engine::types::game_state::WaitingFor;
use engine::types::mana::ManaCost;
use engine::types::player::PlayerId;
use engine::types::zones::Zone;
use engine::types::{CoreType, GameEvent};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

const ELECTROSTATIC_BOLT_ORACLE: &str = "Electrostatic Bolt deals 2 damage to target creature. \
    If it's an artifact creature, Electrostatic Bolt deals 4 damage to it instead.";

#[test]
fn electrostatic_bolt_parses_without_unimplemented() {
    let parsed = parse_oracle_text(
        ELECTROSTATIC_BOLT_ORACLE,
        "Electrostatic Bolt",
        &[],
        &["Instant".to_string()],
        &[],
    );

    // No unimplemented fallback nodes
    assert!(
        !parsed
            .abilities
            .iter()
            .any(|a| matches!(&*a.effect, Effect::Unimplemented { .. })),
        "must not contain Effect::Unimplemented in abilities"
    );

    assert_eq!(
        parsed.abilities.len(),
        1,
        "Electrostatic Bolt must lower to a single ability with an instead branch sub-ability"
    );

    let base = &parsed.abilities[0];
    assert!(
        matches!(&*base.effect, Effect::DealDamage { .. }),
        "base effect must be DealDamage, got {:?}",
        base.effect
    );

    let sub = base
        .sub_ability
        .as_ref()
        .expect("the 'deals 4 damage to it instead' must lower as a sub_ability");

    assert!(
        matches!(&*sub.effect, Effect::DealDamage { .. }),
        "sub-ability effect must be DealDamage, got {:?}",
        sub.effect
    );

    let cond = sub
        .condition
        .as_ref()
        .expect("sub-ability must have a condition");

    let AbilityCondition::ConditionInstead { inner } = cond else {
        panic!("expected ConditionInstead, got {cond:?}");
    };

    let AbilityCondition::TargetMatchesFilter {
        filter,
        use_lki,
        subject_slot,
    } = inner.as_ref()
    else {
        panic!("expected TargetMatchesFilter inside ConditionInstead, got {inner:?}");
    };

    assert!(!use_lki, "present-tense 'it's' must evaluate current state");
    assert_eq!(*subject_slot, None);

    let TargetFilter::Typed(tf) = filter else {
        panic!("expected Typed filter, got {filter:?}");
    };

    assert!(
        tf.type_filters.contains(&TypeFilter::Artifact),
        "condition must filter for Artifact"
    );
    assert!(
        tf.type_filters.contains(&TypeFilter::Creature),
        "condition must filter for Creature"
    );
}

#[test]
fn electrostatic_bolt_deals_2_damage_to_non_artifact_creature() {
    let mut scenario = GameScenario::new();
    let target = scenario.add_creature(P1, "Grizzly Bears", 2, 5).id();
    let bolt = scenario
        .add_spell_to_hand_from_oracle(P0, "Electrostatic Bolt", true, ELECTROSTATIC_BOLT_ORACLE)
        .with_mana_cost(ManaCost::generic(0))
        .id();
    let mut runner = scenario.build();

    runner.cast(bolt).target_object(target).resolve();

    assert_eq!(
        runner.state().objects[&target].damage_marked,
        2,
        "Electrostatic Bolt must deal 2 damage to non-artifact creature"
    );
    assert_eq!(
        runner.state().objects[&target].zone,
        Zone::Battlefield,
        "5 toughness creature survives 2 damage"
    );
}

#[test]
fn electrostatic_bolt_deals_4_damage_to_artifact_creature() {
    let mut scenario = GameScenario::new();
    let target = scenario.add_creature(P1, "Steel Wall", 0, 5).id();
    let bolt = scenario
        .add_spell_to_hand_from_oracle(P0, "Electrostatic Bolt", true, ELECTROSTATIC_BOLT_ORACLE)
        .with_mana_cost(ManaCost::generic(0))
        .id();
    let mut runner = scenario.build();

    let obj = runner.state_mut().objects.get_mut(&target).unwrap();
    obj.card_types.core_types.push(CoreType::Artifact);
    obj.base_card_types.core_types.push(CoreType::Artifact);

    runner.cast(bolt).target_object(target).resolve();

    assert_eq!(
        runner.state().objects[&target].damage_marked,
        4,
        "Electrostatic Bolt must deal 4 damage to artifact creature"
    );
    assert_eq!(
        runner.state().objects[&target].zone,
        Zone::Battlefield,
        "5 toughness creature survives 4 damage"
    );
}

#[test]
fn electrostatic_bolt_destroys_4_toughness_artifact_creature_but_not_non_artifact() {
    let mut scenario = GameScenario::new();
    let non_artifact = scenario.add_creature(P1, "Tough Wall", 0, 4).id();
    let artifact = scenario.add_creature(P1, "Iron Wall", 0, 4).id();

    let bolt1 = scenario
        .add_spell_to_hand_from_oracle(P0, "Electrostatic Bolt", true, ELECTROSTATIC_BOLT_ORACLE)
        .with_mana_cost(ManaCost::generic(0))
        .id();
    let bolt2 = scenario
        .add_spell_to_hand_from_oracle(P0, "Electrostatic Bolt", true, ELECTROSTATIC_BOLT_ORACLE)
        .with_mana_cost(ManaCost::generic(0))
        .id();
    let mut runner = scenario.build();

    let obj = runner.state_mut().objects.get_mut(&artifact).unwrap();
    obj.card_types.core_types.push(CoreType::Artifact);
    obj.base_card_types.core_types.push(CoreType::Artifact);

    // Cast 1 on non-artifact 0/4: takes 2 damage, survives
    runner.cast(bolt1).target_object(non_artifact).resolve();
    assert_eq!(
        runner.state().objects[&non_artifact].zone,
        Zone::Battlefield,
        "0/4 non-artifact creature survives 2 damage"
    );
    assert_eq!(runner.state().objects[&non_artifact].damage_marked, 2);

    // Cast 2 on artifact 0/4: takes 4 damage, lethal damage moves it to graveyard
    runner.cast(bolt2).target_object(artifact).resolve();
    assert_eq!(
        runner.state().objects[&artifact].zone,
        Zone::Graveyard,
        "0/4 artifact creature is destroyed by lethal 4 damage"
    );
}

/// CR 608.2c + CR 109.2a + CR 110.4:
/// When an ability contains both a declared target and a revealed/exiled card,
/// conditions qualifying a non-battlefield "card" must retain `RevealedHasCardType`
/// and must never be misrouted to `TargetMatchesFilter`.
#[test]
fn target_plus_revealed_preserves_revealed_card_type() {
    // Hidetsugu and Kairi: exile top card + target opponent + "If it's an instant or sorcery card"
    let hk_parsed = parse_oracle_text(
        "When Hidetsugu and Kairi dies, exile the top card of your library. Target opponent loses \
         life equal to its mana value. If it's an instant or sorcery card, you may cast it without \
         paying its mana cost.",
        "Hidetsugu and Kairi",
        &[],
        &["Creature".to_string()],
        &[],
    );
    let execute = hk_parsed.triggers[0]
        .execute
        .as_deref()
        .expect("trigger has execute");
    let lose_life = execute
        .sub_ability
        .as_deref()
        .expect("exile chains to life loss");
    let free_cast = lose_life
        .sub_ability
        .as_deref()
        .expect("life loss chains to free cast");
    assert_eq!(
        free_cast.condition,
        Some(AbilityCondition::RevealedHasCardType {
            card_types: vec![CoreType::Instant, CoreType::Sorcery],
            additional_filter: None,
            subtype_filter: None,
        }),
        "Hidetsugu and Kairi must retain RevealedHasCardType, not TargetMatchesFilter"
    );

    // Audacious Swap: target nonland permanent + exile top card + "If it's a nonland card"
    let swap_parsed = parse_oracle_text(
        "Exile target nonland permanent. Its controller exiles the top card of their library. \
         If it's a nonland card, they may cast it without paying its mana cost.",
        "Audacious Swap",
        &[],
        &["Sorcery".to_string()],
        &[],
    );
    let exile_target = &swap_parsed.abilities[0];
    let exile_top = exile_target
        .sub_ability
        .as_deref()
        .expect("exile target chains to exile top card");
    let cast_sub = exile_top
        .sub_ability
        .as_deref()
        .expect("exile top chains to cast sub");
    assert!(
        match &cast_sub.condition {
            Some(AbilityCondition::RevealedHasCardType { .. }) => true,
            Some(AbilityCondition::Not { condition }) => {
                matches!(
                    condition.as_ref(),
                    AbilityCondition::RevealedHasCardType { .. }
                )
            }
            _ => false,
        },
        "Audacious Swap must retain RevealedHasCardType (negated for nonland), got {:?}",
        cast_sub.condition
    );
}

/// End-to-end runtime test for Audacious Swap:
/// "Exile target nonland permanent. Its controller exiles the top card of their library.
/// If it's a nonland card, they may cast it without paying its mana cost."
///
/// Distinguishes a revealed nonland card (controller is offered the free cast)
/// from a revealed land card (no cast offer; resolution completes).
#[test]
fn audacious_swap_runtime_distinguishes_nonland_from_land() {
    const AUDACIOUS_SWAP: &str =
        "Exile target nonland permanent. Its controller exiles the top card \
        of their library. If it's a nonland card, they may cast it without paying its mana cost.";

    // Case 1: Nonland card on top of P1's library (Shock).
    // P1 receives an offer to cast it without paying its mana cost.
    {
        let mut scenario = GameScenario::new();
        let target = scenario.add_creature(P1, "Target Bear", 2, 2).id();
        let shock = scenario.add_spell_to_library_top(P1, "Shock", true).id();
        let swap = scenario
            .add_spell_to_hand_from_oracle(P0, "Audacious Swap", true, AUDACIOUS_SWAP)
            .with_mana_cost(ManaCost::generic(0))
            .id();
        let mut runner = scenario.build();
        let outcome = runner
            .cast(swap)
            .target_object(target)
            .accept_optional()
            .resolve();

        assert_eq!(outcome.zone_of(target), Zone::Exile);
        // Nonland card satisfies Not(RevealedHasCardType(Land)); CastFromZone executes,
        // casting Shock from exile onto the stack, where it resolves to graveyard.
        assert_eq!(
            outcome.zone_of(shock),
            Zone::Graveyard,
            "nonland card is cast from exile and resolves to graveyard"
        );
        assert!(
            outcome.events().iter().any(|e| matches!(
                e,
                GameEvent::EffectResolved {
                    kind: EffectKind::CastFromZone,
                    ..
                }
            )),
            "CastFromZone effect must resolve for nonland card"
        );
        assert!(
            outcome.events().iter().any(
                |e| matches!(e, GameEvent::SpellCast { object_id, .. } if *object_id == shock)
            ),
            "Shock must be cast"
        );
    }

    // Case 2: Land card on top of P1's library (Forest).
    // No cast is executed; the land card remains in exile.
    {
        let mut scenario = GameScenario::new();
        let target = scenario.add_creature(P1, "Target Bear", 2, 2).id();
        let land = scenario.add_card_to_library_top(P1, "Forest");
        let swap = scenario
            .add_spell_to_hand_from_oracle(P0, "Audacious Swap", true, AUDACIOUS_SWAP)
            .with_mana_cost(ManaCost::generic(0))
            .id();
        let mut runner = scenario.build();

        let obj = runner.state_mut().objects.get_mut(&land).unwrap();
        obj.card_types.core_types.push(CoreType::Land);
        obj.base_card_types.core_types.push(CoreType::Land);

        let outcome = runner
            .cast(swap)
            .target_object(target)
            .accept_optional()
            .resolve();
        assert_eq!(outcome.zone_of(target), Zone::Exile);
        assert_eq!(
            outcome.zone_of(land),
            Zone::Exile,
            "land card does not satisfy nonland condition and remains in exile"
        );
        assert!(
            !outcome.events().iter().any(|e| matches!(
                e,
                GameEvent::EffectResolved {
                    kind: EffectKind::CastFromZone,
                    ..
                }
            )),
            "CastFromZone must not resolve for land card"
        );
        assert_eq!(
            outcome.final_waiting_for(),
            &WaitingFor::Priority { player: P0 },
            "resolution finishes and priority returns to active player"
        );
    }
}
