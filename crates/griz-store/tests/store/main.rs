//! Behavior of the durable store: apply, undo, receipts, and recovery.

mod apply;
mod common;
mod receipts;
mod recovery;
mod restore;
mod selection;
mod selection_filters;
mod selection_throughput;
mod undo;
