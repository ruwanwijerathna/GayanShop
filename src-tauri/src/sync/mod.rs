//! Cloud sync (ARCHITECTURE §1.3, §6). The local database stays the source of truth for the till: a sale
//! never waits on the network. This module registers the counter with the server; a background worker pushes
//! the outbox and applies what other counters wrote. It talks to the server only over plain HTTP + JSON
//! (PostgREST RPCs and GoTrue), so a self-hosted server is a change of configuration.

pub mod api;
pub mod apply;
pub mod config;
pub mod credentials;
pub mod engine;
pub mod registration;
pub mod wire;
pub mod worker;
