---
features:
  - "Themes"
---
![[settings-appearance.png]]

Settings → Appearance → **Theme** sets the colours of the whole window: the sidebars, the editor,
the reading view, dialogs, the graph and code. Shale comes with nine themes, and a change shows at once.

- Dark: **Shale** (the default), **Dracula**, **Nord**, **Catppuccin Mocha**, **Gruvbox Dark**.
- Light: **Catppuccin Latte**, **Solarized Light**, **GitHub Light**, **Gruvbox Light**.

![[theme-latte.png]]

*(The window in Catppuccin Latte.)*

# Follow the system's light or dark mode

Tick **Follow the system's light or dark mode** and the one **Theme** list becomes two: **Light theme**
(Catppuccin Latte at first) and **Dark theme** (Shale). Shale uses the one that matches the mode of your
computer and changes by itself when the mode changes. If Shale cannot tell the mode, it uses the dark theme.

# Your own themes

A theme is a terminal colour scheme in the format of [Alacritty](https://alacritty.org). Shale reads
those files as they are, so the whole collection at
<https://github.com/alacritty/alacritty-theme> works: download a `.toml` file, press **Open themes
folder** in Settings → Appearance and put it there. The folder is `themes` next to your settings file
(`~/.config/shale/themes/` on Linux). Every `*.toml` file in it is a theme named like the file
(`Tokyo Night.toml` is "Tokyo Night"), listed with "(yours)" after the built-in ones. Press **Reload
themes** after you changed a file; the folder is read again whenever you open the Appearance tab, too.
A file with the name of a built-in theme (`Dracula.toml`) replaces it, which is the way to tweak one.

Shale needs these parts of the file and ignores the rest:

```toml
[colors.primary]
background = '#282a36'
foreground = '#f8f8f2'

[colors.normal]    # black red green yellow blue magenta cyan white, all eight
black = '#000000'
# ...

[colors.bright]    # optional; a colour left out is the normal one
# ...

[colors.selection] # optional
background = '#44475a'
```

Colours are written `'#rrggbb'` or `'0xrrggbb'`. Shale makes the grey levels of the window from the
background and the text colour, takes the accent from magenta, links from blue, problems from red and
code from the other colours, and makes every pair of colours that the window draws easy to read: a colour that is hard to read on
its background is moved toward white or black until its contrast is 4.5 to 1 (3 to 1 for the accent and
the greys of marks), and a tinted background (a selected row, a hovered button, a banner) gets less tint
until the text reads on it. In a selected row the text is the accent when that reads, else the text
colour. The details are in the specification (CFG-064, CFG-082).

Colours you set in the `[shale]` table below are not corrected, but the window still draws the text of
selected and hovered rows in a colour that reads.

A file that cannot be read is skipped, and the reason is listed under the theme list ("Theme file
skipped: Mine.toml: [colors.normal] is missing `red`").

## Set a colour exactly: the `[shale]` table

If the colours that Shale makes are not what you want, add a table `[shale]` to the file. Its keys are
the names of the colours of the window, and the value is the colour, written as above (with `#rrggbbaa`
you can add an opacity). Those colours are used exactly as you wrote them. `dark = true` or `false`
says whether the theme counts as dark (Shale decides by the background).

```toml
[shale]
accent = '#ff79c6'
syntax_comment = '#6272a4'
selection_bg = '#44475a'
dark = true
```

The names: `bg_primary`, `bg_secondary`, `bg_ribbon`, `bg_hover`, `bg_active`, `border`, `text`,
`text_strong`, `muted`, `faint`, `accent`, `link`, `link_missing`, `tag`, `tag_bg`, `code_bg`, `code`,
`heading`, `done`, `quote`, `syntax_keyword`, `syntax_string`, `syntax_number`, `syntax_comment`,
`syntax_function`, `syntax_type`, `syntax_operator`, `faint_bg`, `selection_bg`, `widget_bg`,
`widget_weak_bg`, `hover_stroke`, `highlight_bg`, `banner_error_bg`, `banner_notice_bg`, `ok_text`,
`good`, `good_bg`, `warn`, `warn_bg`, `bad_bg`, `graph_edge_dim`, `graph_edge`, `graph_node` and
`graph_node_drawing`. A name that is not on the list is an error of the file.

# What a theme does not change

Colours you set in a note yourself (highlights and font colours, `<mark>` and `<span>`) stay as
written. The drawing canvas has its own switch ("Dark drawing canvas" in Appearance). The PDF export
always prints with Shale's own colours.

# In the settings file

The choice is the `[theme]` table of the settings file (`name`, `follow_system`, `light`, `dark`), see
[[Settings reference]]; changing it there applies at once, too.

# Licences

The built-in themes are colour schemes of other people, taken from the alacritty-theme collection:
Dracula, Nord, Catppuccin, Gruvbox, Solarized and GitHub (Primer). They are used under their MIT
licences; the authors and the licence text are in `assets/themes/LICENSES.md` of Shale's source.

Back to [[Start here]]; the fonts are in [[Fonts]].

#userguide #settings
