use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::phase::Phase;

const CODE_OF_CONSTRAINT: &str = "Target creature gets -4/-0 until end of turn.\n\
Draw a card.\n\
Addendum — If you cast this spell during your main phase, tap that creature and it doesn't untap during its controller's next untap step.";

#[test]
fn code_of_constraint_cast_during_main_phase_taps_and_locks_untap() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let target_creature = scenario.add_creature(P1, "Storm Crow", 1, 2).id();
    scenario.with_library_top(P0, &["Island", "Island"]);

    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Code of Constraint", true, CODE_OF_CONSTRAINT)
        .id();

    let mut runner = scenario.build();
    assert!(!runner.state().objects[&target_creature].tapped);

    // Cast during main phase (Addendum condition met)
    runner.cast(spell).target_object(target_creature).resolve();

    // The creature should be tapped immediately by the Addendum effect
    assert!(
        runner.state().objects[&target_creature].tapped,
        "creature must be tapped upon spell resolution when cast during main phase"
    );

    // Advance past opponent's untap step into their upkeep
    runner.advance_to_phase(Phase::Upkeep);
    assert_eq!(runner.state().active_player, P1);

    // The creature must NOT untap during its controller's next untap step
    assert!(
        runner.state().objects[&target_creature].tapped,
        "creature must remain tapped during its controller's next untap step"
    );
}

#[test]
fn code_of_constraint_cast_outside_main_phase_does_not_tap_or_lock() {
    let mut scenario = GameScenario::new();
    // BeginCombat is not a main phase
    scenario.at_phase(Phase::BeginCombat);

    let target_creature = scenario.add_creature(P1, "Storm Crow", 1, 2).id();
    scenario.with_library_top(P0, &["Island", "Island"]);

    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Code of Constraint", true, CODE_OF_CONSTRAINT)
        .id();

    let mut runner = scenario.build();
    assert!(!runner.state().objects[&target_creature].tapped);

    // Cast outside main phase (Addendum condition NOT met)
    runner.cast(spell).target_object(target_creature).resolve();

    // Creature should NOT be tapped
    assert!(
        !runner.state().objects[&target_creature].tapped,
        "creature should not be tapped when cast outside main phase"
    );
}
