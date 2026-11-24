//! A vault as a Hugo site (SITE, #630): the notes the publishing rules choose are written as a
//! complete Hugo tree (content, data files, config and the Shale theme) that `hugo` and `pagefind`
//! build into a wiki. Nothing of a note that is not chosen is ever written (SITE-010).
//!
//! The folder has one small module per concern: `rules` (which notes), `slug` (their URLs), `svg` (a drawing as a picture),
//! `anchor` (Hugo's heading ids), `front` (front matter), `body` (a note's text: links, embeds,
//! math), `plan` (everything that is written), `write` (the atomic write and the manifest), `theme`
//! (the vendored Hugo theme), `publish` (the push to the publish remote, SITE-062…SITE-076) and `cli`
//! (`shale export-site`).

pub(crate) mod anchor;
pub(crate) mod body;
pub(crate) mod cli;
pub(crate) mod front;
pub(crate) mod job;
pub(crate) mod plan;
pub(crate) mod publish;
pub(crate) mod rules;
pub(crate) mod slug;
pub(crate) mod svg;
pub(crate) mod theme;
pub(crate) mod write;

#[cfg(test)]
mod cli_tests;
#[cfg(test)]
mod drawing_tests;
#[cfg(test)]
mod export_tests;
#[cfg(test)]
mod hugo_phase2_tests;
#[cfg(test)]
mod hugo_tests;
#[cfg(test)]
mod svg_tests;
#[cfg(test)]
mod test_support;
#[cfg(all(test, unix))]
mod ui_tests;
