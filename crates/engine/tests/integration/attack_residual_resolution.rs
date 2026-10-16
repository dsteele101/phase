//! `combat_damage::AttackResidual` — the whole attack's projected damage to one
//! defending player — against real engine combat.
//!
//! Every fixture declares its attack and blocks through `apply`, and the
//! ledger's `Exact` figures are checked against the life the engine actually
//! takes. Cards are built from verbatim Oracle text (Scryfall, checked
//! 2026-10-15); a body labelled "synthetic" is not a printed card.

use engine::game::combat::AttackTarget;
use engine::game::combat_damage::{AttackResidual, AttackerResidual, ResidualTotal};
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;

const HEARTHFIRE_HOBGOBLIN: &str = "Double strike";
const FOG_BANK: &str = "Defender (This creature can't attack.)\nFlying\nPrevent all combat damage that would be dealt to and dealt by this creature.";
const FURNACE_OF_RATH: &str = "If a source would deal damage to a permanent or player, it deals double that damage to that permanent or player instead.";
const HEALING_SALVE: &str = "Choose one —\n• Target player gains 3 life.\n• Prevent the next 3 damage that would be dealt to any target this turn.";

fn new_scenario(defender_life: i32) -> GameScenario {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_life(P1, defender_life);
    for player in [P0, P1] {
        scenario.with_library_top(player, &["A", "B", "C", "D", "E"]);
    }
    scenario
}

fn hobgoblin(scenario: &mut GameScenario) -> ObjectId {
    scenario
        .add_creature(P0, "Hearthfire Hobgoblin", 2, 2)
        .from_oracle_text_with_keywords(&["Double strike"], HEARTHFIRE_HOBGOBLIN)
        .id()
}

fn kobolds(scenario: &mut GameScenario) -> ObjectId {
    scenario.add_creature(P1, "Crimson Kobolds", 0, 1).id()
}

fn to_declare_blockers(runner: &mut GameRunner, attackers: &[ObjectId]) {
    runner.pass_both_players();
    runner
        .act(GameAction::DeclareAttackers {
            attacks: attackers
                .iter()
                .map(|&attacker| (attacker, AttackTarget::Player(P1)))
                .collect(),
            bands: vec![],
        })
        .expect("DeclareAttackers should succeed");
    if matches!(runner.state().waiting_for, WaitingFor::Priority { .. }) {
        runner.pass_both_players();
    }
    assert!(matches!(
        runner.state().waiting_for,
        WaitingFor::DeclareBlockers { .. }
    ));
}

/// Declare `blocks`, then run combat damage to its end. Returns the life the
/// defending player lost.
fn resolve_combat(runner: &mut GameRunner, blocks: &[(ObjectId, ObjectId)]) -> i32 {
    let life_before = runner.life(P1);
    runner
        .act(GameAction::DeclareBlockers {
            assignments: blocks.to_vec(),
        })
        .expect("DeclareBlockers should succeed");
    for _ in 0..128 {
        if matches!(runner.state().phase, Phase::EndCombat | Phase::PostCombatMain) {
            break;
        }
        match runner.state().waiting_for.clone() {
            WaitingFor::Priority { .. } => {
                runner
                    .act(GameAction::PassPriority)
                    .expect("passing priority should succeed");
            }
            WaitingFor::OrderTriggers { .. } => {
                engine::game::triggers::drain_order_triggers_with_identity(runner.state_mut());
            }
            WaitingFor::GameOver { .. } => break,
            other => panic!("unexpected prompt during combat damage: {other:?}"),
        }
    }
    life_before - runner.life(P1)
}

/// CR 510.1b + CR 702.4b: five unblocked 2/2 double strikers (Hearthfire
/// Hobgoblin) deal 20. Each Crimson Kobolds block of one prevents exactly its 4
/// (CR 510.1c), so three blocks leave 8 < 9 — and the engine takes exactly 8.
/// The negative control: one block leaves 16, which kills a player at 9.
#[test]
fn the_ledger_tracks_the_whole_attack_against_real_combat() {
    for blocks_committed in [3usize, 1] {
        let mut scenario = new_scenario(9);
        let attackers: Vec<ObjectId> = (0..5).map(|_| hobgoblin(&mut scenario)).collect();
        let walls: Vec<ObjectId> = (0..5).map(|_| kobolds(&mut scenario)).collect();
        let mut runner = scenario.build();
        to_declare_blockers(&mut runner, &attackers);

        let mut ledger = AttackResidual::project_attack(runner.state(), P1, &attackers, &[]);
        assert_eq!(
            ledger.total(),
            ResidualTotal {
                exact: 20,
                at_most: 0,
                no_credit: 0
            }
        );
        for (attacker, wall) in attackers.iter().zip(&walls).take(blocks_committed) {
            assert_eq!(ledger.prevented_by(runner.state(), *attacker, &[*wall]), 4);
            ledger.commit(runner.state(), *attacker, &[*wall]);
        }
        let projected = 20 - 4 * blocks_committed as i32;
        assert_eq!(ledger.total().exact, projected);
        assert_eq!(ledger.total().lethality(), projected);

        let blocks: Vec<(ObjectId, ObjectId)> = attackers
            .iter()
            .zip(&walls)
            .take(blocks_committed)
            .map(|(attacker, wall)| (*wall, *attacker))
            .collect();
        let lost = resolve_combat(&mut runner, &blocks);
        assert_eq!(lost, projected, "the engine takes exactly the ledger's figure");
        if blocks_committed == 1 {
            assert!(runner.life(P1) <= 0, "negative control: one block is not enough");
        } else {
            assert_eq!(runner.life(P1), 1);
        }
    }
}

/// A trampler behind a Fog Bank block is a barrier (`NoCredit`): the block
/// earns no prevention credit, and the lethality total counts the attacker at
/// its unblocked value. The exact sibling — a vanilla 0/2 block — prevents.
#[test]
fn barriered_blocks_earn_no_prevention_credit() {
    let mut scenario = new_scenario(20);
    let mut builder = scenario.add_creature(P0, "Synthetic Flying Trampler", 5, 5);
    builder.flying().trample();
    let trampler = builder.id();
    let fog = scenario
        .add_creature(P1, "Fog Bank", 0, 2)
        .from_oracle_text_with_keywords(&["Defender", "Flying"], FOG_BANK)
        .id();
    let mut wall_builder = scenario.add_creature(P1, "Synthetic Flying Wall", 0, 2);
    wall_builder.flying();
    let wall = wall_builder.id();
    let mut runner = scenario.build();
    to_declare_blockers(&mut runner, &[trampler]);

    let mut ledger = AttackResidual::project_attack(runner.state(), P1, &[trampler], &[]);
    assert_eq!(ledger.residual(trampler), Some(AttackerResidual::Exact(5)));
    assert_eq!(ledger.prevented_by(runner.state(), trampler, &[fog]), 0);
    assert_eq!(
        ledger.prevented_by(runner.state(), trampler, &[wall]),
        2,
        "exact sibling: 5 − 3 tramples over the 0/2"
    );

    ledger.commit(runner.state(), trampler, &[fog]);
    assert_eq!(ledger.residual(trampler), Some(AttackerResidual::NoCredit(5)));
    assert_eq!(
        ledger.total(),
        ResidualTotal {
            exact: 0,
            at_most: 0,
            no_credit: 5
        }
    );
    assert_eq!(ledger.total().lethality(), 5);
}

/// TEXT-2a: an unblocked attacker whose damage to the defending player would be
/// amplified (Furnace of Rath) is `NoCredit` at its unblocked value.
#[test]
fn an_amplified_unblocked_attacker_is_no_credit() {
    let mut scenario = new_scenario(20);
    let bears = scenario.add_creature(P0, "Grizzly Bears", 2, 2).id();
    scenario.add_enchantment_from_oracle(P1, "Furnace of Rath", FURNACE_OF_RATH);
    // A potential blocker, so the declare blockers step is not skipped.
    scenario.add_creature(P1, "Memnite", 1, 1);
    let mut runner = scenario.build();
    to_declare_blockers(&mut runner, &[bears]);
    let ledger = AttackResidual::project_attack(runner.state(), P1, &[bears], &[]);
    assert_eq!(ledger.residual(bears), Some(AttackerResidual::NoCredit(2)));
    let lost = resolve_combat(&mut runner, &[]);
    assert_eq!(lost, 4, "engine-measured: Furnace doubles the 2");
}

/// A reductive shield confined to the defending player is `AtMost`; a block of
/// an `AtMost` attacker earns no prevention credit (the committed figure is not
/// exact).
#[test]
fn an_at_most_attacker_earns_no_prevention_credit() {
    let mut scenario = new_scenario(20);
    let bears = scenario.add_creature(P0, "Grizzly Bears", 2, 2).id();
    let salve = scenario
        .add_spell_to_hand_from_oracle(P0, "Healing Salve", true, HEALING_SALVE)
        .id();
    let wall = scenario.add_creature(P1, "Synthetic Wall", 0, 4).id();
    let mut runner = scenario.build();
    runner.cast(salve).modes(&[1]).target_player(P1).resolve();
    to_declare_blockers(&mut runner, &[bears]);

    let ledger = AttackResidual::project_attack(runner.state(), P1, &[bears], &[]);
    assert_eq!(ledger.residual(bears), Some(AttackerResidual::AtMost(2)));
    assert_eq!(ledger.prevented_by(runner.state(), bears, &[wall]), 0);
    assert_eq!(
        ledger.with_blockers(runner.state(), bears, &[wall]),
        AttackerResidual::Exact(0),
        "reach guard: the candidate block itself projects exactly"
    );
}

/// Attackers `state.combat` records as attacking another player are excluded
/// from this defender's ledger.
#[test]
fn attackers_aimed_elsewhere_are_excluded() {
    let mut scenario = GameScenario::new_n_player(3, 42);
    scenario.at_phase(Phase::PreCombatMain);
    for player in [P0, P1, engine::types::player::PlayerId(2)] {
        scenario.with_library_top(player, &["A", "B", "C", "D", "E"]);
    }
    let at_p1 = scenario.add_creature(P0, "Grizzly Bears", 2, 2).id();
    let at_p2 = scenario.add_creature(P0, "Grizzly Bears", 2, 2).id();
    let mut runner = scenario.build();
    for _ in 0..8 {
        if matches!(
            runner.state().waiting_for,
            WaitingFor::DeclareAttackers { .. }
        ) {
            break;
        }
        runner
            .act(GameAction::PassPriority)
            .expect("passing priority should succeed");
    }
    runner
        .act(GameAction::DeclareAttackers {
            attacks: vec![
                (at_p1, AttackTarget::Player(P1)),
                (at_p2, AttackTarget::Player(engine::types::player::PlayerId(2))),
            ],
            bands: vec![],
        })
        .expect("DeclareAttackers should succeed");
    assert!(
        runner.state().combat.is_some(),
        "reach guard: combat records each attack target"
    );
    let ledger = AttackResidual::project_attack(runner.state(), P1, &[at_p1, at_p2], &[]);
    assert_eq!(ledger.residual(at_p1), Some(AttackerResidual::Exact(2)));
    assert_eq!(ledger.residual(at_p2), None);
    assert_eq!(ledger.total().exact, 2);
}
