/// Output types for the orch parse pipeline.
///
/// These types represent the final, resolved Orchfile structure that
/// `orch parse` serializes to JSON. Consumers (e.g. orchd) can depend
/// on this crate and deserialize directly into these types.
pub mod types;
