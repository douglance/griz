//! Behavior of the durable store: apply, undo, receipts, and recovery.

mod apply;
mod common;
mod history_pagination;
mod history_throughput;
mod journal_order;
mod migration;
mod receipts;
mod recovery;
mod restore;
mod restore_boundary;
mod selection;
mod selection_filters;
mod selection_throughput;
mod undo;
