pub mod category;
pub mod normalize;
pub mod scorer;
pub mod types;
pub mod pipeline;
pub mod store;

pub use category::MarketCategory;
pub use pipeline::{DiagnosticComparison, DiagnosticResult, MatchPipeline};
pub use store::PairStore;
pub use types::{MatchCandidate, MatchDecision, MatchScore};
