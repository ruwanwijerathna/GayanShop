//! Cloud sync (ARCHITECTURE §1.3, §6). The local database stays the source of truth for the till: a sale
//! never waits on the network. This module registers the counter with the server, and talks to it only over
//! plain HTTP + JSON (PostgREST RPCs and GoTrue), so a self-hosted server is a change of configuration.

pub mod api;
pub mod config;
pub mod credentials;
pub mod registration;
pub mod wire;
