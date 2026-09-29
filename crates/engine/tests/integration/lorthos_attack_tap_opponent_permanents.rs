//! Integration test for Lorthos, Tentacled Terror / "Whenever ~ attacks, tap all permanents your opponent controls."
//! CR 109.4 + CR 701.26a: Only permanents controlled by opponents are tapped by the ability.

use engine::game::combat::AttackTarget;
use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::phase::Phase;

const LORTHOS_TEXT: &str = "Whenever Lorthos attacks, tap all permanents your opponent controls.";

#[test]
fn lorthos_attack_taps_only_opponent_permanents() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let lorthos = scenario
        .add_creature_from_oracle(P0, "Lorthos, Tentacled Terror", 8, 8, LORTHOS_TEXT)
        .id();
    let p0_creature = scenario
        .add_creature_from_oracle(P0, "P0 Ally", 2, 2, "")
        .id();
    let p0_land = scenario
        .add_land_from_oracle(P0, "Island", "{T}: Add {U}.")
        .id();

    let p1_creature = scenario
        .add_creature_from_oracle(P1, "P1 Defender", 2, 2, "")
        .id();
    let p1_land = scenario
        .add_land_from_oracle(P1, "Swamp", "{T}: Add {B}.")
        .id();

    let mut runner = scenario.build();

    // Verify initial states (all untapped)
    assert!(!runner.state().objects.get(&p0_creature).unwrap().tapped);
    assert!(!runner.state().objects.get(&p0_land).unwrap().tapped);
    assert!(!runner.state().objects.get(&p1_creature).unwrap().tapped);
    assert!(!runner.state().objects.get(&p1_land).unwrap().tapped);

    runner.advance_to_combat();
    runner
        .declare_attackers(&[(lorthos, AttackTarget::Player(P1))])
        .expect("declaring Lorthos as attacker must succeed");

    // Pass priority until trigger resolves
    for _ in 0..40 {
        match runner.state().waiting_for.clone() {
            WaitingFor::Priority { .. } => {
                if runner.state().stack.is_empty() || runner.act(GameAction::PassPriority).is_err()
                {
                    break;
                }
            }
            _ => break,
        }
    }

    // P1's permanents must be tapped
    assert!(
        runner.state().objects.get(&p1_creature).unwrap().tapped,
        "P1 creature should be tapped by Lorthos attack trigger"
    );
    assert!(
        runner.state().objects.get(&p1_land).unwrap().tapped,
        "P1 land should be tapped by Lorthos attack trigger"
    );

    // P0's other permanents must remain UNTAPPED
    assert!(
        !runner.state().objects.get(&p0_creature).unwrap().tapped,
        "P0 creature should remain untapped"
    );
    assert!(
        !runner.state().objects.get(&p0_land).unwrap().tapped,
        "P0 land should remain untapped"
    );
}
