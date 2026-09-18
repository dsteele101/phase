use engine::game::layers::evaluate_layers;
use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::mana::{ManaCost, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;
use engine::types::ObjectId;

const ERRATIC_MUTATION_ORACLE: &str = "Choose target creature. Reveal cards from the top of your library until you reveal a nonland card. That creature gets +X/-X until end of turn, where X is that card's mana value. Put all cards revealed this way on the bottom of your library in any order.";

#[test]
fn erratic_mutation_single_target_and_cards_to_bottom() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let victim = scenario.add_creature(P1, "Victim", 2, 5).id();

    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Erratic Mutation", true, ERRATIC_MUTATION_ORACLE)
        .id();

    scenario.with_mana_pool(
        P0,
        vec![
            ManaUnit::new(ManaType::Blue, ObjectId(0), false, vec![]),
            ManaUnit::new(ManaType::Colorless, ObjectId(0), false, vec![]),
            ManaUnit::new(ManaType::Colorless, ObjectId(0), false, vec![]),
        ],
    );

    // Setup library: bottom to top
    // Engine convention: `library[0]` is the top. `add_card_to_library_top` inserts at index 0.
    // So to have Top = Land1, Land2, Nonland (MV 3), Other:
    // We add them in reverse order:
    // 1. Other
    // 2. Nonland (MV 3)
    // 3. Land2
    // 4. Land1
    let other = scenario.add_card_to_library_top(P0, "Deep Card");
    let nonland = scenario
        .add_spell_to_library_top(P0, "Divination", false)
        .with_mana_cost(ManaCost::generic(3))
        .id();
    let land2 = scenario
        .add_spell_to_library_top(P0, "Island", false)
        .as_land()
        .id();
    let land1 = scenario
        .add_spell_to_library_top(P0, "Forest", false)
        .as_land()
        .id();

    let mut runner = scenario.build();

    // Verify library order before cast: [land1, land2, nonland, other]
    {
        let p0_lib = &runner
            .state()
            .players
            .iter()
            .find(|p| p.id == P0)
            .unwrap()
            .library;
        assert_eq!(p0_lib[0], land1);
        assert_eq!(p0_lib[1], land2);
        assert_eq!(p0_lib[2], nonland);
        assert_eq!(p0_lib[3], other);
    }

    let outcome = runner.cast(spell).target_object(victim).resolve();

    // Assertions:
    // 1. Hand did not draw any cards (Issue 2: nonland card does NOT go to hand).
    outcome.assert_hand_drawn(P0, 0);

    // 2. All revealed cards (land1, land2, nonland) are in the library.
    outcome.assert_zone(&[land1, land2, nonland], Zone::Library);

    // CR 701.20a: Check exact library order after "in any order" bottom placement.
    // The hit card (nonland) is bottomed first, then the misses (land1, land2) are
    // placed on bottom in encounter order. Result from top to bottom: [other, nonland, land1, land2].
    {
        let p0_lib = &runner
            .state()
            .players
            .iter()
            .find(|p| p.id == P0)
            .unwrap()
            .library;
        assert_eq!(
            p0_lib.iter().copied().collect::<Vec<_>>(),
            vec![other, nonland, land1, land2],
            "library order must preserve bottom placement of hit then misses in preserve order"
        );
    }

    // 3. Check victim P/T: base is 2/5. With +3/-3 from revealed Divination (MV 3), should be 5/2.
    evaluate_layers(runner.state_mut());
    let victim_obj = &runner.state().objects[&victim];
    assert_eq!(victim_obj.power, Some(5));
    assert_eq!(victim_obj.toughness, Some(2));
}

/// CR 608.2c + CR 616.1: When zone changes during RevealUntil resolution trigger
/// competing replacement effects, the engine pauses on `WaitingFor::ReplacementChoice`.
/// After answering the replacement choice, the batch completion must carry the hit
/// snapshot so the chained pump still resolves against the revealed card's mana value.
#[test]
fn erratic_mutation_replacement_pause_preserves_mana_value_referent() {
    use engine::types::ability::{
        AbilityDefinition, AbilityKind, Effect, ReplacementDefinition, TargetFilter,
    };
    use engine::types::replacements::ReplacementEvent;
    use engine::types::zones::EtbTapState;

    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let victim = scenario.add_creature(P1, "Victim", 2, 5).id();

    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Erratic Mutation", true, ERRATIC_MUTATION_ORACLE)
        .id();

    scenario.with_mana_pool(
        P0,
        vec![
            ManaUnit::new(ManaType::Blue, ObjectId(0), false, vec![]),
            ManaUnit::new(ManaType::Colorless, ObjectId(0), false, vec![]),
            ManaUnit::new(ManaType::Colorless, ObjectId(0), false, vec![]),
        ],
    );

    // Install two competing library-to-exile replacement effects on the battlefield.
    let lib_redirect = |desc: &str| {
        ReplacementDefinition::new(ReplacementEvent::Moved)
            .destination_zone(Zone::Library)
            .execute(AbilityDefinition::new(
                AbilityKind::Spell,
                Effect::ChangeZone {
                    destination: Zone::Exile,
                    origin: None,
                    target: TargetFilter::SelfRef,
                    owner_library: false,
                    enter_transformed: false,
                    enters_under: None,
                    enter_tapped: EtbTapState::Unspecified,
                    enters_attacking: false,
                    up_to: false,
                    enter_with_counters: vec![],
                    conditional_enter_with_counters: vec![],
                    face_down_profile: None,
                    enters_modified_if: None,
                },
            ))
            .description(desc.to_string())
    };

    scenario
        .add_creature(P0, "Redirect A", 1, 1)
        .as_enchantment()
        .with_replacement_definition(lib_redirect("Redirect A"));
    scenario
        .add_creature(P0, "Redirect B", 1, 1)
        .as_enchantment()
        .with_replacement_definition(lib_redirect("Redirect B"));

    let other = scenario.add_card_to_library_top(P0, "Deep Card");
    let nonland = scenario
        .add_spell_to_library_top(P0, "Divination", false)
        .with_mana_cost(ManaCost::generic(3))
        .id();
    let land2 = scenario
        .add_spell_to_library_top(P0, "Island", false)
        .as_land()
        .id();
    let land1 = scenario
        .add_spell_to_library_top(P0, "Forest", false)
        .as_land()
        .id();

    let mut runner = scenario.build();

    // Cast Erratic Mutation targeting victim.
    // Resolution hits competing replacement redirects (Library -> Exile), pauses on
    // ReplacementChoice, answers via policy index 0, and resumes batch completion.
    let outcome = runner
        .cast(spell)
        .target_object(victim)
        .replacement_choice(0)
        .resolve();

    // All revealed cards (land1, land2, nonland) were redirected to Exile by the replacement effect.
    let state = outcome.state();
    assert_eq!(state.objects[&nonland].zone, Zone::Exile);
    assert_eq!(state.objects[&land1].zone, Zone::Exile);
    assert_eq!(state.objects[&land2].zone, Zone::Exile);
    assert_eq!(state.objects[&other].zone, Zone::Library);

    // After resolution, check victim P/T: base 2/5 + 3/-3 (Divination MV 3) -> 5/2.
    let victim_obj = &outcome.state().objects[&victim];
    assert_eq!(
        victim_obj.power,
        Some(5),
        "power must be 5 (2 + 3 from hit card MV across replacement pause)"
    );
    assert_eq!(
        victim_obj.toughness,
        Some(2),
        "toughness must be 2 (5 - 3 from hit card MV across replacement pause)"
    );
}
