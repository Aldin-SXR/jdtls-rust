//! Port of the jdt.ls completion ranking contribution API:
//! `CompletionRanking`, `ICompletionRankingProvider`,
//! `CompletionContributionService` and `CompletionRankingAggregation`.
//!
//! Ranking providers rank the proposals of a completion request; their
//! scores raise the proposals' relevance, their decorators prefix the item
//! labels and their data is handed back to them when an item is selected
//! (`java.completion.onDidSelect`).

use super::item::Item;
use super::proposal::{Context, Proposal};
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

/// `CompletionRanking.MAX_SCORE`.
pub const MAX_SCORE: i32 = 100;
/// `CompletionRanking.MIN_SCORE`.
pub const MIN_SCORE: i32 = 0;
/// `CompletionRanking.COMPLETION_EXECUTION_TIME`.
pub const COMPLETION_EXECUTION_TIME: &str = "COMPLETION_EXECUTION_TIME";

/// `CompletionRanking`: one provider's ranking of one proposal.
#[derive(Debug, Clone, Default)]
pub struct CompletionRanking {
    pub score: i32,
    /// `'\0'` for none.
    pub decorator: char,
    pub data: Option<HashMap<String, String>>,
}

/// `ICompletionRankingProvider`.
pub trait CompletionRankingProvider: Send + Sync {
    /// `rank(proposals, context, unit, monitor)`: one entry per proposal, or
    /// `None` (as is a result of another length) to contribute nothing.
    fn rank(&self, proposals: &[Proposal], context: &Context, uri: &str) -> Option<Vec<Option<CompletionRanking>>>;
    /// `onDidCompletionItemSelect(item)`.
    fn on_did_completion_item_select(&self, item: &Item);
}

static PROVIDERS: Mutex<Vec<Arc<dyn CompletionRankingProvider>>> = Mutex::new(Vec::new());

/// `CompletionContributionService.registerRankingProvider`.
#[allow(dead_code)]
pub fn register_ranking_provider(provider: Arc<dyn CompletionRankingProvider>) {
    let mut providers = PROVIDERS.lock().unwrap_or_else(|e| e.into_inner());
    if providers.iter().any(|p| Arc::ptr_eq(p, &provider)) {
        return;
    }
    providers.push(provider);
}

/// `CompletionContributionService.unregisterRankingProvider`.
#[allow(dead_code)]
pub fn unregister_ranking_provider(provider: &Arc<dyn CompletionRankingProvider>) {
    PROVIDERS.lock().unwrap_or_else(|e| e.into_inner()).retain(|p| !Arc::ptr_eq(p, provider));
}

/// `CompletionContributionService.getRankingProviders`.
pub fn ranking_providers() -> Vec<Arc<dyn CompletionRankingProvider>> {
    PROVIDERS.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// `CompletionRankingAggregation`: the rankings of all providers for one proposal.
#[derive(Debug, Clone, Default)]
pub struct CompletionRankingAggregation {
    score: i32,
    decorators: BTreeSet<char>,
    data: HashMap<String, String>,
}

impl CompletionRankingAggregation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn score(&self) -> i32 {
        self.score
    }

    pub fn add_score(&mut self, score: i32) {
        if score <= MIN_SCORE {
            return;
        }
        self.score += if score > MAX_SCORE { MAX_SCORE } else { score };
    }

    /// The decorators in ascending order.
    pub fn decorators(&self) -> String {
        self.decorators.iter().collect()
    }

    pub fn add_decorator(&mut self, decorator: char) {
        if decorator != '\0' {
            self.decorators.insert(decorator);
        }
    }

    pub fn data(&self) -> &HashMap<String, String> {
        &self.data
    }

    pub fn add_data(&mut self, data: Option<&HashMap<String, String>>) {
        if let Some(data) = data {
            for (key, value) in data {
                self.data.insert(key.clone(), value.clone());
            }
        }
    }
}

/// `CompletionProposalRequestor.getAggregatedRankingResult`.
pub fn aggregated_ranking_result(proposals: &[Proposal], context: &Context, uri: &str) -> Vec<Option<CompletionRankingAggregation>> {
    let mut result_combination: Vec<Option<CompletionRankingAggregation>> = vec![None; proposals.len()];
    for provider in ranking_providers() {
        let Some(results) = provider.rank(proposals, context, uri) else { continue };
        if results.len() != proposals.len() {
            continue;
        }
        for (i, result) in results.iter().enumerate() {
            let Some(result) = result else { continue };
            let aggregation = result_combination[i].get_or_insert_with(CompletionRankingAggregation::new);
            aggregation.add_score(result.score);
            aggregation.add_decorator(result.decorator);
            aggregation.add_data(result.data.as_ref());
        }
    }
    result_combination
}

/// Port of `org.eclipse.jdt.ls.core.internal.handlers.CompletionRankingAggregationTest`.
#[cfg(test)]
mod completion_ranking_aggregation_test {
    use super::*;

    #[test]
    fn test_add_null_data() {
        let mut aggregation = CompletionRankingAggregation::new();
        aggregation.add_data(None);
        let data = aggregation.data();
        assert!(data.is_empty());
    }

    #[test]
    fn test_add_data() {
        let mut aggregation = CompletionRankingAggregation::new();
        let mut data = HashMap::new();
        data.insert("foo".to_owned(), "bar".to_owned());
        aggregation.add_data(Some(&data));
        let aggregated_data = aggregation.data();
        assert_eq!("bar", aggregated_data["foo"]);
    }

    #[test]
    fn test_add_decorator() {
        let mut aggregation = CompletionRankingAggregation::new();
        aggregation.add_decorator('★');
        aggregation.add_decorator('a');
        assert_eq!("a★", aggregation.decorators());
    }

    #[test]
    fn test_add_score() {
        let mut aggregation = CompletionRankingAggregation::new();
        aggregation.add_score(-1);
        aggregation.add_score(i32::MAX);
        assert_eq!(MAX_SCORE, aggregation.score());
    }
}
