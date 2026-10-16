//! Whole-exchange combat projection (`combat_damage::ProjectedExchange`) against
//! real engine resolution.
//!
//! Every fixture is driven through the real pipeline (`apply`): attackers and
//! blockers are declared with `GameAction`s, block-declaration triggers go on
//! the stack and resolve, and combat damage is assigned through the engine's own
//! `AssignCombatDamage` prompt using the division the projection realized. The
//! projection is computed from the state the defending player sees at
//! `WaitingFor::DeclareBlockers`, before anything is declared.
//!
//! Cards are built from their verbatim Oracle text (Scryfall, checked
//! 2026-10-15) through the live parser. A body labelled "synthetic" is not a
//! printed card; it exists only to isolate one rules axis.

use std::collections::HashSet;

use engine::game::combat::AttackTarget;
use engine::game::combat_damage::{
    combat_damage_to_defender, combat_damage_to_defender_checked, Basis, BlockProvenance,
    DamageEventClass, KillPlan, PreventionRecipient, ProjectedExchange, Projection,
    ProjectionBarrier, RealizedPlan,
};
use engine::game::effects::attach::attach_to;
use engine::game::layers::evaluate_layers;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::ability::{
    AbilityDefinition, AbilityKind, Effect, PtValue, ReplacementDefinition, TargetFilter,
    TriggerDefinition,
};
use engine::types::actions::GameAction;
use engine::types::events::GameEvent;
use engine::types::game_state::{
    CombatDamageAssignmentMode, CombatDamageSubStep, DerivedIndexReadiness, GameState, WaitingFor,
};
use engine::types::identifiers::ObjectId;
use engine::types::keywords::Keyword;
use engine::types::mana::ManaColor;
use engine::types::phase::Phase;
use engine::types::replacements::ReplacementEvent;
use engine::types::triggers::TriggerMode;
use engine::types::zones::Zone;

const SUQATA_LANCER: &str = "Haste\nFlanking (Whenever a creature without flanking blocks this creature, the blocking creature gets -1/-1 until end of turn.)";
const BENALISH_CAVALRY: &str = "Flanking (Whenever a creature without flanking blocks this creature, the blocking creature gets -1/-1 until end of turn.)";
const KNIGHT_OF_SURSI: &str = "Flying; flanking (Whenever a creature without flanking blocks this creature, the blocking creature gets -1/-1 until end of turn.)\nSuspend 3—{W} (Rather than cast this card from your hand, you may pay {W} and exile it with three time counters on it. At the beginning of your upkeep, remove a time counter. When the last is removed, you may cast it without paying its mana cost. It has haste.)";
const DEVOTED_RETAINER: &str = "Bushido 1 (Whenever this creature blocks or becomes blocked, it gets +1/+1 until end of turn.)";
const WOLVERINE_PACK: &str = "Rampage 2 (Whenever this creature becomes blocked, it gets +2/+2 until end of turn for each creature blocking it beyond the first.)";
const SOMBERWALD_ALPHA: &str = "Whenever a creature you control becomes blocked, it gets +1/+1 until end of turn.\n{1}{G}: Target creature you control gains trample until end of turn. (It can deal excess combat damage to the player or planeswalker it's attacking.)";
const AUDACITY: &str = "Enchant creature\nEnchanted creature gets +2/+0 and has trample. (It can deal excess combat damage to the player or planeswalker it's attacking.)\nWhen this Aura is put into a graveyard from the battlefield, draw a card.";
const WRECKING_OGRE: &str = "Double strike\nBloodrush — {3}{R}{R}, Discard this card: Target attacking creature gets +3/+3 and gains double strike until end of turn.";
const BENALISH_KNIGHT: &str = "Flash (You may cast this spell any time you could cast an instant.)\nFirst strike (This creature deals combat damage before creatures without first strike.)";
const HEALING_SALVE: &str = "Choose one —\n• Target player gains 3 life.\n• Prevent the next 3 damage that would be dealt to any target this turn.";
const FOG_BANK: &str = "Defender (This creature can't attack.)\nFlying\nPrevent all combat damage that would be dealt to and dealt by this creature.";
const FURNACE_OF_RATH: &str = "If a source would deal damage to a permanent or player, it deals double that damage to that permanent or player instead.";
const ALABORN_ZEALOT: &str = "When this creature blocks a creature, destroy both creatures.";
const PSYCHIC_MEMBRANE: &str =
    "Defender (This creature can't attack.)\nWhenever this creature blocks, you may draw a card.";
const WHITE_KNIGHT: &str = "First strike (This creature deals combat damage before creatures without first strike.)\nProtection from black (This creature can't be blocked, targeted, dealt damage, or enchanted by anything black.)";
const RUSTRAZOR_BUTCHER: &str =
    "First strike\nWither (This deals damage to creatures in the form of -1/-1 counters.)";
const RAZOR_SWINE: &str = "First strike\nInfect (This creature deals damage to creatures in the form of -1/-1 counters and to players in the form of poison counters.)";

// --- Fixture builders --------------------------------------------------------

fn new_scenario() -> GameScenario {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    for player in [P0, P1] {
        scenario.with_library_top(player, &["A", "B", "C", "D", "E"]);
    }
    scenario
}

fn lancer(scenario: &mut GameScenario) -> ObjectId {
    scenario
        .add_creature(P0, "Suq'Ata Lancer", 2, 2)
        .from_oracle_text_with_keywords(&["Haste", "Flanking"], SUQATA_LANCER)
        .id()
}

fn cavalry(scenario: &mut GameScenario, player: engine::types::player::PlayerId) -> ObjectId {
    scenario
        .add_creature(player, "Benalish Cavalry", 2, 2)
        .from_oracle_text_with_keywords(&["Flanking"], BENALISH_CAVALRY)
        .id()
}

fn retainer(scenario: &mut GameScenario, player: engine::types::player::PlayerId) -> ObjectId {
    scenario
        .add_creature(player, "Devoted Retainer", 1, 1)
        .from_oracle_text_with_keywords(&["Bushido"], DEVOTED_RETAINER)
        .id()
}

fn ogre(scenario: &mut GameScenario) -> ObjectId {
    scenario
        .add_creature(P0, "Wrecking Ogre", 3, 3)
        .from_oracle_text_with_keywords(&["Double strike", "Bloodrush"], WRECKING_OGRE)
        .id()
}

fn vanilla(
    scenario: &mut GameScenario,
    player: engine::types::player::PlayerId,
    name: &str,
    power: i32,
    toughness: i32,
) -> ObjectId {
    scenario.add_creature(player, name, power, toughness).id()
}

fn audacity_on(runner: &mut GameRunner, aura: ObjectId, host: ObjectId) {
    attach_to(runner.state_mut(), aura, host);
    evaluate_layers(runner.state_mut());
    assert!(
        runner.state().objects[&host].has_keyword(&Keyword::Trample),
        "reach guard: Audacity grants trample"
    );
}

// --- Combat drivers ----------------------------------------------------------

/// From precombat main to the defending player's `DeclareBlockers` prompt.
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
    assert!(
        matches!(
            runner.state().waiting_for,
            WaitingFor::DeclareBlockers { .. }
        ),
        "expected the defending player's block declaration, got {:?}",
        runner.state().waiting_for
    );
}

fn declare_blockers(runner: &mut GameRunner, blocks: &[(ObjectId, ObjectId)]) {
    runner
        .act(GameAction::DeclareBlockers {
            assignments: blocks.to_vec(),
        })
        .expect("DeclareBlockers should succeed");
}

/// Resolve everything the block declaration put on the stack, stopping at the
/// declare blockers step's empty-stack priority window.
fn resolve_block_triggers(runner: &mut GameRunner) {
    for _ in 0..64 {
        match runner.state().waiting_for {
            WaitingFor::OrderTriggers { .. } => {
                engine::game::triggers::drain_order_triggers_with_identity(runner.state_mut());
            }
            WaitingFor::Priority { .. } if !runner.state().stack.is_empty() => {
                runner
                    .act(GameAction::PassPriority)
                    .expect("passing priority should succeed");
            }
            _ => return,
        }
    }
}

/// The current damage step of an `AssignCombatDamage` prompt.
fn current_step(state: &GameState) -> CombatDamageSubStep {
    let combat = state.combat.as_ref().expect("combat in progress");
    if !combat.first_strike_done
        && combat
            .first_strike_participants
            .as_ref()
            .is_some_and(|participants| !participants.is_empty())
    {
        CombatDamageSubStep::FirstStrike
    } else {
        CombatDamageSubStep::Regular
    }
}

/// Drive combat damage to the end of combat, answering each assignment prompt
/// with `assign(step)` → `(to blockers, to defender)`. Returns the life the
/// defending player lost and whether `probe` had marked damage or counters
/// after the first-strike step.
fn finish_combat(
    runner: &mut GameRunner,
    mut assign: impl FnMut(CombatDamageSubStep) -> (Vec<(ObjectId, u32)>, u32),
) -> i32 {
    let life_before = runner.life(P1);
    for _ in 0..128 {
        if matches!(runner.state().phase, Phase::EndCombat | Phase::PostCombatMain) {
            break;
        }
        match runner.state().waiting_for.clone() {
            WaitingFor::OrderTriggers { .. } => {
                engine::game::triggers::drain_order_triggers_with_identity(runner.state_mut());
            }
            WaitingFor::Priority { .. } => {
                runner
                    .act(GameAction::PassPriority)
                    .expect("passing priority should succeed");
            }
            WaitingFor::AssignCombatDamage { .. } => {
                let (assignments, trample_damage) = assign(current_step(runner.state()));
                runner
                    .act(GameAction::AssignCombatDamage {
                        mode: CombatDamageAssignmentMode::Normal,
                        assignments,
                        trample_damage,
                        controller_damage: 0,
                    })
                    .expect("the projected division must be a legal assignment");
            }
            WaitingFor::GameOver { .. } => break,
            other => panic!("unexpected prompt during combat damage: {other:?}"),
        }
    }
    life_before - runner.life(P1)
}

/// Answer assignment prompts with a realized plan's per-step division.
fn assign_from(
    realized: &RealizedPlan,
) -> impl FnMut(CombatDamageSubStep) -> (Vec<(ObjectId, u32)>, u32) + '_ {
    move |step| {
        let allocation = realized
            .steps
            .iter()
            .find(|allocation| allocation.step == step)
            .expect("the realized plan covers every step the engine prompts for");
        (allocation.to_blockers.clone(), allocation.to_defender)
    }
}

fn no_prompt_expected(_: CombatDamageSubStep) -> (Vec<(ObjectId, u32)>, u32) {
    panic!("no interactive assignment was expected for this combat")
}

fn zone_of(runner: &GameRunner, id: ObjectId) -> Zone {
    runner.state().objects[&id].zone
}

fn pt(runner: &GameRunner, id: ObjectId) -> (i32, i32) {
    let object = &runner.state().objects[&id];
    (object.power.unwrap_or(0), object.toughness.unwrap_or(0))
}

fn project(runner: &GameRunner, attacker: ObjectId, blockers: &[ObjectId]) -> ProjectedExchange {
    ProjectedExchange::project(
        runner.state(),
        attacker,
        blockers,
        BlockProvenance::Hypothetical,
    )
    .expect("projection should not be barriered")
}

fn projected_pt(exchange: &ProjectedExchange, id: ObjectId) -> (i32, i32) {
    let combatant = exchange.combatant(id).expect("combatant in the exchange");
    (combatant.power, combatant.toughness)
}

fn killed_set(realized: &RealizedPlan) -> HashSet<ObjectId> {
    realized.plan.killed().collect()
}

fn plan_killing(plans: &[RealizedPlan], killed: &[ObjectId]) -> RealizedPlan {
    let wanted: HashSet<ObjectId> = killed.iter().copied().collect();
    plans
        .iter()
        .find(|realized| killed_set(realized) == wanted)
        .unwrap_or_else(|| panic!("no realizable plan kills exactly {killed:?}"))
        .clone()
}

// --- Row 3: parity between the projected and the real declaration ----------

/// CR 702.25a + CR 509.3d: flanking is a FILTERED "becomes blocked by a
/// creature" trigger — one firing for the one blocker. Memnite becomes 0/0 and
/// dies before damage (CR 704.5f); the projection says so before the block.
#[test]
fn parity_flanking_filtered_single_blocker() {
    let mut scenario = new_scenario();
    let lancer = lancer(&mut scenario);
    let memnite = vanilla(&mut scenario, P1, "Memnite", 1, 1);
    let mut runner = scenario.build();
    to_declare_blockers(&mut runner, &[lancer]);

    let exchange = project(&runner, lancer, &[memnite]);
    assert_eq!(projected_pt(&exchange, memnite), (0, 0));
    assert!(!exchange.combatant(memnite).unwrap().alive);

    declare_blockers(&mut runner, &[(memnite, lancer)]);
    assert!(
        !runner.state().stack.is_empty(),
        "reach guard: the real declaration put the flanking trigger on the stack"
    );
    resolve_block_triggers(&mut runner);
    assert_eq!(zone_of(&runner, memnite), Zone::Graveyard);
    assert_eq!(pt(&runner, lancer), projected_pt(&exchange, lancer));
}

/// CR 509.3d: the per-pair filtered form fires once for each blocker without
/// flanking; Benalish Cavalry has flanking and is not shrunk.
#[test]
fn parity_flanking_filtered_per_pair() {
    let mut scenario = new_scenario();
    let lancer = lancer(&mut scenario);
    let memnite = vanilla(&mut scenario, P1, "Memnite", 1, 1);
    let cavalry = cavalry(&mut scenario, P1);
    let mut runner = scenario.build();
    to_declare_blockers(&mut runner, &[lancer]);

    let exchange = project(&runner, lancer, &[memnite, cavalry]);
    assert_eq!(projected_pt(&exchange, memnite), (0, 0));
    assert_eq!(projected_pt(&exchange, cavalry), (2, 2));

    declare_blockers(&mut runner, &[(memnite, lancer), (cavalry, lancer)]);
    assert_eq!(
        runner.state().stack.len(),
        1,
        "reach guard: exactly one flanking firing, for the one blocker without flanking"
    );
    resolve_block_triggers(&mut runner);
    assert_eq!(zone_of(&runner, memnite), Zone::Graveyard);
    assert_eq!(pt(&runner, cavalry), projected_pt(&exchange, cavalry));
}

/// CR 702.45a + CR 509.3c: Bushido is synthesized in the BARE form (no blocker
/// qualifier), so a Bushido attacker blocked by two creatures triggers once and
/// gets +1/+1 once — not once per blocker.
#[test]
fn parity_bushido_bare_form_collapses_to_one_firing() {
    let mut scenario = new_scenario();
    let samurai = retainer(&mut scenario, P0);
    let kobold_a = vanilla(&mut scenario, P1, "Crimson Kobolds", 0, 1);
    let kobold_b = vanilla(&mut scenario, P1, "Crimson Kobolds", 0, 1);
    let mut runner = scenario.build();

    // Probe: both synthesized Bushido triggers are bare-form.
    let bushido_triggers: Vec<&TriggerDefinition> = runner.state().objects[&samurai]
        .trigger_definitions
        .iter_unchecked()
        .map(|entry| &entry.definition)
        .filter(|definition| {
            matches!(
                definition.mode,
                TriggerMode::Blocks | TriggerMode::BecomesBlocked
            )
        })
        .collect();
    assert_eq!(bushido_triggers.len(), 2, "Bushido synthesizes two triggers");
    assert!(bushido_triggers
        .iter()
        .all(|definition| definition.valid_target.is_none()));

    to_declare_blockers(&mut runner, &[samurai]);
    let exchange = project(&runner, samurai, &[kobold_a, kobold_b]);
    assert_eq!(projected_pt(&exchange, samurai), (2, 2));

    declare_blockers(&mut runner, &[(kobold_a, samurai), (kobold_b, samurai)]);
    assert_eq!(runner.state().stack.len(), 1, "reach guard: one bare firing");
    resolve_block_triggers(&mut runner);
    assert_eq!(pt(&runner, samurai), (2, 2));
}

/// CR 702.23a: Rampage counts the creatures blocking it beyond the first,
/// which a declaration fixes exactly — +0/+0 for one blocker, +2/+2 for two.
#[test]
fn parity_rampage_counts_blockers_beyond_the_first() {
    for blocker_count in [1usize, 2] {
        let mut scenario = new_scenario();
        let pack = scenario
            .add_creature(P0, "Wolverine Pack", 2, 4)
            .from_oracle_text_with_keywords(&["Rampage"], WOLVERINE_PACK)
            .id();
        let blockers: Vec<ObjectId> = (0..blocker_count)
            .map(|_| vanilla(&mut scenario, P1, "Grizzly Bears", 2, 2))
            .collect();
        let mut runner = scenario.build();
        to_declare_blockers(&mut runner, &[pack]);

        let exchange = project(&runner, pack, &blockers);
        let bonus = 2 * (blocker_count as i32 - 1);
        assert_eq!(projected_pt(&exchange, pack), (2 + bonus, 4 + bonus));

        let blocks: Vec<(ObjectId, ObjectId)> =
            blockers.iter().map(|&blocker| (blocker, pack)).collect();
        declare_blockers(&mut runner, &blocks);
        assert_eq!(runner.state().stack.len(), 1, "reach guard: rampage fired");
        resolve_block_triggers(&mut runner);
        assert_eq!(pt(&runner, pack), projected_pt(&exchange, pack));
    }
}

/// CR 509.1h: a blocker-side "blocks" trigger — Bushido on the blocker.
#[test]
fn parity_blocks_trigger_pumps_the_blocker() {
    let mut scenario = new_scenario();
    let bears = vanilla(&mut scenario, P0, "Grizzly Bears", 2, 2);
    let samurai = retainer(&mut scenario, P1);
    let mut runner = scenario.build();
    to_declare_blockers(&mut runner, &[bears]);

    let exchange = project(&runner, bears, &[samurai]);
    assert_eq!(projected_pt(&exchange, samurai), (2, 2));

    declare_blockers(&mut runner, &[(samurai, bears)]);
    assert_eq!(runner.state().stack.len(), 1, "reach guard: bushido fired");
    resolve_block_triggers(&mut runner);
    assert_eq!(pt(&runner, samurai), projected_pt(&exchange, samurai));
    assert_eq!(pt(&runner, bears), projected_pt(&exchange, bears));
}

/// CR 603.2: a watcher on a third permanent (Somberwald Alpha) triggers on the
/// declaration too; the projection collects it from every functioning
/// battlefield source and binds its pump exactly as the engine does.
#[test]
fn parity_third_permanent_watcher() {
    let mut scenario = new_scenario();
    let _alpha = scenario
        .add_creature(P0, "Somberwald Alpha", 3, 2)
        .from_oracle_text(SOMBERWALD_ALPHA)
        .id();
    let bears = vanilla(&mut scenario, P0, "Grizzly Bears", 2, 2);
    let kraken = vanilla(&mut scenario, P1, "Kraken Hatchling", 0, 4);
    let mut runner = scenario.build();
    to_declare_blockers(&mut runner, &[bears]);

    let exchange = match ProjectedExchange::project(
        runner.state(),
        bears,
        &[kraken],
        BlockProvenance::Hypothetical,
    ) {
        Ok(exchange) => exchange,
        Err(barrier) => {
            declare_blockers(&mut runner, &[(kraken, bears)]);
            let stack: Vec<String> = runner
                .state()
                .stack
                .iter()
                .map(|entry| format!("{:?}", entry.kind))
                .collect();
            panic!("watcher fold barriered: {barrier:?}; real stack: {stack:#?}");
        }
    };
    let projected = (
        projected_pt(&exchange, bears),
        projected_pt(&exchange, kraken),
    );
    assert_ne!(
        projected,
        ((2, 2), (0, 4)),
        "the watcher's pump is folded into the projection"
    );

    declare_blockers(&mut runner, &[(kraken, bears)]);
    assert_eq!(runner.state().stack.len(), 1, "reach guard: the watcher fired");
    resolve_block_triggers(&mut runner);
    assert_eq!((pt(&runner, bears), pt(&runner, kraken)), projected);
}

// --- Row 4: an unbuilt trigger index is not "no triggers" ------------------

/// CR 603.2: the trigger index is a serde-skipped derived cache. After a round
/// trip it is `Unbuilt` and yields no candidates; the projection must scan the
/// battlefield instead and find the flanking trigger all the same.
#[test]
fn unbuilt_trigger_index_does_not_hide_block_triggers() {
    let mut scenario = new_scenario();
    let lancer = lancer(&mut scenario);
    let memnite = vanilla(&mut scenario, P1, "Memnite", 1, 1);
    let mut runner = scenario.build();
    to_declare_blockers(&mut runner, &[lancer]);
    assert_eq!(
        runner.state().trigger_index.readiness,
        DerivedIndexReadiness::Built
    );
    let built = project(&runner, lancer, &[memnite]);

    let json = serde_json::to_string(runner.state()).expect("state serializes");
    let restored: GameState = serde_json::from_str(&json).expect("state deserializes");
    assert_eq!(
        restored.trigger_index.readiness,
        DerivedIndexReadiness::Unbuilt
    );
    let event = GameEvent::BlockersDeclared {
        assignments: vec![(memnite, lancer)],
    };
    assert!(
        engine::game::trigger_index::candidates_for_event(&restored, &event).is_empty(),
        "reach guard: the unbuilt index offers no candidates"
    );
    let unbuilt = ProjectedExchange::project(
        &restored,
        lancer,
        &[memnite],
        BlockProvenance::Hypothetical,
    )
    .expect("projection on the restored state");
    assert_eq!(projected_pt(&unbuilt, memnite), projected_pt(&built, memnite));
    assert_eq!(projected_pt(&unbuilt, memnite), (0, 0));
}

// --- Row 5: the functioning gate --------------------------------------------

/// CR 113.6 + CR 603.2: a trigger functions only from its declared zones. A
/// battlefield creature whose "becomes blocked" pump functions only from the
/// graveyard adds no shift; its battlefield-functioning twin does.
/// SYNTHETIC definition (not card text): isolates the functioning gate.
#[test]
fn a_non_functioning_trigger_adds_no_shift() {
    for (zones, expected) in [(vec![Zone::Graveyard], (2, 2)), (vec![Zone::Battlefield], (4, 4))] {
        let mut scenario = new_scenario();
        let mut trigger = TriggerDefinition::new(TriggerMode::BecomesBlocked)
            .valid_card(TargetFilter::SelfRef)
            .execute(AbilityDefinition::new(
                AbilityKind::Spell,
                Effect::Pump {
                    power: PtValue::Fixed(2),
                    toughness: PtValue::Fixed(2),
                    target: TargetFilter::SelfRef,
                },
            ));
        trigger.trigger_zones = zones;
        let attacker = scenario
            .add_creature(P0, "Synthetic Pumper", 2, 2)
            .with_trigger_definition(trigger)
            .id();
        let kraken = vanilla(&mut scenario, P1, "Kraken Hatchling", 0, 4);
        let mut runner = scenario.build();
        to_declare_blockers(&mut runner, &[attacker]);
        let exchange = project(&runner, attacker, &[kraken]);
        assert_eq!(projected_pt(&exchange, attacker), expected);
    }
}

// --- Row 2: the flanking shift reaches the to-defender figure --------------

/// CR 702.25a + CR 704.5f + CR 702.19d: Lancer + Audacity (4/2 trample)
/// blocked by Memnite. Flanking makes Memnite 0/0 and it dies before damage;
/// the blocked trampler with no blockers assigns all 4 to the player.
#[test]
fn flanking_shift_reaches_the_to_defender_figure() {
    let mut scenario = new_scenario();
    let lancer = lancer(&mut scenario);
    let aura = scenario
        .add_enchantment_from_oracle(P0, "Audacity", AUDACITY)
        .with_subtypes(vec!["Aura"])
        .id();
    let memnite = vanilla(&mut scenario, P1, "Memnite", 1, 1);
    let mut runner = scenario.build();
    audacity_on(&mut runner, aura, lancer);
    assert_eq!(pt(&runner, lancer), (4, 2), "reach guard: Audacity applies");
    to_declare_blockers(&mut runner, &[lancer]);

    assert_eq!(
        combat_damage_to_defender_checked(runner.state(), lancer, &[memnite]),
        Ok(Projection {
            value: 4,
            basis: Basis::Exact
        })
    );
    assert_eq!(combat_damage_to_defender(runner.state(), lancer, &[memnite]), 4);
    declare_blockers(&mut runner, &[(memnite, lancer)]);
    assert_eq!(finish_combat(&mut runner, no_prompt_expected), 4);
}

/// The sibling with a flanking blocker: no shift, 4 − 2 lethal = 2 tramples over.
#[test]
fn flanking_blocker_is_not_shrunk_and_absorbs_lethal() {
    let mut scenario = new_scenario();
    let lancer = lancer(&mut scenario);
    let aura = scenario
        .add_enchantment_from_oracle(P0, "Audacity", AUDACITY)
        .with_subtypes(vec!["Aura"])
        .id();
    let cavalry = cavalry(&mut scenario, P1);
    let mut runner = scenario.build();
    audacity_on(&mut runner, aura, lancer);
    to_declare_blockers(&mut runner, &[lancer]);

    let exchange = project(&runner, lancer, &[cavalry]);
    assert_eq!(
        exchange.to_defender(),
        Ok(Projection {
            value: 2,
            basis: Basis::Exact
        })
    );
    let plans = exchange.kill_plans(&[]).expect("no barrier");
    let realized = plan_killing(&plans, &[cavalry]);
    declare_blockers(&mut runner, &[(cavalry, lancer)]);
    assert_eq!(finish_combat(&mut runner, assign_from(&realized)), 2);
}

/// CR 510.1c: the non-trampling Lancer blocked by Memnite assigns nothing to
/// the player even though Memnite dies before damage (CR 509.1h).
#[test]
fn non_trampling_flanker_with_its_blocker_gone_assigns_nothing() {
    let mut scenario = new_scenario();
    let lancer = lancer(&mut scenario);
    let memnite = vanilla(&mut scenario, P1, "Memnite", 1, 1);
    let mut runner = scenario.build();
    to_declare_blockers(&mut runner, &[lancer]);
    assert_eq!(
        combat_damage_to_defender_checked(runner.state(), lancer, &[memnite]),
        Ok(Projection {
            value: 0,
            basis: Basis::Exact
        })
    );
    declare_blockers(&mut runner, &[(memnite, lancer)]);
    assert_eq!(finish_combat(&mut runner, no_prompt_expected), 0);
}

// --- Rows 1 / 1b: kill plans and realizability -----------------------------

/// CR 510.1a + CR 510.1c + CR 510.4 + CR 704.5g: Wrecking Ogre (3 double
/// strike) against two 0/4 Kraken Hatchlings — 6 damage against 8 lethal, so
/// exactly one blocker can die: 3 on one in the first-strike step, then 1 more
/// on it and 2 on the other. The engine, given the projected division, agrees.
#[test]
fn sublethal_carry_lets_a_double_striker_kill_one_of_two_walls() {
    let mut scenario = new_scenario();
    let ogre = ogre(&mut scenario);
    let kraken_a = vanilla(&mut scenario, P1, "Kraken Hatchling", 0, 4);
    let kraken_b = vanilla(&mut scenario, P1, "Kraken Hatchling", 0, 4);
    let mut runner = scenario.build();
    to_declare_blockers(&mut runner, &[ogre]);

    let exchange = project(&runner, ogre, &[kraken_a, kraken_b]);
    let plans = exchange.kill_plans(&[]).expect("no barrier");
    assert!(plans.iter().all(|realized| killed_set(realized).len() <= 1));
    let realized = plan_killing(&plans, &[kraken_a]);
    assert!(!realized.outcome.attacker_dies);
    assert!(
        exchange
            .realize(&KillPlan {
                first_strike_step: vec![],
                regular_step: vec![kraken_a, kraken_b],
            })
            .is_err(),
        "6 damage cannot kill 8 toughness"
    );

    declare_blockers(&mut runner, &[(kraken_a, ogre), (kraken_b, ogre)]);
    let mut marked_after_first_step = 0;
    let mut steps = assign_from(&realized);
    let lost = {
        let runner_ref = &mut runner;
        let life_before = runner_ref.life(P1);
        for _ in 0..128 {
            if matches!(
                runner_ref.state().phase,
                Phase::EndCombat | Phase::PostCombatMain
            ) {
                break;
            }
            match runner_ref.state().waiting_for.clone() {
                WaitingFor::Priority { .. } => {
                    runner_ref.act(GameAction::PassPriority).unwrap();
                }
                WaitingFor::AssignCombatDamage { .. } => {
                    let step = current_step(runner_ref.state());
                    if step == CombatDamageSubStep::Regular {
                        marked_after_first_step =
                            runner_ref.state().objects[&kraken_a].damage_marked;
                    }
                    let (assignments, trample_damage) = steps(step);
                    runner_ref
                        .act(GameAction::AssignCombatDamage {
                            mode: CombatDamageAssignmentMode::Normal,
                            assignments,
                            trample_damage,
                            controller_damage: 0,
                        })
                        .unwrap();
                }
                other => panic!("unexpected prompt {other:?}"),
            }
        }
        life_before - runner_ref.life(P1)
    };
    assert_eq!(lost, 0);
    assert!(
        marked_after_first_step > 0,
        "reach guard: the first-strike step's damage stayed marked"
    );
    assert_eq!(zone_of(&runner, kraken_a), Zone::Graveyard);
    assert_eq!(zone_of(&runner, kraken_b), Zone::Battlefield);
    assert_eq!(zone_of(&runner, ogre), Zone::Battlefield);
}

/// B4 hostile: Kraken Hatchling (0/4) + Aegis Turtle (0/5). Both single kills
/// are offered; both together (9 > 6) are not. The Turtle kill (5 = 3 carried
/// + 2) is realizable and the engine resolves it.
#[test]
fn either_wall_can_be_the_one_that_dies() {
    let mut scenario = new_scenario();
    let ogre = ogre(&mut scenario);
    let kraken = vanilla(&mut scenario, P1, "Kraken Hatchling", 0, 4);
    let turtle = vanilla(&mut scenario, P1, "Aegis Turtle", 0, 5);
    let mut runner = scenario.build();
    to_declare_blockers(&mut runner, &[ogre]);

    let exchange = project(&runner, ogre, &[kraken, turtle]);
    let plans = exchange.kill_plans(&[]).expect("no barrier");
    let killed: Vec<HashSet<ObjectId>> = plans.iter().map(killed_set).collect();
    assert!(killed.contains(&HashSet::from([kraken])));
    assert!(killed.contains(&HashSet::from([turtle])));
    assert!(!killed.contains(&HashSet::from([kraken, turtle])));

    let realized = plan_killing(&plans, &[turtle]);
    declare_blockers(&mut runner, &[(kraken, ogre), (turtle, ogre)]);
    finish_combat(&mut runner, assign_from(&realized));
    assert_eq!(zone_of(&runner, turtle), Zone::Graveyard);
    assert_eq!(zone_of(&runner, kraken), Zone::Battlefield);
}

/// Three walls: Memnite + Kraken Hatchling + Aegis Turtle. {Memnite, Turtle}
/// (1 + 5 = 6) is realizable.
#[test]
fn three_blockers_offer_the_cheap_and_the_expensive_kill_together() {
    let mut scenario = new_scenario();
    let ogre = ogre(&mut scenario);
    let memnite = vanilla(&mut scenario, P1, "Memnite", 1, 1);
    let kraken = vanilla(&mut scenario, P1, "Kraken Hatchling", 0, 4);
    let turtle = vanilla(&mut scenario, P1, "Aegis Turtle", 0, 5);
    let mut runner = scenario.build();
    to_declare_blockers(&mut runner, &[ogre]);

    let exchange = project(&runner, ogre, &[memnite, kraken, turtle]);
    let plans = exchange.kill_plans(&[]).expect("no barrier");
    let realized = plan_killing(&plans, &[memnite, turtle]);
    declare_blockers(
        &mut runner,
        &[(memnite, ogre), (kraken, ogre), (turtle, ogre)],
    );
    finish_combat(&mut runner, assign_from(&realized));
    assert_eq!(zone_of(&runner, memnite), Zone::Graveyard);
    assert_eq!(zone_of(&runner, turtle), Zone::Graveyard);
    assert_eq!(zone_of(&runner, kraken), Zone::Battlefield);
}

/// Above the exact-search limit the priority fallback still yields a plan
/// `realize` accepts.
#[test]
fn more_than_eight_blockers_fall_back_to_a_realizable_priority_plan() {
    let mut scenario = new_scenario();
    let ogre = ogre(&mut scenario);
    let kobolds: Vec<ObjectId> = (0..9)
        .map(|_| vanilla(&mut scenario, P1, "Crimson Kobolds", 0, 1))
        .collect();
    let mut runner = scenario.build();
    to_declare_blockers(&mut runner, &[ogre]);
    let exchange = project(&runner, ogre, &kobolds);
    let plans = exchange.kill_plans(&kobolds).expect("no barrier");
    let [fallback] = plans.as_slice() else {
        panic!("the fallback yields one plan, got {}", plans.len());
    };
    assert_eq!(killed_set(fallback).len(), 6, "3 per step, one damage each");
    assert_eq!(exchange.realize(&fallback.plan).as_ref(), Ok(fallback));
}

/// B3': Wrecking Ogre vs two Grizzly Bears. "Both die in the regular step" is
/// unrealizable (3 first-strike damage, only 1 + 1 of survivable capacity), and
/// would have labelled the Ogre dead (2 + 2 >= 3). Realized: one Bear dies in
/// the first-strike step (2, plus 1 carried to the other), the other in the
/// regular step; only it strikes back for 2 < 3, so the Ogre survives.
#[test]
fn realizability_decides_whether_the_double_striker_survives() {
    let mut scenario = new_scenario();
    let ogre = ogre(&mut scenario);
    let bear_a = vanilla(&mut scenario, P1, "Grizzly Bears", 2, 2);
    let bear_b = vanilla(&mut scenario, P1, "Grizzly Bears", 2, 2);
    let mut runner = scenario.build();
    to_declare_blockers(&mut runner, &[ogre]);

    let exchange = project(&runner, ogre, &[bear_a, bear_b]);
    assert_eq!(
        exchange.realize(&KillPlan {
            first_strike_step: vec![],
            regular_step: vec![bear_a, bear_b],
        }),
        Err(ProjectionBarrier::UnrealizablePlan)
    );
    let plans = exchange.kill_plans(&[]).expect("no barrier");
    let realized = plan_killing(&plans, &[bear_a, bear_b]);
    assert!(!realized.outcome.attacker_dies);

    declare_blockers(&mut runner, &[(bear_a, ogre), (bear_b, ogre)]);
    finish_combat(&mut runner, assign_from(&realized));
    assert_eq!(zone_of(&runner, bear_a), Zone::Graveyard);
    assert_eq!(zone_of(&runner, bear_b), Zone::Graveyard);
    assert_eq!(zone_of(&runner, ogre), Zone::Battlefield);
}

/// p1 = p2 = 7 against two 4-toughness walls (synthetic 7/7 double striker).
/// Killing both in the regular step is unrealizable (7 > 3 + 3 survivable);
/// one per step is. With trample, the carry onto the second wall lowers its
/// regular-step lethal to 1, so 6 tramples over — and the engine agrees.
#[test]
fn seven_power_double_striker_against_two_walls() {
    for trample in [false, true] {
        let mut scenario = new_scenario();
        let mut builder = scenario.add_creature(P0, "Synthetic Striker", 7, 7);
        builder.double_strike();
        if trample {
            builder.trample();
        }
        let striker = builder.id();
        let wall_a = vanilla(&mut scenario, P1, "Kraken Hatchling", 0, 4);
        let wall_b = vanilla(&mut scenario, P1, "Kraken Hatchling", 0, 4);
        let mut runner = scenario.build();
        to_declare_blockers(&mut runner, &[striker]);

        let exchange = project(&runner, striker, &[wall_a, wall_b]);
        assert_eq!(
            exchange.realize(&KillPlan {
                first_strike_step: vec![],
                regular_step: vec![wall_a, wall_b],
            }),
            Err(ProjectionBarrier::UnrealizablePlan)
        );
        let realized = exchange
            .realize(&KillPlan {
                first_strike_step: vec![wall_a],
                regular_step: vec![wall_b],
            })
            .expect("one wall per step is realizable");
        let expected_through = if trample { 6 } else { 0 };
        assert_eq!(realized.outcome.to_defender, expected_through);
        assert_eq!(
            exchange.to_defender().map(|projection| projection.value),
            Ok(expected_through)
        );

        declare_blockers(&mut runner, &[(wall_a, striker), (wall_b, striker)]);
        let lost = finish_combat(&mut runner, assign_from(&realized));
        assert_eq!(lost, expected_through);
        assert_eq!(zone_of(&runner, wall_a), Zone::Graveyard);
        assert_eq!(zone_of(&runner, wall_b), Zone::Graveyard);
    }
}

// --- Row 1c: wither / infect carry ------------------------------------------

/// CR 702.80a + CR 702.7b: Rustrazor Butcher (1/2 first strike, wither)
/// blocked by Kraken Hatchling (0/4) and Grizzly Bears (2/2). It can kill
/// neither; its one point is carried as a -1/-1 counter onto the Bears, which
/// then strike back for 1 < 2 and the Butcher survives. Wasting the point on
/// the Kraken lets the Bears deal 2 and the Butcher dies — the engine agrees on
/// both divisions.
#[test]
fn wither_carry_shrinks_the_surviving_blocker_before_it_strikes() {
    for wasted in [false, true] {
        let mut scenario = new_scenario();
        let butcher = scenario
            .add_creature(P0, "Rustrazor Butcher", 1, 2)
            .from_oracle_text_with_keywords(&["First strike", "Wither"], RUSTRAZOR_BUTCHER)
            .id();
        let kraken = vanilla(&mut scenario, P1, "Kraken Hatchling", 0, 4);
        let bears = vanilla(&mut scenario, P1, "Grizzly Bears", 2, 2);
        let mut runner = scenario.build();
        to_declare_blockers(&mut runner, &[butcher]);

        let exchange = project(&runner, butcher, &[kraken, bears]);
        let realized = exchange
            .realize(&KillPlan::default())
            .expect("nothing dies is realizable");
        let carried = realized
            .outcome
            .blockers
            .iter()
            .find(|blocker| blocker.id == bears)
            .unwrap();
        assert_eq!(carried.minus_counters, 1, "the carry lands on the Bears");
        assert_eq!(carried.assigned_amount(), 1);
        assert!(!realized.outcome.attacker_dies);

        declare_blockers(&mut runner, &[(kraken, butcher), (bears, butcher)]);
        let division = if wasted { kraken } else { bears };
        finish_combat(&mut runner, |_| (vec![(division, 1)], 0));
        let butcher_zone = zone_of(&runner, butcher);
        if wasted {
            assert_eq!(butcher_zone, Zone::Graveyard, "the wasted carry kills it");
        } else {
            assert_eq!(butcher_zone, Zone::Battlefield);
            assert_eq!(pt(&runner, bears), (1, 1), "a -1/-1 counter on the Bears");
        }
    }
}

/// CR 702.90c: the infect sibling. Razor Swine (2/1 first strike, infect)
/// against Grizzly Bears + Kraken Hatchling: killing the Bears leaves only the
/// 0-power wall, so the Swine survives; keeping both alive does not.
#[test]
fn infect_carry_and_kill_plans() {
    let mut scenario = new_scenario();
    let swine = scenario
        .add_creature(P0, "Razor Swine", 2, 1)
        .from_oracle_text_with_keywords(&["First strike", "Infect"], RAZOR_SWINE)
        .id();
    let bears = vanilla(&mut scenario, P1, "Grizzly Bears", 2, 2);
    let kraken = vanilla(&mut scenario, P1, "Kraken Hatchling", 0, 4);
    let mut runner = scenario.build();
    to_declare_blockers(&mut runner, &[swine]);

    let exchange = project(&runner, swine, &[bears, kraken]);
    let plans = exchange.kill_plans(&[]).expect("no barrier");
    let kill_bears = plan_killing(&plans, &[bears]);
    assert!(!kill_bears.outcome.attacker_dies);
    let kill_none = plan_killing(&plans, &[]);
    assert!(kill_none.outcome.attacker_dies);

    declare_blockers(&mut runner, &[(bears, swine), (kraken, swine)]);
    finish_combat(&mut runner, assign_from(&kill_bears));
    assert_eq!(zone_of(&runner, bears), Zone::Graveyard);
    assert_eq!(zone_of(&runner, swine), Zone::Battlefield);
}

/// CR 702.2c + CR 702.80a + CR 702.12b + CR 704.5f: lethal need with deathtouch
/// is 1 — except against an indestructible blocker, which only -1/-1 counters
/// from wither/infect can kill (need = its toughness). SYNTHETIC bodies isolate
/// the keyword interaction.
#[test]
fn deathtouch_wither_and_indestructible_lethal_need() {
    let mut scenario = new_scenario();
    let both = scenario
        .add_creature(P0, "Synthetic Withering Deathtoucher", 2, 2)
        .wither()
        .deathtouch()
        .id();
    let deathtouch_only = scenario
        .add_creature(P0, "Synthetic Deathtoucher", 2, 2)
        .deathtouch()
        .id();
    let wall = scenario
        .add_creature(P1, "Synthetic Indestructible Wall", 0, 2)
        .indestructible()
        .id();
    let plain = vanilla(&mut scenario, P1, "Synthetic Wall", 0, 3);
    let runner = scenario.build();
    let state = runner.state();

    let project = |attacker, blocker| {
        ProjectedExchange::project(state, attacker, &[blocker], BlockProvenance::Hypothetical)
            .unwrap()
            .kill_plans(&[])
            .unwrap()
            .iter()
            .map(killed_set)
            .collect::<Vec<_>>()
    };
    // Counters kill the indestructible wall: need = toughness 2 <= 2.
    assert!(project(both, wall).contains(&HashSet::from([wall])));
    // Deathtouch alone cannot destroy it.
    assert_eq!(project(deathtouch_only, wall), vec![HashSet::new()]);
    // Against a destructible wall deathtouch needs 1.
    assert_eq!(project(deathtouch_only, plain), vec![HashSet::from([plain])]);
}

/// A non-wither first striker in the same shape: marks never change power, so
/// the Bears deal 2 whatever the division and the 1/2 dies either way.
#[test]
fn non_wither_carry_placement_does_not_change_the_outcome() {
    for division_on_bears in [false, true] {
        let mut scenario = new_scenario();
        let striker = scenario
            .add_creature(P0, "Synthetic First Striker", 1, 2)
            .first_strike()
            .id();
        let kraken = vanilla(&mut scenario, P1, "Kraken Hatchling", 0, 4);
        let bears = vanilla(&mut scenario, P1, "Grizzly Bears", 2, 2);
        let mut runner = scenario.build();
        to_declare_blockers(&mut runner, &[striker]);
        let exchange = project(&runner, striker, &[kraken, bears]);
        let realized = exchange.realize(&KillPlan::default()).unwrap();
        assert!(realized.outcome.attacker_dies);

        declare_blockers(&mut runner, &[(kraken, striker), (bears, striker)]);
        let target = if division_on_bears { bears } else { kraken };
        finish_combat(&mut runner, |_| (vec![(target, 1)], 0));
        assert_eq!(zone_of(&runner, striker), Zone::Graveyard);
    }
}

// --- Rows 6 / 6b: barrier classification for tramplers ---------------------

/// B1': prevention of damage dealt TO the attacker keeps a first-strike-killed
/// trampler alive and RAISES its damage to the player. Grizzly Bears + Audacity
/// (4/2 trample) blocked by Benalish Knight (2/2 first strike). Unbarriered the
/// Knight's 2 kills the Bears first (0 through); with Healing Salve's shield on
/// the Bears they survive and trample 2 over the Knight.
#[test]
fn prevention_on_the_attacker_is_not_monotone_and_is_barriered() {
    for shielded in [false, true] {
        let mut scenario = new_scenario();
        let bears = vanilla(&mut scenario, P0, "Grizzly Bears", 2, 2);
        let aura = scenario
            .add_enchantment_from_oracle(P0, "Audacity", AUDACITY)
            .with_subtypes(vec!["Aura"])
            .id();
        let salve = scenario
            .add_spell_to_hand_from_oracle(P0, "Healing Salve", true, HEALING_SALVE)
            .id();
        let knight = scenario
            .add_creature(P1, "Benalish Knight", 2, 2)
            .from_oracle_text_with_keywords(&["Flash", "First strike"], BENALISH_KNIGHT)
            .id();
        let mut runner = scenario.build();
        audacity_on(&mut runner, aura, bears);
        if shielded {
            runner.cast(salve).modes(&[1]).target_object(bears).resolve();
        }
        to_declare_blockers(&mut runner, &[bears]);

        let figure = combat_damage_to_defender_checked(runner.state(), bears, &[knight]);
        if shielded {
            assert_eq!(
                figure,
                Err(ProjectionBarrier::UnknownDirection(
                    DamageEventClass::Prevention {
                        recipient: PreventionRecipient::Attacker
                    }
                ))
            );
        } else {
            assert_eq!(
                figure,
                Ok(Projection {
                    value: 0,
                    basis: Basis::Exact
                })
            );
        }
        declare_blockers(&mut runner, &[(knight, bears)]);
        let lost = finish_combat(&mut runner, |_| (vec![(knight, 2)], 2));
        // Engine-measured: 2 through with the shield (hand-derived 2), 0 without.
        assert_eq!(lost, if shielded { 2 } else { 0 });
    }
}

/// Documented conservative gaps for a trampler: Fog Bank's prevention on the
/// blocker, and Furnace of Rath's amplification, give `Err` — no numeric claim.
/// The engine's actual figures are recorded beside the unbarriered twins.
#[test]
fn trampler_barriers_make_no_numeric_claim() {
    // Fog Bank blocking a flying 5-power double-strike trampler (synthetic body).
    for fog in [false, true] {
        let mut scenario = new_scenario();
        let mut builder = scenario.add_creature(P0, "Synthetic Flying Trampler", 5, 5);
        builder.flying().double_strike().trample();
        let trampler = builder.id();
        let wall = if fog {
            scenario
                .add_creature(P1, "Fog Bank", 0, 2)
                .from_oracle_text_with_keywords(&["Defender", "Flying"], FOG_BANK)
                .id()
        } else {
            let mut builder = scenario.add_creature(P1, "Synthetic Flying Wall", 0, 2);
            builder.flying();
            builder.id()
        };
        let mut runner = scenario.build();
        to_declare_blockers(&mut runner, &[trampler]);
        let figure = combat_damage_to_defender_checked(runner.state(), trampler, &[wall]);
        if fog {
            assert_eq!(
                figure,
                Err(ProjectionBarrier::UnknownDirection(
                    DamageEventClass::Prevention {
                        recipient: PreventionRecipient::Blocker
                    }
                ))
            );
        } else {
            assert_eq!(figure.map(|p| p.value), Ok(8));
        }
        declare_blockers(&mut runner, &[(wall, trampler)]);
        let lost = finish_combat(&mut runner, |_| (vec![(wall, 2)], 3));
        // Engine-measured: 6 behind Fog Bank (hand-derived 6), 8 unbarriered.
        assert_eq!(lost, if fog { 6 } else { 8 });
    }

    // Furnace of Rath with a 5/5 trampler blocked by a 0/2.
    for furnace in [false, true] {
        let mut scenario = new_scenario();
        let mut builder = scenario.add_creature(P0, "Synthetic Trampler", 5, 5);
        builder.trample();
        let trampler = builder.id();
        let wall = vanilla(&mut scenario, P1, "Synthetic Wall", 0, 2);
        if furnace {
            scenario.add_enchantment_from_oracle(P1, "Furnace of Rath", FURNACE_OF_RATH);
        }
        let mut runner = scenario.build();
        to_declare_blockers(&mut runner, &[trampler]);
        let figure = combat_damage_to_defender_checked(runner.state(), trampler, &[wall]);
        if furnace {
            assert_eq!(
                figure,
                Err(ProjectionBarrier::UnknownDirection(
                    DamageEventClass::Amplification
                ))
            );
        } else {
            assert_eq!(figure.map(|p| p.value), Ok(3));
        }
        declare_blockers(&mut runner, &[(wall, trampler)]);
        let lost = finish_combat(&mut runner, |_| (vec![(wall, 2)], 3));
        // Engine-measured: 6 under Furnace (hand-derived 3 doubled), 3 without.
        assert_eq!(lost, if furnace { 6 } else { 3 });
    }
}

// --- Rows 6c / 8: the fold has no inert allow-list -------------------------

/// Alaborn Zealot's block trigger destroys both creatures — not a pump, so the
/// fold is barriered; the engine indeed destroys both. The mandatory-pump
/// sibling (a Bushido blocker) folds exactly.
#[test]
fn a_non_pump_block_trigger_is_a_barrier() {
    let mut scenario = new_scenario();
    let bears = vanilla(&mut scenario, P0, "Grizzly Bears", 2, 2);
    let zealot = scenario
        .add_creature(P1, "Alaborn Zealot", 1, 1)
        .from_oracle_text(ALABORN_ZEALOT)
        .id();
    let samurai = retainer(&mut scenario, P1);
    let mut runner = scenario.build();
    to_declare_blockers(&mut runner, &[bears]);

    assert_eq!(
        ProjectedExchange::project(
            runner.state(),
            bears,
            &[zealot],
            BlockProvenance::Hypothetical
        )
        .err(),
        Some(ProjectionBarrier::UnmodelledBlockTrigger { source: zealot })
    );
    assert_eq!(projected_pt(&project(&runner, bears, &[samurai]), samurai), (2, 2));

    declare_blockers(&mut runner, &[(zealot, bears)]);
    assert_eq!(
        runner.state().stack.len(),
        1,
        "reach guard: the real declaration fired the Zealot's trigger"
    );
    let trigger = format!("{:?}", runner.state().stack[0].kind);
    resolve_block_triggers(&mut runner);
    assert_eq!(
        (zone_of(&runner, zealot), zone_of(&runner, bears)),
        (Zone::Graveyard, Zone::Graveyard),
        "engine resolution of the Zealot trigger: {trigger}"
    );
}

/// Psychic Membrane's "you may draw" is optional, so the fold is barriered —
/// paired with the exact flanking fold.
#[test]
fn an_optional_block_trigger_is_a_barrier() {
    let mut scenario = new_scenario();
    let lancer = lancer(&mut scenario);
    let membrane = scenario
        .add_creature(P1, "Psychic Membrane", 0, 3)
        .from_oracle_text_with_keywords(&["Defender"], PSYCHIC_MEMBRANE)
        .id();
    let memnite = vanilla(&mut scenario, P1, "Memnite", 1, 1);
    let mut runner = scenario.build();
    to_declare_blockers(&mut runner, &[lancer]);
    assert_eq!(
        ProjectedExchange::project(
            runner.state(),
            lancer,
            &[membrane],
            BlockProvenance::Hypothetical
        )
        .err(),
        Some(ProjectionBarrier::UnmodelledBlockTrigger { source: membrane })
    );
    assert_eq!(projected_pt(&project(&runner, lancer, &[memnite]), memnite), (0, 0));
}

// --- Row 6d: blocked non-tramplers keep their exact zero --------------------

fn assert_exact_zero_and_exchange_barriered(
    runner: &GameRunner,
    attacker: ObjectId,
    blocker: ObjectId,
    class: DamageEventClass,
) {
    assert_eq!(
        combat_damage_to_defender_checked(runner.state(), attacker, &[blocker]),
        Ok(Projection {
            value: 0,
            basis: Basis::Exact
        })
    );
    assert_eq!(combat_damage_to_defender(runner.state(), attacker, &[blocker]), 0);
    assert_eq!(
        project(runner, attacker, &[blocker]).kill_plans(&[]).err(),
        Some(ProjectionBarrier::UnknownDirection(class)),
        "the block earns the figure but no kill or survival credit"
    );
}

/// (a) Fog Bank blocking Knight of Sursi (flying flanker, no trample).
#[test]
fn fog_bank_block_of_a_non_trampler_is_exact_zero() {
    let mut scenario = new_scenario();
    let knight = scenario
        .add_creature(P0, "Knight of Sursi", 2, 2)
        .from_oracle_text_with_keywords(&["Flying", "Flanking", "Suspend"], KNIGHT_OF_SURSI)
        .id();
    let fog = scenario
        .add_creature(P1, "Fog Bank", 0, 2)
        .from_oracle_text_with_keywords(&["Defender", "Flying"], FOG_BANK)
        .id();
    let mut runner = scenario.build();
    to_declare_blockers(&mut runner, &[knight]);
    assert_exact_zero_and_exchange_barriered(
        &runner,
        knight,
        fog,
        DamageEventClass::Prevention {
            recipient: PreventionRecipient::Blocker,
        },
    );
    declare_blockers(&mut runner, &[(fog, knight)]);
    resolve_block_triggers(&mut runner);
    assert_eq!(finish_combat(&mut runner, no_prompt_expected), 0);
}

/// (b) White Knight (protection from black) blocking Walking Corpse (black 2/2).
#[test]
fn protection_block_of_a_non_trampler_is_exact_zero() {
    let mut scenario = new_scenario();
    let corpse = scenario
        .add_creature(P0, "Walking Corpse", 2, 2)
        .with_color(vec![ManaColor::Black])
        .id();
    let white_knight = scenario
        .add_creature(P1, "White Knight", 2, 2)
        .from_oracle_text_with_keywords(&["First strike", "Protection"], WHITE_KNIGHT)
        .id();
    let mut runner = scenario.build();
    to_declare_blockers(&mut runner, &[corpse]);
    assert_exact_zero_and_exchange_barriered(
        &runner,
        corpse,
        white_knight,
        DamageEventClass::Protection,
    );
    declare_blockers(&mut runner, &[(white_knight, corpse)]);
    assert_eq!(finish_combat(&mut runner, no_prompt_expected), 0);
}

/// (c) Furnace of Rath on the battlefield with an ordinary block.
#[test]
fn amplified_block_of_a_non_trampler_is_exact_zero() {
    let mut scenario = new_scenario();
    let bears = vanilla(&mut scenario, P0, "Grizzly Bears", 2, 2);
    let memnite = vanilla(&mut scenario, P1, "Memnite", 1, 1);
    scenario.add_enchantment_from_oracle(P1, "Furnace of Rath", FURNACE_OF_RATH);
    let mut runner = scenario.build();
    to_declare_blockers(&mut runner, &[bears]);
    assert_exact_zero_and_exchange_barriered(
        &runner,
        bears,
        memnite,
        DamageEventClass::Amplification,
    );
    declare_blockers(&mut runner, &[(memnite, bears)]);
    assert_eq!(finish_combat(&mut runner, no_prompt_expected), 0);
}

/// The unbarriered twin is exact zero WITH kill credit, and a trampler under
/// the same barrier is the documented gap.
#[test]
fn barrier_twins_for_the_non_trampler_rule() {
    let mut scenario = new_scenario();
    let bears = vanilla(&mut scenario, P0, "Grizzly Bears", 2, 2);
    let mut builder = scenario.add_creature(P0, "Synthetic Trampler", 2, 2);
    builder.trample();
    let trampler = builder.id();
    let memnite = vanilla(&mut scenario, P1, "Memnite", 1, 1);
    let runner = scenario.build();
    assert_eq!(
        combat_damage_to_defender_checked(runner.state(), bears, &[memnite]),
        Ok(Projection {
            value: 0,
            basis: Basis::Exact
        })
    );
    assert!(project(&runner, bears, &[memnite]).kill_plans(&[]).is_ok());

    let mut furnace = new_scenario();
    let trampler_b = {
        let mut builder = furnace.add_creature(P0, "Synthetic Trampler", 2, 2);
        builder.trample();
        builder.id()
    };
    let memnite_b = vanilla(&mut furnace, P1, "Memnite", 1, 1);
    furnace.add_enchantment_from_oracle(P1, "Furnace of Rath", FURNACE_OF_RATH);
    let furnace = furnace.build();
    assert_eq!(
        combat_damage_to_defender_checked(furnace.state(), trampler_b, &[memnite_b]),
        Err(ProjectionBarrier::UnknownDirection(
            DamageEventClass::Amplification
        ))
    );
    // The same trampler without Furnace is exact (2 − 1 lethal = 1).
    assert_eq!(
        combat_damage_to_defender_checked(runner.state(), trampler, &[memnite]),
        Ok(Projection {
            value: 1,
            basis: Basis::Exact
        })
    );
}

/// CR 614.9: a replacement that could carry damage to another recipient
/// (redirection) on a blocked non-trampler's combat damage makes even the zero
/// unknown. SYNTHETIC redirection shield on the blocker (not card text). The
/// reach guard: the identical fixture without it reaches rule 1's exact zero.
#[test]
fn redirection_on_a_blocked_non_trampler_is_unknown() {
    for redirected in [false, true] {
        let mut scenario = new_scenario();
        let bears = vanilla(&mut scenario, P0, "Grizzly Bears", 2, 2);
        let mut builder = scenario.add_creature(P1, "Synthetic Redirector", 1, 1);
        if redirected {
            let mut redirect = ReplacementDefinition::new(ReplacementEvent::DamageDone)
                .valid_card(TargetFilter::SelfRef);
            redirect.redirect_target = Some(TargetFilter::Controller);
            builder.with_replacement_definition(redirect);
        }
        let blocker = builder.id();
        let runner = scenario.build();
        let figure = combat_damage_to_defender_checked(runner.state(), bears, &[blocker]);
        if redirected {
            assert_eq!(
                figure,
                Err(ProjectionBarrier::UnknownDirection(
                    DamageEventClass::Redirection
                ))
            );
        } else {
            assert_eq!(
                figure,
                Ok(Projection {
                    value: 0,
                    basis: Basis::Exact
                })
            );
        }
    }
}

// --- Row 7 (engine part): defender-recipient prevention is AtMost ----------

/// Healing Salve's shield on the defending player confines its effect to damage
/// dealt to that player: assignments are unchanged, so the unblocked figure is
/// `AtMost(2)` and the engine deals no more than that.
#[test]
fn prevention_on_the_defending_player_is_at_most() {
    let mut scenario = new_scenario();
    let bears = vanilla(&mut scenario, P0, "Grizzly Bears", 2, 2);
    let salve = scenario
        .add_spell_to_hand_from_oracle(P0, "Healing Salve", true, HEALING_SALVE)
        .id();
    // A potential blocker, so the declare blockers step is not skipped.
    vanilla(&mut scenario, P1, "Memnite", 1, 1);
    let mut runner = scenario.build();
    runner.cast(salve).modes(&[1]).target_player(P1).resolve();
    to_declare_blockers(&mut runner, &[bears]);

    let figure = combat_damage_to_defender_checked(runner.state(), bears, &[]);
    assert_eq!(
        figure,
        Ok(Projection {
            value: 2,
            basis: Basis::AtMost
        })
    );
    declare_blockers(&mut runner, &[]);
    let lost = finish_combat(&mut runner, no_prompt_expected);
    assert!(lost <= 2, "AtMost is an upper bound on what the engine deals");
    assert_eq!(lost, 0, "engine-measured: the shield prevents all 2");
}

// --- Row 9: Declared provenance folds the stack once ----------------------

/// CR 603.3: an unresolved flanking trigger on the stack is folded; once it has
/// resolved its shift is in the object and is not folded again — the same
/// projected stats either way. An unrelated stack object is not foldable;
/// the empty-stack twin is.
#[test]
fn declared_provenance_folds_pending_block_triggers_once() {
    let mut scenario = new_scenario();
    let lancer = lancer(&mut scenario);
    let bears = vanilla(&mut scenario, P1, "Grizzly Bears", 2, 2);
    let salve = scenario
        .add_spell_to_hand_from_oracle(P0, "Healing Salve", true, HEALING_SALVE)
        .id();
    let mut runner = scenario.build();
    to_declare_blockers(&mut runner, &[lancer]);
    declare_blockers(&mut runner, &[(bears, lancer)]);
    assert_eq!(runner.state().stack.len(), 1, "reach guard: flanking on the stack");

    let declared = |state: &GameState| {
        ProjectedExchange::project(state, lancer, &[bears], BlockProvenance::Declared)
    };
    let pending = declared(runner.state()).expect("the flanking trigger folds");
    assert_eq!(projected_pt(&pending, bears), (1, 1));

    // An unrelated spell on top of the trigger is not foldable.
    let mut with_spell = GameRunner::from_state(runner.state().clone());
    let _ = with_spell
        .cast(salve)
        .modes(&[1])
        .target_player(P1)
        .commit();
    assert_eq!(with_spell.state().stack.len(), 2, "reach guard: the spell is on the stack");
    assert_eq!(
        declared(with_spell.state()).err(),
        Some(ProjectionBarrier::StackObjectNotFoldable)
    );

    resolve_block_triggers(&mut runner);
    assert!(runner.state().stack.is_empty());
    assert_eq!(pt(&runner, bears), (1, 1), "the shift is now in the object");
    let resolved = declared(runner.state()).expect("empty stack folds trivially");
    assert_eq!(projected_pt(&resolved, bears), (1, 1), "not folded twice");
}
