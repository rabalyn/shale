//! The Hugo config and the Shale theme that every exported tree carries (SITE-046, SITE-047,
//! SITE-070): the sources are files below `assets/site/`, built into the program. The theme is
//! vendored into the tree (`themes/shale/`), so the server needs only `hugo` and `pagefind` and
//! downloads no Hugo module.

/// The files of the theme: their path in the tree and their text.
const THEME: [(&str, &str); 16] = [
    ("themes/shale/theme.toml", include_str!("../../assets/site/theme/theme.toml")),
    ("themes/shale/layouts/index.html", include_str!("../../assets/site/theme/layouts/index.html")),
    ("themes/shale/layouts/index.graph.html", include_str!("../../assets/site/theme/layouts/index.graph.html")),
    ("themes/shale/layouts/_default/baseof.html", include_str!("../../assets/site/theme/layouts/_default/baseof.html")),
    ("themes/shale/layouts/_default/single.html", include_str!("../../assets/site/theme/layouts/_default/single.html")),
    ("themes/shale/layouts/_default/list.html", include_str!("../../assets/site/theme/layouts/_default/list.html")),
    ("themes/shale/layouts/_default/_markup/render-codeblock-math.html", include_str!("../../assets/site/theme/layouts/_default/_markup/render-codeblock-math.html")),
    ("themes/shale/layouts/_default/_markup/render-image.html", include_str!("../../assets/site/theme/layouts/_default/_markup/render-image.html")),
    ("themes/shale/layouts/_default/_markup/render-link.html", include_str!("../../assets/site/theme/layouts/_default/_markup/render-link.html")),
    ("themes/shale/layouts/_default/_markup/render-passthrough.html", include_str!("../../assets/site/theme/layouts/_default/_markup/render-passthrough.html")),
    ("themes/shale/layouts/partials/backlinks.html", include_str!("../../assets/site/theme/layouts/partials/backlinks.html")),
    ("themes/shale/layouts/partials/search.html", include_str!("../../assets/site/theme/layouts/partials/search.html")),
    ("themes/shale/layouts/partials/type-header.html", include_str!("../../assets/site/theme/layouts/partials/type-header.html")),
    ("themes/shale/static/shale-site.css", include_str!("../../assets/site/theme/static/shale-site.css")),
    ("themes/shale/static/shale-graph.js", include_str!("../../assets/site/theme/static/shale-graph.js")),
    ("hugo.toml", include_str!("../../assets/site/hugo.toml")),
];

/// The config and the theme of a site called `site_title`: (path, text). The config names the site
/// first (a TOML string), then holds the fixed settings of `assets/site/hugo.toml`.
pub fn files(site_title: &str) -> Vec<(String, String)> {
    THEME
        .iter()
        .map(|(path, text)| {
            let text = if *path == "hugo.toml" { format!("title = {}\n{text}", toml::Value::String(site_title.to_string())) } else { text.to_string() };
            (path.to_string(), text)
        })
        .collect()
}

/// The paths of the files of the theme and the config (the files below `assets/site/`, in the tree).
#[cfg(test)]
pub fn paths() -> Vec<&'static str> {
    THEME.iter().map(|(p, _)| *p).collect()
}
