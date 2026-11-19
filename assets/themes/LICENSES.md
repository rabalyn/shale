# Bundled themes

The built-in colour themes of Shale (CFG-062). Each `.toml` file here is a terminal colour scheme in
the Alacritty format (CFG-063), taken unchanged from the repository
[alacritty/alacritty-theme](https://github.com/alacritty/alacritty-theme) (`themes/<file>.toml`,
master, November 2026), with one exception named below. Only the colours are used. The theme
**Shale** is made in code (`Palette::shale()`), not by a file, and is part of Shale (MIT).

The files of the alacritty-theme repository are licensed under the **Apache License 2.0**
(<https://www.apache.org/licenses/LICENSE-2.0>), Copyright 2019-2023 indrajit, Copyright 2023- The
Alacritty Project. The colour schemes they carry are the work of the authors below, who license them
under the MIT License; its text follows the list.

| Theme | File | Colour scheme by | Licence of the scheme | Source of the scheme |
|---|---|---|---|---|
| Dracula | `dracula.toml` | Zeno Rocha and the Dracula Theme contributors | MIT, Copyright (c) 2018 Dracula Theme | <https://github.com/dracula/alacritty> |
| Nord | `nord.toml` | Arctic Ice Studio, Sven Greb | MIT, Copyright (c) 2016-present Sven Greb | <https://github.com/nordtheme/nord> |
| Catppuccin Mocha | `catppuccin_mocha.toml` | The Catppuccin Org | MIT, Copyright (c) 2021 Catppuccin | <https://github.com/catppuccin/alacritty> |
| Catppuccin Latte | `catppuccin_latte.toml` | The Catppuccin Org | MIT, Copyright (c) 2021 Catppuccin | <https://github.com/catppuccin/alacritty> |
| Gruvbox Dark | `gruvbox_dark.toml` | Pavel Pertsev (morhetz) | MIT/X11, as stated in its README | <https://github.com/morhetz/gruvbox> |
| Gruvbox Light | `gruvbox_light.toml` | Pavel Pertsev (morhetz) | MIT/X11, as stated in its README | <https://github.com/morhetz/gruvbox> |
| Solarized Light | `solarized_light.toml` | Ethan Schoonover | MIT, Copyright (c) 2011 Ethan Schoonover | <https://github.com/altercation/solarized> |
| GitHub Light | `github_light.toml` | GitHub (Primer) | MIT, Copyright (c) 2020 Primer | <https://github.com/primer/github-vscode-theme> |

`solarized_light.toml` has one change: a `[shale]` table at its end that sets the colour of comments
(Solarized's bright black is almost black, which would make comments as strong as text). Nothing
else of the files was changed.

## The MIT License

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and
associated documentation files (the "Software"), to deal in the Software without restriction,
including without limitation the rights to use, copy, modify, merge, publish, distribute,
sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or
substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT
NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES
OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
