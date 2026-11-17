//! PDF export through LaTeX templates (ADR-0019, TEX): finding a LuaLaTeX (`engine`), the managed
//! TeX Live (`install`), compiling with LuaLaTeX and biber (`compile`) and the Settings → LaTeX page
//! (`settings_ui`). The template sets, the writer and the dialog come with #280 to #282; the
//! test stack is `tests/tex-env/`.

pub mod compile;
pub mod engine;
pub mod install;
mod proc;
pub mod settings_ui;

pub use engine::TexConfig;

#[cfg(test)]
mod compile_tests;
#[cfg(test)]
mod engine_tests;
#[cfg(test)]
mod env_tests;
#[cfg(test)]
mod install_tests;
#[cfg(test)]
mod settings_ui_tests;
