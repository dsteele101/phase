//! Combination search over priced reach sources.
//!
//! The question is joint, not per-spell: which SUBSET of the available sources
//! takes the opponent to 0 within the mana on hand? Evaluating spells one at a
//! time in hand order is exactly how "Seal + Seal" (4 damage) beats
//! "Seal + Flame Rift" (6 damage) — each Seal is cheaper in isolation.

use super::sources::PricedSource;
use super::LineStep;

/// Above this many sources only the most mana-efficient are searched; the
/// subset enumeration is `2^n`.
const MAX_SEARCHED_SOURCES: usize = 14;

/// What a line has to work with, read from the state the sources were priced
/// in.
pub(super) struct Budget {
    pub(super) opponent_life: u32,
    pub(super) controller_life: u32,
    pub(super) mana: u32,
}

/// Every combination of `sources` that meets the opponent's life total while
/// the AI survives, cheapest first: least mana, then least life paid, then
/// fewest cards. Mana is checked only as a total here — colours, restrictions
/// and real payment are the reducer's to certify.
///
/// At most one source in a combination may scale with X; X is sized to the
/// mana the rest of the combination leaves over (CR 107.3a).
pub(super) fn lethal_combinations(sources: &[PricedSource], budget: &Budget) -> Vec<Vec<usize>> {
    let searched = searched_indices(sources, budget.mana);
    let ceiling: u32 = searched
        .iter()
        .map(|&index| sources[index].loss.opponent.at(budget.mana))
        .sum();
    if ceiling < budget.opponent_life {
        return Vec::new();
    }

    let mut lethal: Vec<(u32, u32, u32, u32, Vec<usize>)> = Vec::new();
    for mask in 1u32..(1u32 << searched.len()) {
        let members: Vec<usize> = searched
            .iter()
            .enumerate()
            .filter(|(bit, _)| mask & (1 << bit) != 0)
            .map(|(_, &index)| index)
            .collect();
        let mut scaling = members
            .iter()
            .filter(|&&index| sources[index].loss.opponent.per_x > 0);
        let x_member = scaling.next();
        if scaling.next().is_some() {
            continue;
        }
        let mana_value: u32 = members
            .iter()
            .map(|&index| sources[index].mana.mana_value())
            .sum();
        if mana_value > budget.mana {
            continue;
        }
        let x = x_member.map_or(0, |&index| {
            (budget.mana - mana_value) / sources[index].x_shards().max(1)
        });
        let damage: u32 = members
            .iter()
            .map(|&index| sources[index].loss.opponent.at(x))
            .sum();
        let life_paid: u32 = members
            .iter()
            .map(|&index| sources[index].loss.controller.at(x))
            .sum();
        // CR 104.4a: if both players hit 0 together the game is a draw, so the
        // AI must stay above 0 for the line to win.
        if damage >= budget.opponent_life && life_paid < budget.controller_life {
            lethal.push((mana_value, life_paid, members.len() as u32, mask, members));
        }
    }
    lethal.sort_by_key(|(mana_value, life_paid, count, mask, _)| {
        (*mana_value, *life_paid, *count, *mask)
    });
    lethal.into_iter().map(|(.., members)| members).collect()
}

/// The order a combination is played in: each source's first step by
/// [`super::sources::StepOrder`], then the follow-up activations of permanents
/// those steps cast, with an X-scaling source last of all so it absorbs every
/// mana the rest leave over.
pub(super) fn line_steps(sources: &[PricedSource], members: &[usize]) -> Vec<LineStep> {
    let mut ordered: Vec<&PricedSource> = members.iter().map(|&index| &sources[index]).collect();
    ordered.sort_by_key(|source| source.order);
    let (scaling, fixed): (Vec<&PricedSource>, Vec<&PricedSource>) = ordered
        .into_iter()
        .partition(|source| source.loss.opponent.per_x > 0);
    let first_steps = fixed.iter().filter_map(|source| source.steps.first());
    let follow_ups = fixed.iter().flat_map(|source| source.steps.iter().skip(1));
    let scaling_steps = scaling.iter().flat_map(|source| source.steps.iter());
    first_steps
        .chain(follow_ups)
        .chain(scaling_steps)
        .cloned()
        .collect()
}

/// The sources the subset search covers: all of them when few, otherwise the
/// most damage per mana.
fn searched_indices(sources: &[PricedSource], mana: u32) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..sources.len()).collect();
    if indices.len() > MAX_SEARCHED_SOURCES {
        indices.sort_by(|&left, &right| {
            let efficiency = |index: usize| {
                let source = &sources[index];
                let damage = source.loss.opponent.at(mana) as f64;
                damage / (source.mana.mana_value() + 1) as f64
            };
            efficiency(right)
                .partial_cmp(&efficiency(left))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.cmp(&right))
        });
        indices.truncate(MAX_SEARCHED_SOURCES);
        indices.sort_unstable();
    }
    indices
}
