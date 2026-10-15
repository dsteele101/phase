//! `payable_cost_routes`: every complete, payable way to settle the choices
//! between costs in an activation cost.
//!
//! CR 601.2h + CR 602.2b + CR 118.3: an activation prompts for its choices
//! between costs one at a time, but pays the total cost at once and partial
//! payments are not allowed, so an early choice can be completed only together
//! with the later ones. These tests pin that the engine's route enumeration
//! judges each fully settled cost with the activation payability authority,
//! that its first legs agree with the branches the activation prompt offers,
//! and that the enumeration bound is honoured without truncation.
//!
//! Printed cards carry at most one choice between costs per activation cost
//! (Camellia, the Seedmiser's Forage; the Crystal Shard cycle). The costs with
//! two or nested choices below are SYNTHETIC, hand-built `AbilityDefinition`s:
//! they exist to exercise the enumerator's joint-feasibility contract, not to
//! model a real card.

use std::sync::Arc;

use engine::game::casting::{
    activated_ability_definitions, can_activate_ability_now, find_one_of_cost,
    one_of_branch_payable_in, payable_cost_routes, CostRouteBudgetExceeded, PayableCostRoute,
    MAX_COST_ROUTE_LEAVES,
};
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::ability::{
    AbilityCost, AbilityDefinition, AbilityKind, ControllerRef, Effect, QuantityExpr,
    SacrificeCost, TargetFilter, TypedFilter,
};
use engine::types::actions::GameAction;
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::identifiers::{ObjectId, ObjectIncarnationRef};
use engine::types::mana::{ManaCost, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

const CAMELLIA_ORACLE: &str = "Menace\nOther Squirrels you control have menace.\nWhenever you sacrifice one or more Foods, create a 1/1 green Squirrel creature token.\n{2}, Forage: Put a +1/+1 counter on each other Squirrel you control. (To forage, exile three cards from your graveyard or sacrifice a Food.)";

const CRYSTAL_SHARD_ORACLE: &str = "{3}, {T} or {U}, {T}: Return target creature to its owner's hand unless its controller pays {1}.";

// --- cost builders ---------------------------------------------------------

fn mana(amount: u32) -> AbilityCost {
    AbilityCost::Mana {
        cost: ManaCost::generic(amount),
    }
}

fn life(amount: i32) -> AbilityCost {
    AbilityCost::PayLife {
        amount: QuantityExpr::Fixed { value: amount },
    }
}

fn sacrifice_self() -> AbilityCost {
    AbilityCost::Sacrifice(SacrificeCost::count(TargetFilter::SelfRef, 1))
}

fn sacrifice_creature() -> AbilityCost {
    AbilityCost::Sacrifice(SacrificeCost::count(
        TargetFilter::Typed(TypedFilter::creature().controller(ControllerRef::You)),
        1,
    ))
}

fn one_of(costs: Vec<AbilityCost>) -> AbilityCost {
    AbilityCost::OneOf { costs }
}

fn all_of(costs: Vec<AbilityCost>) -> AbilityCost {
    AbilityCost::Composite { costs }
}

/// A free, always-payable branch for the enumeration-bound fixtures.
fn free() -> AbilityCost {
    life(0)
}

/// E1's synthetic two-choice cost: "{1} or pay 2 life, and {1} or sacrifice
/// this". With one mana, the two mana legs cannot both be paid.
fn coupled_cost() -> AbilityCost {
    all_of(vec![
        one_of(vec![mana(1), life(2)]),
        one_of(vec![mana(1), sacrifice_self()]),
    ])
}

fn nested_cost() -> AbilityCost {
    one_of(vec![one_of(vec![life(2), mana(1)]), mana(3)])
}

fn two_life_choices_cost() -> AbilityCost {
    all_of(vec![
        one_of(vec![life(15), mana(1)]),
        one_of(vec![life(10), mana(1)]),
    ])
}

fn shared_creature_cost() -> AbilityCost {
    all_of(vec![
        one_of(vec![sacrifice_creature(), mana(1)]),
        sacrifice_creature(),
    ])
}

fn gain_life_ability(cost: AbilityCost) -> AbilityDefinition {
    let mut ability = AbilityDefinition::new(
        AbilityKind::Activated,
        Effect::GainLife {
            amount: QuantityExpr::Fixed { value: 1 },
            player: TargetFilter::Controller,
        },
    );
    ability.cost = Some(cost);
    ability
}

fn colorless(amount: usize) -> Vec<ManaUnit> {
    (0..amount)
        .map(|_| ManaUnit::new(ManaType::Colorless, ObjectId(0), false, vec![]))
        .collect()
}

// --- boards ------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
struct Spec {
    mana: usize,
    life: i32,
    own_food: bool,
    graveyard: usize,
}

/// A 2/2 creature carrying one hand-built activated ability (index 0) with
/// `cost`, its controller's only creature.
fn board(cost: &AbilityCost, spec: Spec) -> (GameRunner, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_life(P0, spec.life);
    let source = scenario
        .add_creature(P0, "Route Source", 2, 2)
        .with_ability_definition(gain_life_ability(cost.clone()))
        .id();
    if spec.own_food {
        scenario
            .add_creature(P0, "Food", 0, 0)
            .as_artifact()
            .with_subtypes(vec!["Food"]);
    }
    let names: Vec<String> = (0..spec.graveyard)
        .map(|index| format!("Graveyard Card {index}"))
        .collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    scenario.with_graveyard(P0, &refs);
    scenario.with_mana_pool(P0, colorless(spec.mana));
    (scenario.build(), source)
}

fn routes_for(
    state: &GameState,
    source: ObjectId,
    cost: &AbilityCost,
    ability_index: Option<usize>,
) -> Vec<PayableCostRoute> {
    payable_cost_routes(state, P0, source, cost, ability_index)
        .expect("fixture stays within the route bound")
}

/// Each route as its `(branch_index, branch)` legs.
fn shapes(routes: &[PayableCostRoute]) -> Vec<Vec<(usize, AbilityCost)>> {
    routes
        .iter()
        .map(|route| {
            route
                .legs
                .iter()
                .map(|leg| (leg.branch_index, leg.branch.clone()))
                .collect()
        })
        .collect()
}

/// The distinct first-leg branch indices, ascending.
fn first_leg_indices(routes: &[PayableCostRoute]) -> Vec<usize> {
    let mut indices: Vec<usize> = routes
        .iter()
        .filter_map(|route| route.legs.first().map(|leg| leg.branch_index))
        .collect();
    indices.sort_unstable();
    indices.dedup();
    indices
}

/// The reference: indices (in the unfiltered list) of the first choice's
/// branches the activation prompt offers — the same per-branch authority
/// `payable_one_of_activation_branches` filters with.
fn prompt_indices(
    state: &GameState,
    source: ObjectId,
    cost: &AbilityCost,
    ability_index: Option<usize>,
) -> Vec<usize> {
    find_one_of_cost(cost).map_or_else(Vec::new, |branches| {
        branches
            .iter()
            .enumerate()
            .filter(|(_, branch)| {
                one_of_branch_payable_in(state, P0, source, cost, branch, ability_index)
            })
            .map(|(index, _)| index)
            .collect()
    })
}

fn ability_index_with_cost(state: &GameState, source: ObjectId) -> (usize, AbilityCost) {
    activated_ability_definitions(state, source)
        .into_iter()
        .find_map(|(index, ability)| {
            ability
                .cost
                .filter(|cost| find_one_of_cost(cost).is_some())
                .map(|cost| (index, cost))
        })
        .expect("the card has an activated ability with a choice between costs")
}

// --- E1: coupled resources ----------------------------------------------------

/// CR 601.2g + CR 119.4: with one mana, "{1} then {1}" is not a route; the mana
/// leg of the first choice completes only through the sacrifice, while paying
/// life first leaves the mana for the second choice.
#[test]
fn coupled_mana_legs_are_settled_jointly() {
    let cost = coupled_cost();
    let (runner, source) = board(
        &cost,
        Spec {
            mana: 1,
            life: 20,
            own_food: false,
            graveyard: 0,
        },
    );
    let routes = routes_for(runner.state(), source, &cost, Some(0));
    assert_eq!(
        shapes(&routes),
        vec![
            vec![(0, mana(1)), (1, sacrifice_self())],
            vec![(1, life(2)), (0, mana(1))],
            vec![(1, life(2)), (1, sacrifice_self())],
        ],
        "every payable route in prompt order, and no mana-then-mana route"
    );
    assert_eq!(
        routes[1].resolved,
        all_of(vec![life(2), mana(1)]),
        "a route carries the whole cost its choices settle to"
    );
}

/// CR 119.4: at 1 life the 2-life leg is no option, so the only route spends
/// the mana first and sacrifices the source.
#[test]
fn coupled_cost_without_life_leaves_only_the_sacrifice_route() {
    let cost = coupled_cost();
    let (runner, source) = board(
        &cost,
        Spec {
            mana: 1,
            life: 1,
            own_food: false,
            graveyard: 0,
        },
    );
    assert_eq!(
        shapes(&routes_for(runner.state(), source, &cost, Some(0))),
        vec![vec![(0, mana(1)), (1, sacrifice_self())]]
    );
}

// --- E2: agreement with the activation prompt -----------------------------------

/// The parsed Forage cost (`{2}` beside "exile three or sacrifice a Food").
fn camellia_cost() -> AbilityCost {
    let mut scenario = GameScenario::new();
    let camellia = scenario
        .add_creature(P0, "Camellia, the Seedmiser", 3, 3)
        .with_subtypes(vec!["Squirrel", "Warlock"])
        .from_oracle_text(CAMELLIA_ORACLE)
        .id();
    let runner = scenario.build();
    ability_index_with_cost(runner.state(), camellia).1
}

/// CR 601.2h + CR 602.2b + CR 118.3: across a grid of boards, the first legs
/// of the routes include every branch the activation prompt offers, and an
/// activatable ability always has a route.
///
/// Observed relation (pinned exactly below): on this grid the first legs equal
/// the prompt's per-branch answer in every cell. Activation, however, is
/// refused while a route exists in exactly the `shared_creature` cells with
/// mana: there the payability authority's precheck of the still-unsettled
/// whole cost (`is_payable_for_activation` on the root, which treats the
/// choice's sacrifice leg as needing its own creature) fails, although the
/// settled "{1}, sacrifice a creature" route is payable. That root prune is
/// recorded, not endorsed: the enumerator does not reproduce it.
#[test]
fn route_first_legs_cover_the_prompt_branches_across_boards() {
    let costs = [
        ("coupled", coupled_cost()),
        ("camellia", camellia_cost()),
        ("nested", nested_cost()),
        ("two_life_choices", two_life_choices_cost()),
        ("shared_creature", shared_creature_cost()),
    ];
    let mut first_legs_beyond_prompt = Vec::new();
    let mut refused_with_routes = Vec::new();
    for (name, cost) in &costs {
        for mana in [0, 1, 2] {
            for life in [1, 20] {
                for own_food in [false, true] {
                    let spec = Spec {
                        mana,
                        life,
                        own_food,
                        graveyard: 3,
                    };
                    let (runner, source) = board(cost, spec);
                    let state = runner.state();
                    let routes = routes_for(state, source, cost, Some(0));
                    let firsts = first_leg_indices(&routes);
                    let prompt = prompt_indices(state, source, cost, Some(0));
                    assert!(
                        prompt.iter().all(|index| firsts.contains(index)),
                        "{name} {spec:?}: prompt {prompt:?} not within route first legs {firsts:?}"
                    );
                    let activatable = can_activate_ability_now(state, P0, source, 0);
                    assert!(
                        !activatable || !routes.is_empty(),
                        "{name} {spec:?}: activatable but no route"
                    );
                    if firsts != prompt {
                        first_legs_beyond_prompt.push(format!("{name} {spec:?}"));
                    }
                    if !activatable && !routes.is_empty() {
                        assert!(
                            !cost.is_payable_for_activation(state, P0, source, Some(0)),
                            "{name} {spec:?}: refused for a reason other than the root precheck"
                        );
                        refused_with_routes.push(format!("{name} {spec:?}"));
                    }
                    if mana == 2 && life == 20 && own_food {
                        assert!(!routes.is_empty(), "{name}: a friendly board has a route");
                    }
                }
            }
        }
    }
    assert_eq!(first_legs_beyond_prompt, Vec::<String>::new());
    let mut root_pruned = Vec::new();
    for mana in [1, 2] {
        for life in [1, 20] {
            for own_food in [false, true] {
                let spec = Spec {
                    mana,
                    life,
                    own_food,
                    graveyard: 3,
                };
                root_pruned.push(format!("shared_creature {spec:?}"));
            }
        }
    }
    assert_eq!(refused_with_routes, root_pruned);
}

/// CR 118.3: a real Crystal Shard's two `{T}`-bearing branches. Tapped, no
/// branch is a route; untapped, the routes are exactly the branches the prompt
/// offers for the mana available.
#[test]
fn crystal_shard_routes_follow_the_tap_and_mana_state() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let shard = scenario
        .add_artifact_from_oracle(P0, "Crystal Shard", CRYSTAL_SHARD_ORACLE)
        .id();
    scenario.add_creature(P1, "Grizzly Bears", 2, 2);
    scenario.with_mana_pool(P0, colorless(3));
    let mut runner = scenario.build();
    let (index, cost) = ability_index_with_cost(runner.state(), shard);
    let branches = find_one_of_cost(&cost).expect("a choice between costs");
    assert_eq!(branches.len(), 2);
    assert!(
        branches.iter().all(|branch| matches!(
            branch,
            AbilityCost::Composite { costs } if costs.contains(&AbilityCost::Tap)
        )),
        "both Crystal Shard branches tap it: {branches:?}"
    );

    // Three colorless: only "{3}, {T}".
    let routes = routes_for(runner.state(), shard, &cost, Some(index));
    assert_eq!(first_leg_indices(&routes), vec![0]);
    assert_eq!(
        first_leg_indices(&routes),
        prompt_indices(runner.state(), shard, &cost, Some(index))
    );

    // A blue mana as well: both branches.
    let _ = runner.state_mut().add_mana_to_pool(
        P0,
        ManaUnit::new(ManaType::Blue, ObjectId(0), false, vec![]),
    );
    let routes = routes_for(runner.state(), shard, &cost, Some(index));
    assert_eq!(first_leg_indices(&routes), vec![0, 1]);
    assert_eq!(
        first_leg_indices(&routes),
        prompt_indices(runner.state(), shard, &cost, Some(index))
    );

    // Tapped: neither.
    runner.state_mut().objects.get_mut(&shard).unwrap().tapped = true;
    assert!(routes_for(runner.state(), shard, &cost, Some(index)).is_empty());
    assert!(prompt_indices(runner.state(), shard, &cost, Some(index)).is_empty());
}

/// CR 118.3: the mana sources a `{T}` leg excludes are judged per settled
/// cost. The source is the only mana source: tapping it for the mana of the
/// `{T}`-bearing branch is impossible, but the plain `{1}` branch can use it.
#[test]
fn a_tap_branch_and_a_plain_branch_are_scoped_separately() {
    let cost = one_of(vec![all_of(vec![mana(1), AbilityCost::Tap]), mana(1)]);
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let rock = scenario
        .add_artifact_from_oracle(P0, "Mana Rock", "{T}: Add {C}.")
        .with_ability_definition(gain_life_ability(cost.clone()))
        .id();
    let runner = scenario.build();
    let (index, found) = ability_index_with_cost(runner.state(), rock);
    assert_eq!(found, cost);

    let routes = routes_for(runner.state(), rock, &cost, Some(index));
    assert_eq!(shapes(&routes), vec![vec![(1, mana(1))]]);
    assert_eq!(
        first_leg_indices(&routes),
        prompt_indices(runner.state(), rock, &cost, Some(index))
    );
}

// --- E3: Camellia, the Seedmiser ------------------------------------------------

fn camellia_board(
    food_controller: PlayerId,
    graveyard: usize,
    mana: usize,
) -> (GameRunner, ObjectId, usize, AbilityCost) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let camellia = scenario
        .add_creature(P0, "Camellia, the Seedmiser", 3, 3)
        .with_subtypes(vec!["Squirrel", "Warlock"])
        .from_oracle_text(CAMELLIA_ORACLE)
        .id();
    scenario
        .add_creature(food_controller, "Food", 0, 0)
        .as_artifact()
        .with_subtypes(vec!["Food"]);
    let names: Vec<String> = (0..graveyard)
        .map(|index| format!("Graveyard Card {index}"))
        .collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    scenario.with_graveyard(P0, &refs);
    scenario.with_mana_pool(P0, colorless(mana));
    let runner = scenario.build();
    let (index, cost) = ability_index_with_cost(runner.state(), camellia);
    (runner, camellia, index, cost)
}

fn is_graveyard_exile(cost: &AbilityCost) -> bool {
    matches!(
        cost,
        AbilityCost::Exile {
            count: 3,
            zone: Some(Zone::Graveyard),
            ..
        }
    )
}

fn is_sacrifice(cost: &AbilityCost) -> bool {
    matches!(cost, AbilityCost::Sacrifice(_))
}

/// CR 701.61a + CR 601.2h: with `{2}`, three graveyard cards and a Food, both
/// halves of Forage are routes, each carrying its unfiltered branch index.
#[test]
fn camellia_forage_routes_with_both_halves_payable() {
    let (runner, camellia, index, cost) = camellia_board(P0, 3, 2);
    let routes = routes_for(runner.state(), camellia, &cost, Some(index));
    assert_eq!(routes.len(), 2, "{routes:?}");
    assert_eq!(routes[0].legs.len(), 1);
    assert_eq!(routes[0].legs[0].branch_index, 0);
    assert!(is_graveyard_exile(&routes[0].legs[0].branch));
    assert_eq!(routes[1].legs.len(), 1);
    assert_eq!(routes[1].legs[0].branch_index, 1);
    assert!(is_sacrifice(&routes[1].legs[0].branch));
}

/// CR 701.61a + CR 118.3: with two graveyard cards only the Food half pays.
/// Its provenance is branch 1 of the printed choice while the prompt, which
/// lists only payable branches, offers it at position 0.
#[test]
fn camellia_forage_branch_index_is_provenance_not_a_prompt_index() {
    let (mut runner, camellia, index, cost) = camellia_board(P0, 2, 2);
    let routes = routes_for(runner.state(), camellia, &cost, Some(index));
    assert_eq!(routes.len(), 1, "{routes:?}");
    let leg = &routes[0].legs[0];
    assert_eq!(leg.branch_index, 1);
    assert!(is_sacrifice(&leg.branch));

    let result = runner
        .act(GameAction::ActivateAbility {
            source_id: camellia,
            ability_index: index,
        })
        .expect("the Food half is payable");
    let WaitingFor::ActivationCostOneOfChoice { costs, .. } = result.waiting_for else {
        panic!("expected the branch prompt, got {:?}", result.waiting_for);
    };
    assert_eq!(costs.len(), 1, "only the payable branch is offered");
    assert_eq!(costs.iter().position(|c| *c == leg.branch), Some(0));
}

/// CR 601.2h + CR 701.21a: one mana short, or only an opponent's Food, and no
/// route exists.
#[test]
fn camellia_forage_has_no_route_when_the_total_cost_is_unpayable() {
    let (runner, camellia, index, cost) = camellia_board(P0, 3, 1);
    assert!(routes_for(runner.state(), camellia, &cost, Some(index)).is_empty());
    assert!(!can_activate_ability_now(
        runner.state(),
        P0,
        camellia,
        index
    ));

    let (mut runner, camellia, index, cost) = camellia_board(P1, 2, 2);
    assert!(routes_for(runner.state(), camellia, &cost, Some(index)).is_empty());
    assert!(!can_activate_ability_now(
        runner.state(),
        P0,
        camellia,
        index
    ));

    // CR 701.21a: control, not ownership, decides — the opponent's Food under
    // P0's control is sacrificeable, so the Food half becomes a route.
    let food = runner
        .state()
        .battlefield
        .iter()
        .copied()
        .find(|id| runner.state().objects[id].name == "Food")
        .expect("the Food is on the battlefield");
    runner
        .state_mut()
        .objects
        .get_mut(&food)
        .unwrap()
        .controller = P0;
    let routes = routes_for(runner.state(), camellia, &cost, Some(index));
    assert_eq!(routes.len(), 1, "{routes:?}");
    assert!(is_sacrifice(&routes[0].legs[0].branch));
    assert_eq!(routes[0].legs[0].branch_index, 1);
}

// --- E4-E6: shapes ----------------------------------------------------------------

/// A choice nested inside a choice is settled in two prompt-order legs.
#[test]
fn nested_choice_routes_carry_one_leg_per_choice() {
    let cost = nested_cost();
    let (runner, source) = board(
        &cost,
        Spec {
            mana: 1,
            life: 20,
            own_food: false,
            graveyard: 0,
        },
    );
    let routes = routes_for(runner.state(), source, &cost, Some(0));
    let inner = one_of(vec![life(2), mana(1)]);
    assert_eq!(
        shapes(&routes),
        vec![
            vec![(0, inner.clone()), (0, life(2))],
            vec![(0, inner), (1, mana(1))],
        ]
    );
    assert!(routes.iter().all(|route| route.legs.len() == 2));
    assert_eq!(routes[0].resolved, life(2));
}

/// CR 119.4: two life legs that each fit the life total do not fit together.
#[test]
fn life_legs_are_settled_against_one_life_total() {
    let cost = two_life_choices_cost();
    let (runner, source) = board(
        &cost,
        Spec {
            mana: 1,
            life: 20,
            own_food: false,
            graveyard: 0,
        },
    );
    assert_eq!(
        shapes(&routes_for(runner.state(), source, &cost, Some(0))),
        vec![
            vec![(0, life(15)), (1, mana(1))],
            vec![(1, mana(1)), (0, life(10))],
        ]
    );
}

/// CR 118.3 + CR 701.21a: one creature cannot be sacrificed twice.
#[test]
fn two_sacrifices_of_the_only_creature_are_not_a_route() {
    let cost = shared_creature_cost();
    let (runner, source) = board(
        &cost,
        Spec {
            mana: 1,
            life: 20,
            own_food: false,
            graveyard: 0,
        },
    );
    assert_eq!(
        shapes(&routes_for(runner.state(), source, &cost, Some(0))),
        vec![vec![(1, mana(1))]]
    );
}

// --- E7: the enumeration bound ------------------------------------------------------

fn free_choice(branches: usize) -> AbilityCost {
    one_of(vec![free(); branches])
}

/// Exactly `MAX_COST_ROUTE_LEAVES` settled costs is within the bound; one more
/// set of branches is refused rather than truncated.
#[test]
fn route_bound_admits_the_cap_and_refuses_beyond_it() {
    assert_eq!(
        MAX_COST_ROUTE_LEAVES % 16,
        0,
        "fixture divides the cap into 4 x 4 x n"
    );
    let at_cap = all_of(vec![
        free_choice(4),
        free_choice(4),
        free_choice(MAX_COST_ROUTE_LEAVES / 16),
    ]);
    let (runner, source) = board(
        &at_cap,
        Spec {
            mana: 0,
            life: 20,
            own_food: false,
            graveyard: 0,
        },
    );
    let routes = payable_cost_routes(runner.state(), P0, source, &at_cap, Some(0))
        .expect("exactly the cap is within the bound");
    assert_eq!(routes.len(), MAX_COST_ROUTE_LEAVES);

    let beyond = all_of(vec![
        free_choice(4),
        free_choice(4),
        free_choice(MAX_COST_ROUTE_LEAVES / 16 + 1),
    ]);
    assert_eq!(
        payable_cost_routes(runner.state(), P0, source, &beyond, Some(0)),
        Err(CostRouteBudgetExceeded)
    );
}

/// The bound counts settled costs, not top-level branches: two top-level
/// branches whose nested choices together exceed the cap are refused.
#[test]
fn route_bound_counts_nested_choices() {
    let half_plus_one = MAX_COST_ROUTE_LEAVES / 2 + 1;
    let cost = one_of(vec![free_choice(half_plus_one), free_choice(half_plus_one)]);
    let (runner, source) = board(
        &cost,
        Spec {
            mana: 0,
            life: 20,
            own_food: false,
            graveyard: 0,
        },
    );
    const { assert!(2 < MAX_COST_ROUTE_LEAVES, "the top level alone is in bound") };
    assert_eq!(
        payable_cost_routes(runner.state(), P0, source, &cost, Some(0)),
        Err(CostRouteBudgetExceeded)
    );
}

fn mana_choice(branches: usize) -> AbilityCost {
    one_of(vec![mana(1); branches])
}

/// Every settled cost reached counts toward the bound, payable or not. Three
/// one-mana legs per settled cost: with three mana each settled cost is a
/// route (the reach guard), with none no settled cost is. Exactly the cap of
/// unpayable settled costs is an empty answer; one more set of branches is
/// refused although none of them would be kept.
#[test]
fn route_bound_counts_unpayable_settled_costs() {
    assert_eq!(
        MAX_COST_ROUTE_LEAVES % 16,
        0,
        "fixture divides the cap into 4 x 4 x n"
    );
    let cost_with = |last: usize| all_of(vec![mana_choice(4), mana_choice(4), mana_choice(last)]);
    let at_cap = cost_with(MAX_COST_ROUTE_LEAVES / 16);
    let beyond = cost_with(MAX_COST_ROUTE_LEAVES / 16 + 1);
    let spec = |mana| Spec {
        mana,
        life: 20,
        own_food: false,
        graveyard: 0,
    };

    let (runner, source) = board(&at_cap, spec(3));
    assert_eq!(
        payable_cost_routes(runner.state(), P0, source, &at_cap, Some(0))
            .expect("exactly the cap is within the bound")
            .len(),
        MAX_COST_ROUTE_LEAVES,
        "reach guard: with three mana every settled cost is a route"
    );

    let (runner, source) = board(&at_cap, spec(0));
    assert_eq!(
        payable_cost_routes(runner.state(), P0, source, &at_cap, Some(0)),
        Ok(Vec::new()),
        "the cap of unpayable settled costs is within the bound and keeps none"
    );
    assert_eq!(
        payable_cost_routes(runner.state(), P0, source, &beyond, Some(0)),
        Err(CostRouteBudgetExceeded),
        "unpayable settled costs count toward the bound"
    );
}

// --- E8: degenerate costs -------------------------------------------------------------

/// A cost with no choice is one route with no legs when payable and none when
/// not; an empty choice has no route.
#[test]
fn costs_without_a_real_choice() {
    let cost = mana(1);
    let (runner, source) = board(
        &cost,
        Spec {
            mana: 1,
            life: 20,
            own_food: false,
            graveyard: 0,
        },
    );
    assert_eq!(
        routes_for(runner.state(), source, &cost, Some(0)),
        vec![PayableCostRoute {
            legs: Vec::new(),
            resolved: mana(1),
        }]
    );
    let (runner, source) = board(
        &cost,
        Spec {
            mana: 0,
            life: 20,
            own_food: false,
            graveyard: 0,
        },
    );
    assert!(routes_for(runner.state(), source, &cost, Some(0)).is_empty());

    let empty = one_of(Vec::new());
    assert!(routes_for(runner.state(), source, &empty, Some(0)).is_empty());
}

// --- E9: the ability index is passed through ---------------------------------------------

/// CR 118.3: a granted ability whose choice sacrifices the object that granted
/// it. With the ability's index the granter is known and the sacrifice is a
/// route; without one it is not. Each answer matches the prompt reference
/// computed with the same index, so `None` is never coerced to an index.
#[test]
fn granted_ability_routes_follow_the_ability_index() {
    let cost = one_of(vec![
        AbilityCost::Sacrifice(SacrificeCost::count(
            TargetFilter::GrantingObject { bound: None },
            1,
        )),
        mana(3),
    ]);
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let host = scenario
        .add_creature(P0, "Host", 2, 2)
        .with_ability_definition(gain_life_ability(cost.clone()))
        .id();
    let granter = scenario
        .add_creature(P0, "Granting Relic", 0, 0)
        .as_artifact()
        .id();
    let mut runner = scenario.build();
    let stamp = ObjectIncarnationRef::from_object(&runner.state().objects[&granter]);
    Arc::make_mut(&mut runner.state_mut().objects.get_mut(&host).unwrap().abilities)[0]
        .granting_object = Some(stamp);
    let state = runner.state();

    let with_index = routes_for(state, host, &cost, Some(0));
    assert_eq!(
        first_leg_indices(&with_index),
        prompt_indices(state, host, &cost, Some(0))
    );
    assert_eq!(first_leg_indices(&with_index), vec![0]);

    let without_index = routes_for(state, host, &cost, None);
    assert_eq!(
        first_leg_indices(&without_index),
        prompt_indices(state, host, &cost, None)
    );
    assert!(without_index.is_empty());
}
