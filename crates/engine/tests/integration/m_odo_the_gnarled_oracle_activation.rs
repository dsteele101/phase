//! CR 113.6b + CR 602.5b: M'Odo, the Gnarled Oracle activation restriction integration tests.
//!
//! "Activate this ability only if M'Odo, the Gnarled Oracle is on the battlefield or in the command zone."
//!
//! CR 113.6b: "An ability that can be activated only if the object is in a specific zone or zones functions
//! only when it is in that zone or one of those zones."
//!
//! The ability must be activatable from Zone::Battlefield and Zone::Command,
//! but NOT from Zone::Hand or Zone::Graveyard.

use engine::ai_support::legal_actions;
use engine::game::casting::can_activate_ability_now;
use engine::game::scenario::{GameScenario, P0};
use engine::game::zones::move_to_zone;
use engine::types::actions::GameAction;
use engine::types::mana::{ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const M_ODO_ORACLE: &str = "{X}{B}, Exile a creature card with mana value X from your graveyard: Target opponent loses X life and you gain X life. Activate this ability only if M'Odo, the Gnarled Oracle is on the battlefield or in the command zone.";

fn floating_black(n: usize) -> Vec<ManaUnit> {
    (0..n)
        .map(|_| {
            ManaUnit::new(
                ManaType::Black,
                engine::types::identifiers::ObjectId(0),
                false,
                vec![],
            )
        })
        .collect()
}

#[test]
fn m_odo_activatable_from_battlefield_and_command_zone_but_not_hand_or_graveyard() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let m_odo_id = scenario
        .add_creature_from_oracle(P0, "M'Odo, the Gnarled Oracle", 0, 5, M_ODO_ORACLE)
        .id();
    // Add a creature to graveyard so the cost (exile a creature card from graveyard) can be paid
    scenario.add_creature_to_graveyard(P0, "Grizzly Bears", 2, 2);
    // Add black mana to float
    scenario.with_mana_pool(P0, floating_black(3));

    let mut runner = scenario.build();

    // 1. On Battlefield: can_activate_ability_now should be true
    assert_eq!(runner.state().objects[&m_odo_id].zone, Zone::Battlefield);
    assert!(
        can_activate_ability_now(runner.state(), P0, m_odo_id, 0),
        "M'Odo must be activatable from the battlefield"
    );
    let actions = legal_actions(runner.state());
    assert!(
        actions.iter().any(|a| matches!(
            a,
            GameAction::ActivateAbility { source_id, .. } if *source_id == m_odo_id
        )),
        "ActivateAbility action for M'Odo must be offered when on battlefield"
    );

    // 2. In Command zone: can_activate_ability_now should be true
    {
        let state = runner.state_mut();
        let mut events = Vec::new();
        move_to_zone(state, m_odo_id, Zone::Command, &mut events);
    }
    assert_eq!(runner.state().objects[&m_odo_id].zone, Zone::Command);
    assert!(
        can_activate_ability_now(runner.state(), P0, m_odo_id, 0),
        "M'Odo must be activatable from the command zone per CR 113.6b"
    );
    let actions = legal_actions(runner.state());
    assert!(
        actions.iter().any(|a| matches!(
            a,
            GameAction::ActivateAbility { source_id, .. } if *source_id == m_odo_id
        )),
        "ActivateAbility action for M'Odo must be offered when in command zone"
    );

    // 3. In Hand: can_activate_ability_now should be false
    {
        let state = runner.state_mut();
        let mut events = Vec::new();
        move_to_zone(state, m_odo_id, Zone::Hand, &mut events);
    }
    assert_eq!(runner.state().objects[&m_odo_id].zone, Zone::Hand);
    assert!(
        !can_activate_ability_now(runner.state(), P0, m_odo_id, 0),
        "M'Odo must NOT be activatable from hand"
    );
    let actions = legal_actions(runner.state());
    assert!(
        !actions.iter().any(|a| matches!(
            a,
            GameAction::ActivateAbility { source_id, .. } if *source_id == m_odo_id
        )),
        "ActivateAbility action for M'Odo must not be offered when in hand"
    );

    // 4. In Graveyard: can_activate_ability_now should be false
    {
        let state = runner.state_mut();
        let mut events = Vec::new();
        move_to_zone(state, m_odo_id, Zone::Graveyard, &mut events);
    }
    assert_eq!(runner.state().objects[&m_odo_id].zone, Zone::Graveyard);
    assert!(
        !can_activate_ability_now(runner.state(), P0, m_odo_id, 0),
        "M'Odo must NOT be activatable from graveyard"
    );
    let actions = legal_actions(runner.state());
    assert!(
        !actions.iter().any(|a| matches!(
            a,
            GameAction::ActivateAbility { source_id, .. } if *source_id == m_odo_id
        )),
        "ActivateAbility action for M'Odo must not be offered when in graveyard"
    );
}
