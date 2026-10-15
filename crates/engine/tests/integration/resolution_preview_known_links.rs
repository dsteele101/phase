//! `resolution_preview::known_resolution_links` — the chain links a spell on
//! the stack will certainly follow, given the choices already made while it
//! was cast — and `known_condition_outcome`, the classifier it gates on.
//!
//! CR 608.2c: a spell's instructions are followed in the order written, and a
//! later "instead" sentence replaces the instruction before it (a
//! self-replacement effect, CR 614.15). Whether a cost-gated override applies
//! is settled when the spell is cast (CR 601.2b + CR 601.2h, "kicked" per CR
//! 702.33d), so the preview can follow it; a condition read from the game
//! state at resolution is not settled, so the preview reports nothing past it.
//!
//! Real-card rows cast the card through the real pipeline, read the preview
//! off its stack entry, then resolve it and check that resolution did what
//! the preview predicted (parity). Rows marked "engine-parity only" use a
//! hand-built chain for a resolver shape no printed card isolates; their
//! expected value is derived from the resolver arm named on the row.

use engine::game::effects::resolution_preview::{
    known_condition_outcome, known_resolution_links, ConditionOutcome,
};
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::ability::{
    AbilityCondition, Comparator, DamageSource, Effect, MultiTargetSpec, ObjectScope, PlayerFilter,
    QuantityExpr, QuantityRef, ResolvedAbility, SubAbilityLink, TargetFilter, TargetRef,
    TargetSelectionMode, UnlessPayModifier,
};
use engine::types::ability::{AbilityCost, OpponentMayScope, PlayerScope};
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::events::GameEvent;
use engine::types::game_state::{CastPaymentMode, DistributionUnit, PayCostKind, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::keywords::Keyword;
use engine::types::mana::{ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const SHIVAN_FIRE: &str = "Kicker {4} (You may pay an additional {4} as you cast this spell.)\n\
Shivan Fire deals 2 damage to target creature. If this spell was kicked, it deals 4 damage instead.";

const BLOODCHIEFS_THIRST: &str =
    "Kicker {2}{B} (You may pay an additional {2}{B} as you cast this spell.)\n\
Destroy target creature or planeswalker with mana value 2 or less. If this spell was kicked, \
instead destroy target creature or planeswalker.";

const INTO_THE_FLOOD_MAW: &str =
    "Gift a tapped Fish (You may promise an opponent a gift as you cast this spell. \
If you do, they create a tapped 1/1 blue Fish creature token before its other effects.)\n\
Return target creature an opponent controls to its owner's hand. If the gift was promised, \
instead return target nonland permanent an opponent controls to its owner's hand.";

const TOO_EVIL_TO_STAY_DEAD: &str = "Teamwork 4 (As an additional cost to cast this spell, you may tap any number of creatures you control with total power 4 or more.)\nChoose target creature card in your graveyard with mana value 4 or less. If this spell was cast using teamwork, instead choose target creature card in your graveyard. Return the chosen card to the battlefield.";

const FROM_FATHER_TO_SON: &str = "Search your library for a Vehicle card, reveal it, and put it into your hand. If this spell was cast from a graveyard, put that card onto the battlefield instead. Then shuffle.\n\
Flashback {4}{W}{W}{W} (You may cast this card from your graveyard for its flashback cost. Then exile it.)";

const TAKE_THE_FALL: &str = "Target creature gets -1/-0 until end of turn. \
It gets -4/-0 until end of turn instead if you control an outlaw. \
(Assassins, Mercenaries, Pirates, Rogues, and Warlocks are outlaws.)\n\
Draw a card.";

// --- Harness -----------------------------------------------------------------

fn pool(types: &[ManaType]) -> Vec<ManaUnit> {
    types
        .iter()
        .map(|&mana_type| ManaUnit::new(mana_type, ObjectId(0), false, vec![]))
        .collect()
}

fn red_and_generic(generic: usize) -> Vec<ManaUnit> {
    let mut types = vec![ManaType::Red];
    types.extend(std::iter::repeat_n(ManaType::Colorless, generic));
    pool(&types)
}

/// Casts `spell`, answers its optional additional cost with `pay` (tapping
/// `tap` when that cost taps creatures, promising a gift to P1), and answers
/// each target prompt with the next object of `chosen`. Stops once the spell
/// is on the stack.
fn cast(
    runner: &mut GameRunner,
    spell: ObjectId,
    pay: bool,
    tap: &[ObjectId],
    chosen: &[ObjectId],
) {
    let card_id = runner.state().objects[&spell].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: spell,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("the spell is castable");
    let mut chosen = chosen.iter();
    loop {
        let action = match &runner.state().waiting_for {
            WaitingFor::OptionalCostChoice { .. } => GameAction::DecideOptionalCost { pay },
            WaitingFor::ChooseGiftRecipient { .. } => {
                GameAction::ChooseGiftRecipient { opponent: P1 }
            }
            WaitingFor::PayCost {
                kind: PayCostKind::TapCreatures { .. },
                ..
            } => GameAction::SelectCards {
                cards: tap.to_vec(),
            },
            WaitingFor::TargetSelection { .. } => GameAction::SelectTargets {
                targets: vec![TargetRef::Object(
                    *chosen.next().expect("a declared target for every prompt"),
                )],
            },
            WaitingFor::ManaPayment { .. } => GameAction::PassPriority,
            WaitingFor::Priority { .. } => break,
            other => panic!("unexpected prompt while casting: {other:?}"),
        };
        runner.act(action).expect("the cast advances");
    }
    assert!(
        runner.state().stack.iter().any(|entry| entry.id == spell),
        "the spell is on the stack"
    );
}

fn stack_ability(runner: &GameRunner, spell: ObjectId) -> ResolvedAbility {
    runner
        .state()
        .stack
        .iter()
        .find(|entry| entry.id == spell)
        .and_then(|entry| entry.ability())
        .expect("the spell's ability is on the stack")
        .clone()
}

fn preview(runner: &GameRunner, spell: ObjectId) -> Vec<ResolvedAbility> {
    known_resolution_links(runner.state(), &stack_ability(runner, spell))
}

/// Passes priority until the stack is empty, returning every event emitted.
fn resolve(runner: &mut GameRunner) -> Vec<GameEvent> {
    let mut events = Vec::new();
    for _ in 0..40 {
        let action = match &runner.state().waiting_for {
            // CR 701.23: find the first legal candidates.
            WaitingFor::SearchChoice { cards, count, .. } => GameAction::SelectCards {
                cards: cards.iter().take(*count).copied().collect(),
            },
            _ if runner.state().stack.is_empty() => break,
            _ => GameAction::PassPriority,
        };
        events.extend(runner.act(action).expect("resolution advances").events);
    }
    assert!(runner.state().stack.is_empty(), "the stack resolved");
    events
}

fn object_targets(link: &ResolvedAbility) -> Vec<ObjectId> {
    link.targets
        .iter()
        .filter_map(|target| match target {
            TargetRef::Object(id) => Some(*id),
            TargetRef::Player(_) => None,
        })
        .collect()
}

fn fixed_damage(link: &ResolvedAbility) -> Option<i32> {
    match &link.effect {
        Effect::DealDamage {
            amount: QuantityExpr::Fixed { value },
            ..
        } => Some(*value),
        _ => None,
    }
}

fn damage_dealt_to(events: &[GameEvent], victim: ObjectId) -> u32 {
    events
        .iter()
        .filter_map(|event| match event {
            GameEvent::DamageDealt { target, amount, .. }
                if *target == TargetRef::Object(victim) =>
            {
                Some(*amount)
            }
            _ => None,
        })
        .sum()
}

// --- (1)/(2) Shivan Fire -----------------------------------------------------

/// P0 holds Shivan Fire ({R}, kicker {4}) with mana for the kicker; P1
/// controls a 3/3.
fn shivan_fire() -> (GameRunner, ObjectId, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let spell = scenario
        .add_spell_to_hand(P0, "Shivan Fire", true)
        .from_oracle_text_with_keywords(&["Kicker"], SHIVAN_FIRE)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Red],
            generic: 0,
        })
        .id();
    let giant = scenario.add_creature(P1, "Hill Giant", 3, 3).id();
    scenario.with_mana_pool(P0, red_and_generic(4));
    (scenario.build(), spell, giant)
}

/// CR 702.33d + CR 614.15: kicked, Shivan Fire's "4 damage instead" replaces
/// the 2-damage instruction, so the preview reports exactly the one 4-damage
/// link aimed at the creature — never the replaced 2 beside it — and
/// resolution deals exactly that, killing the 3/3.
#[test]
fn kicked_shivan_fire_previews_only_the_four_damage_override() {
    let (mut runner, spell, giant) = shivan_fire();
    cast(&mut runner, spell, true, &[], &[giant]);

    let links = preview(&runner, spell);
    assert_eq!(links.len(), 1, "the override replaces the root: {links:?}");
    assert_eq!(fixed_damage(&links[0]), Some(4));
    assert_eq!(object_targets(&links[0]), vec![giant]);

    let events = resolve(&mut runner);
    assert_eq!(damage_dealt_to(&events, giant), 4, "parity: 4 damage dealt");
    assert_eq!(runner.state().objects[&giant].zone, Zone::Graveyard);
}

/// CR 608.2c: unkicked, the override does not apply; the preview reports the
/// printed 2-damage link and resolution deals 2, leaving the 3/3 alive.
#[test]
fn unkicked_shivan_fire_previews_the_two_damage_root() {
    let (mut runner, spell, giant) = shivan_fire();
    cast(&mut runner, spell, false, &[], &[giant]);

    let links = preview(&runner, spell);
    assert_eq!(links.len(), 1, "{links:?}");
    assert_eq!(fixed_damage(&links[0]), Some(2));
    assert_eq!(object_targets(&links[0]), vec![giant]);

    let events = resolve(&mut runner);
    assert_eq!(damage_dealt_to(&events, giant), 2, "parity: 2 damage dealt");
    assert_eq!(runner.state().objects[&giant].zone, Zone::Battlefield);
    assert_eq!(runner.state().objects[&giant].damage_marked, 2);
}

// --- (3) Parity across the cost-gated "instead" class -------------------------

/// P0 holds Bloodchief's Thirst with mana for the kicker; P1 controls a mana
/// value 1 creature and a mana value 4 creature.
fn bloodchiefs_thirst() -> (GameRunner, ObjectId, ObjectId, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let spell = scenario
        .add_spell_to_hand(P0, "Bloodchief's Thirst", false)
        .from_oracle_text_with_keywords(&["Kicker"], BLOODCHIEFS_THIRST)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Black],
            generic: 0,
        })
        .id();
    let small = scenario
        .add_creature(P1, "Elvish Mystic", 1, 1)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Green],
            generic: 0,
        })
        .id();
    let big = scenario
        .add_creature(P1, "Hill Giant", 3, 3)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Red],
            generic: 3,
        })
        .id();
    scenario.with_mana_pool(
        P0,
        pool(&[
            ManaType::Black,
            ManaType::Black,
            ManaType::Colorless,
            ManaType::Colorless,
        ]),
    );
    (scenario.build(), spell, small, big)
}

/// Kicked, "instead destroy target creature or planeswalker" replaces the
/// mana-value-gated destroy: one Destroy link on the mana value 4 creature,
/// which resolution destroys.
#[test]
fn kicked_bloodchiefs_thirst_previews_the_override_destroy() {
    let (mut runner, spell, small, big) = bloodchiefs_thirst();
    cast(&mut runner, spell, true, &[], &[big]);

    let links = preview(&runner, spell);
    assert_eq!(links.len(), 1, "{links:?}");
    assert!(matches!(links[0].effect, Effect::Destroy { .. }));
    assert_eq!(object_targets(&links[0]), vec![big]);

    resolve(&mut runner);
    assert_eq!(runner.state().objects[&big].zone, Zone::Graveyard);
    assert_eq!(runner.state().objects[&small].zone, Zone::Battlefield);
}

/// Unkicked, the printed destroy on the mana value 1 creature.
#[test]
fn unkicked_bloodchiefs_thirst_previews_the_printed_destroy() {
    let (mut runner, spell, small, big) = bloodchiefs_thirst();
    cast(&mut runner, spell, false, &[], &[small]);

    let links = preview(&runner, spell);
    assert_eq!(links.len(), 1, "{links:?}");
    assert!(matches!(links[0].effect, Effect::Destroy { .. }));
    assert_eq!(object_targets(&links[0]), vec![small]);

    resolve(&mut runner);
    assert_eq!(runner.state().objects[&small].zone, Zone::Graveyard);
    assert_eq!(runner.state().objects[&big].zone, Zone::Battlefield);
}

/// P0 holds Into the Flood Maw; P1 controls a creature and a noncreature
/// artifact.
fn into_the_flood_maw() -> (GameRunner, ObjectId, ObjectId, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let spell = scenario
        .add_spell_to_hand(P0, "Into the Flood Maw", true)
        .from_oracle_text_with_keywords(&["Gift"], INTO_THE_FLOOD_MAW)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Blue],
            generic: 0,
        })
        .id();
    let creature = scenario.add_creature(P1, "Grizzly Bears", 2, 2).id();
    let artifact = scenario.add_artifact_from_oracle(P1, "Mind Stone", "").id();
    scenario.with_mana_pool(P0, pool(&[ManaType::Blue]));
    (scenario.build(), spell, creature, artifact)
}

fn bounce_links(links: &[ResolvedAbility]) -> Vec<&ResolvedAbility> {
    links
        .iter()
        .filter(|link| matches!(link.effect, Effect::Bounce { .. }))
        .collect()
}

/// CR 702.174m: with the gift promised, "instead return target nonland
/// permanent" replaces the creature bounce: exactly one Bounce link, on the
/// artifact, which resolution returns to hand.
#[test]
fn promised_into_the_flood_maw_previews_the_override_bounce() {
    let (mut runner, spell, creature, artifact) = into_the_flood_maw();
    cast(&mut runner, spell, true, &[], &[artifact]);

    let links = preview(&runner, spell);
    let bounces = bounce_links(&links);
    assert_eq!(bounces.len(), 1, "{links:?}");
    assert_eq!(object_targets(bounces[0]), vec![artifact]);

    resolve(&mut runner);
    assert_eq!(runner.state().objects[&artifact].zone, Zone::Hand);
    assert_eq!(runner.state().objects[&creature].zone, Zone::Battlefield);
}

/// Without the gift, the printed creature bounce.
#[test]
fn unpromised_into_the_flood_maw_previews_the_printed_bounce() {
    let (mut runner, spell, creature, artifact) = into_the_flood_maw();
    cast(&mut runner, spell, false, &[], &[creature]);

    let links = preview(&runner, spell);
    let bounces = bounce_links(&links);
    assert_eq!(bounces.len(), 1, "{links:?}");
    assert_eq!(object_targets(bounces[0]), vec![creature]);

    resolve(&mut runner);
    assert_eq!(runner.state().objects[&creature].zone, Zone::Hand);
    assert_eq!(runner.state().objects[&artifact].zone, Zone::Battlefield);
}

/// P0 holds Too Evil to Stay Dead ({0} here so mana is not modeled) with a
/// mana value 3 creature card in its graveyard and a 4-power teammate.
fn too_evil_to_stay_dead() -> (GameRunner, ObjectId, ObjectId, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let corpse = scenario
        .add_creature_to_graveyard(P0, "Fallen Champion", 3, 3)
        .with_mana_cost(ManaCost::generic(3))
        .id();
    let teammate = scenario.add_creature(P0, "Teammate", 4, 4).id();
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Too Evil to Stay Dead", false, TOO_EVIL_TO_STAY_DEAD)
        .with_mana_cost(ManaCost::generic(0))
        .id();
    (scenario.build(), spell, corpse, teammate)
}

fn is_return_to_battlefield(link: &ResolvedAbility) -> bool {
    matches!(
        link.effect,
        Effect::ChangeZone {
            destination: Zone::Battlefield,
            ..
        }
    )
}

/// CR 702.194c + CR 614.15: cast using teamwork, the broad "instead choose"
/// replaces the narrow one, and "Return the chosen card to the battlefield."
/// follows it, aimed at the chosen card, which resolution returns.
#[test]
fn too_evil_to_stay_dead_with_teamwork_previews_override_then_return() {
    let (mut runner, spell, corpse, teammate) = too_evil_to_stay_dead();
    cast(&mut runner, spell, true, &[teammate], &[corpse]);

    let links = preview(&runner, spell);
    assert_eq!(links.len(), 2, "{links:?}");
    assert!(matches!(links[0].effect, Effect::TargetOnly { .. }));
    assert!(is_return_to_battlefield(&links[1]));
    assert_eq!(object_targets(&links[1]), vec![corpse]);

    resolve(&mut runner);
    assert_eq!(runner.state().objects[&corpse].zone, Zone::Battlefield);
}

/// CR 608.2c: without teamwork the override is not swapped in, and the
/// independent last sentence — the unswapped `SequentialSibling` tail — still
/// runs on the narrowly chosen card (issue #4772's shape).
#[test]
fn too_evil_to_stay_dead_without_teamwork_previews_root_then_return() {
    let (mut runner, spell, corpse, _teammate) = too_evil_to_stay_dead();
    cast(&mut runner, spell, false, &[], &[corpse]);

    let links = preview(&runner, spell);
    assert_eq!(links.len(), 2, "{links:?}");
    assert!(matches!(links[0].effect, Effect::TargetOnly { .. }));
    assert!(is_return_to_battlefield(&links[1]));
    assert_eq!(object_targets(&links[1]), vec![corpse]);

    resolve(&mut runner);
    assert_eq!(runner.state().objects[&corpse].zone, Zone::Battlefield);
}

/// CR 608.2c: cast from hand, From Father to Son's "onto the battlefield
/// instead" override does not apply, and its else branch — the base "put it
/// into your hand" chain — follows the search. The preview starts at the
/// search and ends by moving the found card to hand; resolution puts the
/// Vehicle into P0's hand, not onto the battlefield.
#[test]
fn from_father_to_son_cast_from_hand_previews_search_then_base_chain() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let spell = scenario
        .add_spell_to_hand(P0, "From Father to Son", false)
        .from_oracle_text_with_keywords(&["Flashback"], FROM_FATHER_TO_SON)
        .with_mana_cost(ManaCost::generic(0))
        .id();
    let vehicle = scenario.add_card_to_library_top(P0, "Smuggler's Copter");
    let mut runner = scenario.build();
    {
        let vehicle = runner.state_mut().objects.get_mut(&vehicle).unwrap();
        vehicle.card_types.core_types.push(CoreType::Artifact);
        vehicle.card_types.subtypes.push("Vehicle".to_string());
        vehicle.base_card_types = vehicle.card_types.clone();
    }
    cast(&mut runner, spell, false, &[], &[]);

    let ability = stack_ability(&runner, spell);
    assert!(
        ability
            .sub_ability
            .as_deref()
            .is_some_and(|sub| sub.else_ability.is_some()),
        "reach guard: the override carries the base chain as its else branch"
    );
    let links = known_resolution_links(runner.state(), &ability);
    assert!(
        matches!(
            links.first().map(|link| &link.effect),
            Some(Effect::SearchLibrary { .. })
        ),
        "{links:?}"
    );
    assert!(
        matches!(
            links.last().map(|link| &link.effect),
            Some(Effect::ChangeZone {
                destination: Zone::Hand,
                ..
            })
        ),
        "{links:?}"
    );

    resolve(&mut runner);
    assert_eq!(runner.state().objects[&vehicle].zone, Zone::Hand);
}

/// "It gets -4/-0 instead if you control an outlaw" is decided by the board
/// at resolution, so the preview claims nothing about Take the Fall — not
/// even its first instruction, which the override may yet replace. The
/// reach guard: the spell did resolve (−1/−0 and a card drawn).
#[test]
fn take_the_fall_unsettled_override_previews_nothing() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Take the Fall", true, TAKE_THE_FALL)
        .with_mana_cost(ManaCost::generic(0))
        .id();
    let bear = scenario.add_creature(P1, "Grizzly Bears", 2, 2).id();
    scenario.with_library_top(P0, &["Island"]);
    let mut runner = scenario.build();
    cast(&mut runner, spell, false, &[], &[bear]);

    let ability = stack_ability(&runner, spell);
    let sub = ability.sub_ability.as_deref().expect("the override sub");
    assert!(
        matches!(
            sub.condition,
            Some(AbilityCondition::ConditionInstead { .. })
        ),
        "reach guard: the override is the board-gated ConditionInstead"
    );
    assert!(known_resolution_links(runner.state(), &ability).is_empty());

    let hand_before = runner.state().players[0].hand.len();
    resolve(&mut runner);
    assert_eq!(runner.state().players[0].hand.len(), hand_before + 1);
    assert_eq!(runner.state().objects[&bear].power, Some(1));
}

// --- (4)-(8), (10) Resolver shapes, engine-parity only -------------------------

/// A battlefield 3/3 for P1 and a fresh state to evaluate hand-built chains
/// against. The chains use P0's 3/3 as their source.
fn board() -> (GameRunner, ObjectId, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let source = scenario.add_creature(P0, "Source", 3, 3).id();
    let victim = scenario.add_creature(P1, "Victim", 3, 3).id();
    (scenario.build(), source, victim)
}

fn link(effect: Effect, targets: Vec<TargetRef>, source: ObjectId) -> ResolvedAbility {
    ResolvedAbility::new(effect, targets, source, P0)
}

fn damage(value: i32, target: TargetFilter) -> Effect {
    Effect::DealDamage {
        amount: QuantityExpr::Fixed { value },
        target,
        damage_source: None,
        excess: None,
    }
}

fn destroy() -> Effect {
    Effect::Destroy {
        target: TargetFilter::Any,
        cant_regenerate: false,
    }
}

fn draw() -> Effect {
    Effect::Draw {
        count: QuantityExpr::Fixed { value: 1 },
        target: TargetFilter::Controller,
    }
}

fn with_sub(
    mut parent: ResolvedAbility,
    mut sub: ResolvedAbility,
    condition: Option<AbilityCondition>,
    sub_link: SubAbilityLink,
) -> ResolvedAbility {
    sub.condition = condition;
    sub.sub_link = sub_link;
    parent.sub_ability = Some(Box::new(sub));
    parent
}

/// "Deal 2 to target creature. If kicked, it deals 4 instead." with `tail`
/// hung under the override.
fn kicker_override(
    source: ObjectId,
    victim: ObjectId,
    kicked: bool,
    tail: Option<ResolvedAbility>,
) -> ResolvedAbility {
    let mut root = link(
        damage(2, TargetFilter::Any),
        vec![TargetRef::Object(victim)],
        source,
    );
    root.context.additional_cost_paid = kicked;
    let mut override_sub = link(damage(4, TargetFilter::ParentTarget), vec![], source);
    override_sub.sub_ability = tail.map(Box::new);
    with_sub(
        root,
        override_sub,
        Some(AbilityCondition::AdditionalCostPaidInstead),
        SubAbilityLink::SequentialSibling,
    )
}

fn effects(links: &[ResolvedAbility]) -> Vec<&Effect> {
    links.iter().map(|link| &link.effect).collect()
}

/// (4) Engine-parity only — the not-swapped arm (`resolve_chain_body`'s
/// "instead" not-swap continuation) runs a `ContinuationStep` sub of the
/// override never: it is part of the replaced clause. Unkicked: the root
/// alone. Reach guard, same shape kicked: the override and its continuation.
#[test]
fn unswapped_override_drops_its_continuation_step() {
    let (runner, source, victim) = board();
    let mut continuation = link(draw(), vec![], source);
    continuation.sub_link = SubAbilityLink::ContinuationStep;

    let unkicked = kicker_override(source, victim, false, Some(continuation.clone()));
    let links = known_resolution_links(runner.state(), &unkicked);
    assert_eq!(effects(&links), vec![&damage(2, TargetFilter::Any)]);

    let kicked = kicker_override(source, victim, true, Some(continuation));
    let links = known_resolution_links(runner.state(), &kicked);
    assert_eq!(
        effects(&links),
        vec![&damage(4, TargetFilter::ParentTarget), &draw()]
    );
    assert_eq!(object_targets(&links[0]), vec![victim]);
}

/// (5) Engine-parity only — an unswapped override's else branch is the base
/// chain that follows the root (From Father to Son's shape: "put it into your
/// hand … onto the battlefield instead"). Root, then the else chain.
#[test]
fn unswapped_override_hands_off_to_its_else_chain() {
    let (runner, source, victim) = board();
    let mut root = link(destroy(), vec![TargetRef::Object(victim)], source);
    root.context.cast_from_zone = Some(Zone::Hand);
    let mut override_sub = link(draw(), vec![], source);
    override_sub.else_ability = Some(Box::new(link(
        damage(1, TargetFilter::ParentTarget),
        vec![],
        source,
    )));
    let chain = with_sub(
        root,
        override_sub,
        Some(AbilityCondition::ConditionInstead {
            inner: Box::new(AbilityCondition::WasCast {
                zone: Some(Zone::Graveyard),
            }),
        }),
        SubAbilityLink::ContinuationStep,
    );

    let links = known_resolution_links(runner.state(), &chain);
    assert_eq!(
        effects(&links),
        vec![&destroy(), &damage(1, TargetFilter::ParentTarget)]
    );
    assert_eq!(
        object_targets(&links[1]),
        vec![victim],
        "the else chain inherits the root's target"
    );
}

/// (6) Engine-parity only — an unswapped override's `SequentialSibling` tail
/// is an independent later sentence and runs; the same tail left
/// `Unimplemented` is never assumed.
#[test]
fn unswapped_override_runs_its_sibling_tail_but_not_an_unimplemented_one() {
    let (runner, source, victim) = board();
    let mut tail = link(draw(), vec![], source);
    tail.sub_link = SubAbilityLink::SequentialSibling;
    let chain = kicker_override(source, victim, false, Some(tail));
    let links = known_resolution_links(runner.state(), &chain);
    assert_eq!(
        effects(&links),
        vec![&damage(2, TargetFilter::Any), &draw()]
    );

    let mut unimplemented = link(
        Effect::unimplemented("copy", "copy it with new targets"),
        vec![],
        source,
    );
    unimplemented.sub_link = SubAbilityLink::SequentialSibling;
    let chain = kicker_override(source, victim, false, Some(unimplemented));
    let links = known_resolution_links(runner.state(), &chain);
    assert_eq!(effects(&links), vec![&damage(2, TargetFilter::Any)]);
}

/// (7) Engine-parity only — an override whose condition is read at
/// resolution may replace the lethal root with something harmless, so
/// nothing is reported, not even the root. Reach guard: the same chain with
/// a settled-false condition reports the root.
#[test]
fn unsettled_override_reports_nothing_settled_false_reports_the_root() {
    let (runner, source, victim) = board();
    let chain = |condition| {
        with_sub(
            link(destroy(), vec![TargetRef::Object(victim)], source),
            link(draw(), vec![], source),
            Some(condition),
            SubAbilityLink::SequentialSibling,
        )
    };
    let unsettled = [
        AbilityCondition::TargetHasKeywordInstead {
            keyword: Keyword::Flying,
        },
        AbilityCondition::ConditionInstead {
            inner: Box::new(AbilityCondition::QuantityCheck {
                lhs: QuantityExpr::Ref {
                    qty: QuantityRef::HandSize {
                        player: PlayerScope::Controller,
                    },
                },
                comparator: Comparator::GE,
                rhs: QuantityExpr::Fixed { value: 1 },
            }),
        },
    ];
    for condition in unsettled {
        let links = known_resolution_links(runner.state(), &chain(condition.clone()));
        assert!(links.is_empty(), "{condition:?} is unsettled: {links:?}");
    }

    let settled_false = chain(AbilityCondition::ConditionInstead {
        inner: Box::new(AbilityCondition::WasCast {
            zone: Some(Zone::Graveyard),
        }),
    });
    let links = known_resolution_links(runner.state(), &settled_false);
    assert_eq!(effects(&links), vec![&destroy()]);
}

/// (8) Engine-parity only — a plain sub gated on the board at resolution: the
/// root before it is certain, the walk stops at the gate. Side by side with
/// (7): an unsettled plain gate loses only what follows it, an unsettled
/// override loses the root too.
#[test]
fn unsettled_plain_gate_keeps_the_root_and_stops() {
    let (runner, source, victim) = board();
    let gate = AbilityCondition::QuantityCheck {
        lhs: QuantityExpr::Ref {
            qty: QuantityRef::HandSize {
                player: PlayerScope::Controller,
            },
        },
        comparator: Comparator::GE,
        rhs: QuantityExpr::Fixed { value: 1 },
    };
    let plain = with_sub(
        link(destroy(), vec![TargetRef::Object(victim)], source),
        link(draw(), vec![], source),
        Some(gate.clone()),
        SubAbilityLink::ContinuationStep,
    );
    let links = known_resolution_links(runner.state(), &plain);
    assert_eq!(effects(&links), vec![&destroy()]);

    let override_chain = with_sub(
        link(destroy(), vec![TargetRef::Object(victim)], source),
        link(draw(), vec![], source),
        Some(AbilityCondition::ConditionInstead {
            inner: Box::new(gate),
        }),
        SubAbilityLink::ContinuationStep,
    );
    assert!(known_resolution_links(runner.state(), &override_chain).is_empty());
}

// --- (9) Classifier table ------------------------------------------------------

#[test]
fn known_condition_outcome_table() {
    let (mut runner, source, victim) = board();
    let mut kicked = link(destroy(), vec![TargetRef::Object(victim)], source);
    kicked.context.additional_cost_paid = true;
    let unkicked = link(destroy(), vec![TargetRef::Object(victim)], source);
    let state = runner.state();
    let outcome = |condition: AbilityCondition, ability: &ResolvedAbility| {
        known_condition_outcome(&condition, state, ability)
    };
    let paid = || AbilityCondition::AdditionalCostPaidInstead;
    let unsettled = || AbilityCondition::TargetHasKeywordInstead {
        keyword: Keyword::Flying,
    };

    assert_eq!(outcome(paid(), &kicked), ConditionOutcome::Holds);
    assert_eq!(outcome(paid(), &unkicked), ConditionOutcome::Fails);
    assert_eq!(
        outcome(unsettled(), &kicked),
        ConditionOutcome::Undetermined
    );
    assert_eq!(
        outcome(AbilityCondition::IsYourTurn, &kicked),
        ConditionOutcome::Undetermined
    );
    assert_eq!(
        outcome(
            AbilityCondition::ConditionInstead {
                inner: Box::new(paid())
            },
            &kicked
        ),
        ConditionOutcome::Holds
    );
    // Kleene: a settled operand decides a compound regardless of the other.
    assert_eq!(
        outcome(
            AbilityCondition::And {
                conditions: vec![paid(), unsettled()]
            },
            &unkicked
        ),
        ConditionOutcome::Fails
    );
    assert_eq!(
        outcome(
            AbilityCondition::And {
                conditions: vec![paid(), unsettled()]
            },
            &kicked
        ),
        ConditionOutcome::Undetermined
    );
    assert_eq!(
        outcome(
            AbilityCondition::Or {
                conditions: vec![paid(), unsettled()]
            },
            &kicked
        ),
        ConditionOutcome::Holds
    );
    assert_eq!(
        outcome(
            AbilityCondition::Or {
                conditions: vec![paid(), unsettled()]
            },
            &unkicked
        ),
        ConditionOutcome::Undetermined
    );
    assert_eq!(
        outcome(
            AbilityCondition::Not {
                condition: Box::new(paid())
            },
            &kicked
        ),
        ConditionOutcome::Fails
    );
    assert_eq!(
        outcome(
            AbilityCondition::Not {
                condition: Box::new(unsettled())
            },
            &kicked
        ),
        ConditionOutcome::Undetermined
    );
    // CR 115.1: "if it was kicked" about the TARGET reads an object that may
    // change before resolution; about the source, the spell's own context.
    let scoped = |subject| AbilityCondition::AdditionalCostPaid {
        subject,
        source: Default::default(),
        origin: None,
        origin_ordinal: None,
        variant: None,
        kicker_cost: None,
        min_count: 1,
    };
    assert_eq!(
        outcome(scoped(ObjectScope::Source), &kicked),
        ConditionOutcome::Holds
    );
    assert_eq!(
        outcome(scoped(ObjectScope::Target), &kicked),
        ConditionOutcome::Undetermined
    );

    // CR 400.7: a cast stamp read from the spell's own object is settled while
    // that object exists; with the object gone nothing is settled.
    let variant = || AbilityCondition::CastVariantPaidInstead {
        variant: engine::types::ability::CastVariantPaid::Evoke,
    };
    assert_eq!(outcome(variant(), &kicked), ConditionOutcome::Fails);
    runner.state_mut().objects.remove(&source);
    assert_eq!(
        known_condition_outcome(&variant(), runner.state(), &kicked),
        ConditionOutcome::Undetermined,
        "source gone"
    );
}

// --- (10) Hostile rows -----------------------------------------------------------

/// Two Shivan Fires on the stack, one kicked and one not, each previewed from
/// its own casting context.
#[test]
fn each_stack_entry_is_previewed_from_its_own_casting_choices() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let fires: Vec<ObjectId> = (0..2)
        .map(|_| {
            scenario
                .add_spell_to_hand(P0, "Shivan Fire", true)
                .from_oracle_text_with_keywords(&["Kicker"], SHIVAN_FIRE)
                .with_mana_cost(ManaCost::Cost {
                    shards: vec![ManaCostShard::Red],
                    generic: 0,
                })
                .id()
        })
        .collect();
    let giant = scenario.add_creature(P1, "Hill Giant", 3, 3).id();
    let mut mana = red_and_generic(4);
    mana.extend(red_and_generic(0));
    scenario.with_mana_pool(P0, mana);
    let mut runner = scenario.build();

    cast(&mut runner, fires[0], true, &[], &[giant]);
    cast(&mut runner, fires[1], false, &[], &[giant]);

    let kicked = preview(&runner, fires[0]);
    let unkicked = preview(&runner, fires[1]);
    assert_eq!(
        kicked.iter().map(fixed_damage).collect::<Vec<_>>(),
        vec![Some(4)]
    );
    assert_eq!(
        unkicked.iter().map(fixed_damage).collect::<Vec<_>>(),
        vec![Some(2)]
    );
}

/// A chain whose second instruction targets a second object reports both
/// links, each with its own target.
#[test]
fn second_target_of_a_two_target_chain_is_reported_on_its_own_link() {
    let (runner, source, victim) = board();
    let other = source;
    let chain = with_sub(
        link(destroy(), vec![TargetRef::Object(other)], source),
        link(destroy(), vec![TargetRef::Object(victim)], source),
        None,
        SubAbilityLink::SequentialSibling,
    );
    let links = known_resolution_links(runner.state(), &chain);
    assert_eq!(links.len(), 2);
    assert_eq!(object_targets(&links[0]), vec![other]);
    assert_eq!(object_targets(&links[1]), vec![victim]);
}

/// Every resolution-time divergence on a head stops the walk there. Each row
/// is paired with the undiverted head, which is reported.
#[test]
fn diverting_heads_report_nothing() {
    let (runner, source, victim) = board();
    let base = || link(destroy(), vec![TargetRef::Object(victim)], source);
    assert_eq!(
        known_resolution_links(runner.state(), &base()).len(),
        1,
        "reach guard: the undiverted head is reported"
    );

    let diverted: Vec<(&str, ResolvedAbility)> = vec![
        ("optional", {
            let mut head = base();
            head.optional = true;
            head
        }),
        ("optional_for", {
            let mut head = base();
            head.optional_for = Some(OpponentMayScope::AnyOpponent);
            head
        }),
        ("optional_player", {
            let mut head = base();
            head.optional_player = Some(TargetFilter::Player);
            head
        }),
        ("unless_pay", {
            let mut head = base();
            head.unless_pay = Some(UnlessPayModifier {
                cost: AbilityCost::Mana {
                    cost: ManaCost::generic(2),
                },
                payer: TargetFilter::Controller,
            });
            head
        }),
        ("player_scope", {
            let mut head = base();
            head.player_scope = Some(PlayerFilter::Opponent);
            head
        }),
        ("repeat_for", {
            let mut head = base();
            head.repeat_for = Some(QuantityExpr::Fixed { value: 2 });
            head
        }),
        ("multi_target", {
            let mut head = base();
            head.multi_target = Some(MultiTargetSpec {
                min: QuantityExpr::Fixed { value: 1 },
                max: Some(QuantityExpr::Fixed { value: 2 }),
            });
            head
        }),
        ("distribution", {
            let mut head = base();
            head.distribution = Some(vec![(TargetRef::Object(victim), 3)]);
            head
        }),
        ("distribute", {
            let mut head = base();
            head.distribute = Some(DistributionUnit::Damage);
            head
        }),
        ("random selection", {
            let mut head = base();
            head.target_selection_mode = TargetSelectionMode::Random;
            head
        }),
        ("unimplemented", {
            let mut head = base();
            head.effect = Effect::unimplemented("mystery", "does something");
            head
        }),
        ("damage source rebound at resolution", {
            let mut head = base();
            head.effect = Effect::DealDamage {
                amount: QuantityExpr::Fixed { value: 3 },
                target: TargetFilter::Any,
                damage_source: Some(DamageSource::Target),
                excess: None,
            };
            head
        }),
    ];
    for (field, head) in diverted {
        assert!(
            known_resolution_links(runner.state(), &head).is_empty(),
            "{field} diverts the head"
        );
    }
}

/// A head whose own settled-false condition sends it to its else branch does
/// not also run its override's not-swapped continuation: only the else chain
/// is reported. Reach guard: the same head without a condition reports the
/// head and the continuation.
#[test]
fn false_head_runs_its_else_chain_not_the_override_continuation() {
    let (runner, source, victim) = board();
    let chain = |head_condition: Option<AbilityCondition>| {
        let mut tail = link(damage(1, TargetFilter::ParentTarget), vec![], source);
        tail.sub_link = SubAbilityLink::SequentialSibling;
        let mut head = kicker_override(source, victim, false, Some(tail));
        head.condition = head_condition;
        head.else_ability = Some(Box::new(link(draw(), vec![], source)));
        head
    };

    let false_head = chain(Some(AbilityCondition::WasCast {
        zone: Some(Zone::Graveyard),
    }));
    let links = known_resolution_links(runner.state(), &false_head);
    assert_eq!(effects(&links), vec![&draw()]);

    let links = known_resolution_links(runner.state(), &chain(None));
    assert_eq!(
        effects(&links),
        vec![
            &damage(2, TargetFilter::Any),
            &damage(1, TargetFilter::ParentTarget)
        ]
    );
}
