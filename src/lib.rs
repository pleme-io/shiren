//! Shiren (試練) — test runner framework for Neovim with language-specific adapters
//!
//! Part of the blnvim-ng distribution — a Rust-native Neovim plugin suite.
//! Built with [`nvim-oxi`](https://github.com/noib3/nvim-oxi) for zero-cost
//! Neovim API bindings.

use nvim_oxi as oxi;

#[oxi::plugin]
fn shiren() -> oxi::Result<()> {
    Ok(())
}
