pub mod normalize;
pub mod scorer;
pub mod types;
pub mod pipeline;
pub mod store;

pub use pipeline::MatchPipeline;
pub use store::PairStore;
pub use types::{MatchCandidate, MatchDecision, MatchScore};
