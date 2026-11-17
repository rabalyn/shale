//! PDF export through LaTeX templates (ADR-0019, TEX): finding a LuaLaTeX (`engine`), the managed
//! TeX Live (`install`), compiling with LuaLaTeX and biber (`compile`) and the Settings → LaTeX page
//! (`settings_ui`), the template sets in the vault (`sets`, `import`, `archive`, `sets_ui`). The
//! writer and the dialog come with #281 and #282; the test stack is `tests/tex-env/`.

mod archive;
pub mod compile;
pub mod engine;
pub mod import;
pub mod install;
pub(crate) mod proc;
pub mod sets;
pub mod sets_ui;
pub mod settings_ui;

pub use engine::TexConfig;

#[cfg(test)]
mod archive_tests;
#[cfg(test)]
mod compile_tests;
#[cfg(test)]
mod engine_tests;
#[cfg(test)]
mod env_tests;
#[cfg(test)]
mod import_tests;
#[cfg(test)]
mod install_tests;
#[cfg(test)]
mod sets_env_tests;
#[cfg(test)]
mod sets_tests;
#[cfg(test)]
mod sets_ui_tests;
#[cfg(test)]
mod settings_ui_tests;
