use engine::ai_support::legal_actions;
use engine::game::game_object::AttachTarget;
use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::actions::GameAction;

const LEONIN_BOLA: &str =
    "Equipped creature has \"{T}, Unattach Leonin Bola: Tap target creature.\"\nEquip {1}";

#[test]
fn leonin_bola_granted_ability_on_creature_with_activated_ability() {
    let mut scenario = GameScenario::new();

    // Create target creature for opponent
    let target = scenario.add_creature(P1, "Bear", 2, 2);
    let target_id = target.id();

    // Create host creature with an intrinsic activated ability ({T}: Deal 1 damage to any target)
    let host = scenario.add_creature_from_oracle(
        P0,
        "Prodigal Pyromancer",
        1,
        1,
        "{T}: ~ deals 1 damage to any target.",
    );
    let host_id = host.id();

    // Create Leonin Bola Equipment
    let mut bola = scenario.add_artifact_from_oracle(P0, "Leonin Bola", LEONIN_BOLA);
    bola.with_subtypes(vec!["Equipment"]);
    let bola_id = bola.id();

    let mut runner = scenario.build();

    // Attach Leonin Bola to host
    {
        let bola_obj = runner.state_mut().objects.get_mut(&bola_id).unwrap();
        bola_obj.attached_to = Some(AttachTarget::Object(host_id));
        let host_obj = runner.state_mut().objects.get_mut(&host_id).unwrap();
        host_obj.attachments.push(bola_id);
    }

    // Reapply layers so the static ability on Leonin Bola grants the ability to host
    engine::game::layers::mark_layers_full(runner.state_mut());
    engine::game::layers::flush_layers(runner.state_mut());

    // Host should now have 2 abilities:
    // 0: {T}: ~ deals 1 damage to any target.
    // 1: {T}, Unattach Leonin Bola: Tap target creature.
    let host_obj = runner.state().objects.get(&host_id).unwrap();
    assert_eq!(host_obj.abilities.len(), 2);
    let can_activate =
        engine::game::casting::can_activate_ability_now(runner.state(), P0, host_id, 1);
    assert!(
        can_activate,
        "Leonin Bola's granted ability should be activatable"
    );

    // Get legal actions
    let legal = legal_actions(runner.state());
    let host_activations: Vec<_> = legal
        .iter()
        .filter_map(|action| match action {
            GameAction::ActivateAbility {
                source_id,
                ability_index,
            } if *source_id == host_id => Some(*ability_index),
            _ => None,
        })
        .collect();

    // Both ability 0 and ability 1 should be legally activatable
    assert!(
        host_activations.contains(&0),
        "host's intrinsic ability 0 should be activatable: {host_activations:?}"
    );
    assert!(
        host_activations.contains(&1),
        "Leonin Bola's granted ability 1 should be activatable: {host_activations:?}"
    );

    // Target creature should initially be untapped
    assert!(!runner.state().objects.get(&target_id).unwrap().tapped);

    // Activate Leonin Bola's granted ability (index 1) targeting opponent's creature
    runner
        .activate(host_id, 1)
        .target_object(target_id)
        .resolve();

    // After activation, cost is paid: host is tapped, Leonin Bola is unattached
    assert!(
        runner.state().objects.get(&host_id).unwrap().tapped,
        "host should be tapped as cost"
    );
    assert!(
        runner
            .state()
            .objects
            .get(&bola_id)
            .unwrap()
            .attached_to
            .is_none(),
        "Leonin Bola should be unattached as cost"
    );
    assert!(
        !runner
            .state()
            .objects
            .get(&host_id)
            .unwrap()
            .attachments
            .contains(&bola_id),
        "host attachments should no longer contain Leonin Bola"
    );

    // Ability resolved: target creature is now tapped!
    assert!(
        runner.state().objects.get(&target_id).unwrap().tapped,
        "target creature should be tapped by resolved ability"
    );
}
