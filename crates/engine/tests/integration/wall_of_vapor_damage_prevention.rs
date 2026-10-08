//! Combat relation damage prevention and trigger integration tests.
//!
//! CR 615.1a (damage prevention), CR 509.1g (creatures it's blocking / creatures blocking it),
//! CR 603.10a (LTB / dies combat history look-back), CR 608.2h / CR 113.7a (LKI target revalidation),
//! CR 301.5a (Aura/Equipment combat relation host rebinding).

use engine::game::combat::AttackTarget;
use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const WALL_OF_VAPOR_ORACLE: &str =
    "Defender (This creature can't attack.)\nPrevent all damage that would be dealt to this creature by creatures it's blocking.";
const DRY_SPELL_ORACLE: &str = "Dry Spell deals 1 damage to each creature and each player.";
const PRODIGAL_SORCERER_ORACLE: &str =
    "Vigilance\n{T}: Prodigal Sorcerer deals 1 damage to any target.";

#[test]
fn wall_of_vapor_takes_damage_from_spell() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let wall = scenario
        .add_creature_from_oracle(P1, "Wall of Vapor", 0, 1, WALL_OF_VAPOR_ORACLE)
        .id();

    let dry_spell = scenario
        .add_spell_to_hand_from_oracle(
            P0,
            "Dry Spell",
            /* is_instant */ false,
            DRY_SPELL_ORACLE,
        )
        .id();

    let mut runner = scenario.build();

    let outcome = runner.cast(dry_spell).resolve();

    // Wall of Vapor took 1 damage from Dry Spell (damage not prevented)
    // Since toughness is 1, 1 lethal damage causes it to die to state-based actions.
    outcome.assert_zone(&[wall], Zone::Graveyard);
}

#[test]
fn wall_of_vapor_prevents_combat_damage_from_blocked_creature() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let wall = scenario
        .add_creature_from_oracle(P1, "Wall of Vapor", 0, 1, WALL_OF_VAPOR_ORACLE)
        .id();
    let attacker = scenario.add_creature(P0, "Attacker", 2, 2).id();

    let mut runner = scenario.build();

    // Advance to DeclareAttackers
    runner.pass_both_players();
    runner
        .act(GameAction::DeclareAttackers {
            attacks: vec![(attacker, AttackTarget::Player(P1))],
            bands: vec![],
        })
        .expect("DeclareAttackers should succeed");

    // Pass priority to DeclareBlockers
    runner.pass_both_players();
    runner
        .act(GameAction::DeclareBlockers {
            assignments: vec![(wall, attacker)],
        })
        .expect("DeclareBlockers should succeed");

    // Pass priority through DeclareBlockers and combat damage
    runner.pass_both_players();

    // Combat damage was dealt. Wall of Vapor blocked the 2/2 attacker, so all damage
    // dealt to it by the blocked creature was prevented.
    // Wall of Vapor survives on the battlefield with 0 damage marked.
    assert_eq!(
        runner.state().objects.get(&wall).unwrap().damage_marked,
        0,
        "Combat damage from blocked attacker must be prevented"
    );
    assert_eq!(
        runner.state().objects.get(&wall).unwrap().zone,
        Zone::Battlefield,
        "Wall of Vapor must survive combat"
    );
}

#[test]
fn wall_of_vapor_prevents_noncombat_damage_from_blocked_creature() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let wall = scenario
        .add_creature_from_oracle(P1, "Wall of Vapor", 0, 1, WALL_OF_VAPOR_ORACLE)
        .id();
    let pinger = scenario
        .add_creature_from_oracle(P0, "Prodigal Sorcerer", 1, 1, PRODIGAL_SORCERER_ORACLE)
        .id();

    let mut runner = scenario.build();

    // Advance to DeclareAttackers
    runner.pass_both_players();
    runner
        .act(GameAction::DeclareAttackers {
            attacks: vec![(pinger, AttackTarget::Player(P1))],
            bands: vec![],
        })
        .expect("DeclareAttackers should succeed");

    // Pass priority to DeclareBlockers
    runner.pass_both_players();
    runner
        .act(GameAction::DeclareBlockers {
            assignments: vec![(wall, pinger)],
        })
        .expect("DeclareBlockers should succeed");

    // In DeclareBlockers step, P0 activates Prodigal Sorcerer targeting Wall of Vapor.
    runner.activate(pinger, 0).target_object(wall).resolve();

    // CR 615.1a: Wall of Vapor prevents all damage from creatures it's blocking,
    // including non-combat damage from an activated ability.
    assert_eq!(
        runner.state().objects.get(&wall).unwrap().damage_marked,
        0,
        "Noncombat damage from blocked creature must be prevented"
    );
    assert_eq!(
        runner.state().objects.get(&wall).unwrap().zone,
        Zone::Battlefield,
        "Wall of Vapor must survive noncombat damage from blocked creature"
    );
}

#[test]
fn wall_of_vapor_does_not_prevent_damage_from_unblocked_creature() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let wall = scenario
        .add_creature_from_oracle(P1, "Wall of Vapor", 0, 1, WALL_OF_VAPOR_ORACLE)
        .id();
    let bear = scenario.add_creature(P1, "Grizzly Bears", 2, 2).id();
    let pinger = scenario
        .add_creature_from_oracle(P0, "Prodigal Sorcerer", 1, 1, PRODIGAL_SORCERER_ORACLE)
        .id();

    let mut runner = scenario.build();

    // Advance to DeclareAttackers
    runner.pass_both_players();
    runner
        .act(GameAction::DeclareAttackers {
            attacks: vec![(pinger, AttackTarget::Player(P1))],
            bands: vec![],
        })
        .expect("DeclareAttackers should succeed");

    // Bear blocks pinger; Wall of Vapor does NOT block pinger.
    runner.pass_both_players();
    runner
        .act(GameAction::DeclareBlockers {
            assignments: vec![(bear, pinger)],
        })
        .expect("DeclareBlockers should succeed");

    // P0 activates Prodigal Sorcerer targeting Wall of Vapor.
    runner.activate(pinger, 0).target_object(wall).resolve();

    // Wall of Vapor is not blocking Prodigal Sorcerer, so damage is NOT prevented.
    // 1 lethal damage causes it to die to state-based actions.
    assert_eq!(
        runner.state().objects.get(&wall).unwrap().zone,
        Zone::Graveyard,
        "Damage from creature Wall of Vapor is not blocking must NOT be prevented"
    );
}

#[test]
fn armored_transport_prevents_combat_damage_from_blocking_creatures() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let transport = scenario
        .add_creature_from_oracle(
            P0,
            "Armored Transport",
            2,
            1,
            "Prevent all combat damage that would be dealt to this creature by creatures blocking it.",
        )
        .id();
    let blocker = scenario.add_creature(P1, "Grizzly Bears", 2, 2).id();

    let mut runner = scenario.build();

    // Advance to DeclareAttackers
    runner.pass_both_players();
    runner
        .act(GameAction::DeclareAttackers {
            attacks: vec![(transport, AttackTarget::Player(P1))],
            bands: vec![],
        })
        .expect("DeclareAttackers should succeed");

    // Blocker blocks Armored Transport
    runner.pass_both_players();
    runner
        .act(GameAction::DeclareBlockers {
            assignments: vec![(blocker, transport)],
        })
        .expect("DeclareBlockers should succeed");

    // Pass through combat damage step
    runner.pass_both_players();

    // CR 615.1a + CR 509.1g: Armored Transport takes 0 damage (prevented) and survives.
    assert_eq!(
        runner
            .state()
            .objects
            .get(&transport)
            .unwrap()
            .damage_marked,
        0,
        "Combat damage from blocking creature must be prevented"
    );
    assert_eq!(
        runner.state().objects.get(&transport).unwrap().zone,
        Zone::Battlefield,
        "Armored Transport must survive combat"
    );
    // Blocker took 2 combat damage from Armored Transport and died
    assert_eq!(
        runner.state().objects.get(&blocker).unwrap().zone,
        Zone::Graveyard,
        "Blocker must be destroyed by combat damage"
    );
}

#[test]
fn wall_of_corpses_destroys_blocked_creature_with_lki() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_mana_pool(
        P1,
        vec![ManaUnit::new(
            ManaType::Black,
            ObjectId(9_999),
            false,
            vec![],
        )],
    );

    let attacker = scenario.add_creature(P0, "Hill Giant", 3, 3).id();
    let corpses = scenario
        .add_creature_from_oracle(
            P1,
            "Wall of Corpses",
            0,
            2,
            "Defender (This creature can't attack.)\n{B}, Sacrifice this creature: Destroy target creature this creature is blocking.",
        )
        .id();

    let mut runner = scenario.build();

    // Advance to DeclareAttackers
    runner.pass_both_players();
    runner
        .act(GameAction::DeclareAttackers {
            attacks: vec![(attacker, AttackTarget::Player(P1))],
            bands: vec![],
        })
        .expect("DeclareAttackers should succeed");

    // Wall of Corpses blocks attacker
    runner.pass_both_players();
    runner
        .act(GameAction::DeclareBlockers {
            assignments: vec![(corpses, attacker)],
        })
        .expect("DeclareBlockers should succeed");

    // P0 passes priority in DeclareBlockers
    runner
        .act(GameAction::PassPriority)
        .expect("P0 passes priority");

    // CR 106.4: Step/phase transitions clear mana pools, so fund P1's pool directly in DeclareBlockers
    runner.state_mut().players[P1.0 as usize]
        .mana_pool
        .add(ManaUnit::new(
            ManaType::Black,
            ObjectId(9_999),
            false,
            vec![],
        ));

    // P1 activates Wall of Corpses sacrificing itself (ability index 1, following Defender keyword)
    runner
        .activate(corpses, 1)
        .pay_with(&[corpses])
        .target_object(attacker)
        .resolve();

    // Wall of Corpses was sacrificed
    assert_eq!(
        runner.state().objects.get(&corpses).unwrap().zone,
        Zone::Graveyard,
        "Wall of Corpses must be in graveyard after sacrifice cost"
    );

    // CR 113.7a / CR 608.2h: Attacker was destroyed by Wall of Corpses ability via LKI combat relation
    assert_eq!(
        runner.state().objects.get(&attacker).unwrap().zone,
        Zone::Graveyard,
        "Attacker must be destroyed by Wall of Corpses' activated ability"
    );
}

#[test]
fn baneclaw_marauder_triggers_when_blocking_creature_dies() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let marauder = scenario
        .add_creature_from_oracle(
            P0,
            "Baneclaw Marauder",
            3,
            4,
            "Whenever this creature becomes blocked, each creature blocking it gets -1/-1 until end of turn.\nWhenever a creature blocking this creature dies, that creature's controller loses 1 life.",
        )
        .id();
    let blocker = scenario.add_creature(P1, "Llanowar Elves", 1, 1).id();

    let mut runner = scenario.build();

    // Advance to DeclareAttackers
    runner.pass_both_players();
    runner
        .act(GameAction::DeclareAttackers {
            attacks: vec![(marauder, AttackTarget::Player(P1))],
            bands: vec![],
        })
        .expect("DeclareAttackers should succeed");

    // Blocker blocks Marauder
    runner.pass_both_players();
    runner
        .act(GameAction::DeclareBlockers {
            assignments: vec![(blocker, marauder)],
        })
        .expect("DeclareBlockers should succeed");

    // Drain stack: BecomesBlocked trigger resolves (-1/-1), blocker dies to SBAs,
    // dies trigger fires and resolves (P1 loses 1 life)
    while !runner.state().stack.is_empty() {
        runner.pass_both_players();
    }

    assert_eq!(
        runner.state().objects.get(&blocker).unwrap().zone,
        Zone::Graveyard,
        "Blocker must have died from -1/-1"
    );
    assert_eq!(
        runner.state().players[P1.0 as usize].life,
        19,
        "P1 must have lost 1 life from Baneclaw Marauder's dies trigger"
    );
}

#[test]
fn trailblazers_torch_deals_damage_to_blocking_creatures() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let attacker = scenario.add_creature(P0, "Attacker", 2, 2).id();
    let torch = scenario
        .add_artifact_from_oracle(
            P0,
            "Trailblazer's Torch",
            "Whenever equipped creature becomes blocked, it deals 2 damage to each creature blocking it.",
        )
        .with_subtypes(vec!["Equipment"])
        .id();

    let blocker = scenario.add_creature(P1, "Grizzly Bears", 2, 2).id();

    let mut runner = scenario.build();

    // Attach Torch to attacker
    runner
        .state_mut()
        .objects
        .get_mut(&torch)
        .unwrap()
        .attached_to = Some(engine::game::game_object::AttachTarget::Object(attacker));
    runner
        .state_mut()
        .objects
        .get_mut(&attacker)
        .unwrap()
        .attachments
        .push(torch);
    engine::game::trigger_index::reindex_object_triggers(runner.state_mut(), torch);

    // Advance to DeclareAttackers
    runner.pass_both_players();
    runner
        .act(GameAction::DeclareAttackers {
            attacks: vec![(attacker, AttackTarget::Player(P1))],
            bands: vec![],
        })
        .expect("DeclareAttackers should succeed");

    // Blocker blocks attacker
    runner.pass_both_players();
    runner
        .act(GameAction::DeclareBlockers {
            assignments: vec![(blocker, attacker)],
        })
        .expect("DeclareBlockers should succeed");

    // Drain stack: BecomesBlocked trigger fires and deals 2 damage to blocker
    while !runner.state().stack.is_empty() {
        runner.pass_both_players();
    }

    assert_eq!(
        runner.state().objects.get(&blocker).unwrap().zone,
        Zone::Graveyard,
        "Blocker must be destroyed by 2 damage from Trailblazer's Torch trigger"
    );
}
