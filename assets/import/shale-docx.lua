-- Shale's docx import filter (IMP-037): turns pandoc's document into the Markdown Shale shows.
local function raw(s) return pandoc.RawInline('gfm', s) end

-- the vault-relative folder the pictures are extracted to (metadata `shale-media`, '/' separated)
local media = ''
-- pictures become Shale embeds with their vault-relative path
local function image(i)
  local src = i.src:gsub('^%./', '')
  if media ~= '' and not src:match('^%a+:') then src = media .. '/' .. src end
  return raw('![[' .. src .. ']]')
end
-- a picture with a caption: the picture, then the caption in italics
local function figure(f)
  local out = pandoc.Blocks(f.content)
  local cap = pandoc.utils.blocks_to_inlines(f.caption.long or {})
  if #cap > 0 then out:insert(pandoc.Para({pandoc.Emph(cap)})) end
  return out
end
-- headings that were bold in Word: plain heading text
local function header(h) h.content = h.content:walk({Strong = function(s) return s.content end}); return h end
-- highlight → Shale's ==mark==; small caps and other spans → their text
local function span(s)
  if s.classes:includes('mark') then local r = pandoc.Inlines({raw('==')}); r:extend(s.content); r:insert(raw('==')); return r end
  return s.content
end
local function div(d) return d.content end
-- links to places inside the Word file (table of contents, bookmarks) → their text
local function link(l) if l.target:sub(1, 1) == '#' then return l.content end end
-- math in Obsidian's form
local function math(m)
  if m.mathtype == 'DisplayMath' then return raw('$$' .. m.text .. '$$') end
  return raw('$' .. m.text .. '$')
end
-- tables with merged cells or several blocks in a cell: one line per cell, no spans, so the
-- table stays a pipe table (Shale's table editor) instead of HTML
local function flat(blocks)
  local out = pandoc.Inlines({})
  for idx, b in ipairs(blocks) do
    if idx > 1 then out:insert(raw('<br>')) end
    if b.t == 'BulletList' or b.t == 'OrderedList' then
      for j, item in ipairs(b.content) do
        if j > 1 then out:insert(raw('<br>')) end
        out:insert(pandoc.Str(b.t == 'BulletList' and '•' or (tostring(j) .. '.'))); out:insert(pandoc.Space())
        out:extend(pandoc.utils.blocks_to_inlines(item))
      end
    else out:extend(pandoc.utils.blocks_to_inlines({b}, {raw('<br>')})) end
  end
  return out
end
local function tbl(t)
  local function fix(rows)
    -- how many more rows a cell from above still covers, by column
    local covered = {}
    for _, row in ipairs(rows) do
      local cells = pandoc.List({})
      local col = 1
      local function fill()
        while (covered[col] or 0) > 0 do
          cells:insert(pandoc.Cell({})); covered[col] = covered[col] - 1; col = col + 1
        end
      end
      for _, c in ipairs(row.cells) do
        fill()
        local cols, rows_covered = c.col_span, c.row_span
        c.contents = {pandoc.Plain(flat(c.contents))}; c.col_span = 1; c.row_span = 1
        cells:insert(c)
        for k = 0, cols - 1 do
          if rows_covered > 1 then covered[col + k] = rows_covered - 1 end
        end
        col = col + 1
        for _ = 2, cols do cells:insert(pandoc.Cell({})); col = col + 1 end
      end
      fill()
      row.cells = cells
    end
  end
  fix(t.head.rows); for _, b in ipairs(t.bodies) do fix(b.body); fix(b.head) end; fix(t.foot.rows)
  t.colspecs = t.colspecs:map(function(cs) return {cs[1], nil} end)
  return t
end

-- a line break inside a paragraph: a plain new line (written with --wrap=preserve)
local function linebreak() return pandoc.SoftBreak() end
local function smallcaps(s) return s.content end
-- a definition list: the term in bold, then its definitions
local function deflist(dl)
  local out = pandoc.Blocks({})
  for _, item in ipairs(dl.content) do
    out:insert(pandoc.Para({pandoc.Strong(item[1])}))
    for _, d in ipairs(item[2]) do out:extend(d) end
  end
  return out
end
-- Word's Title and Subtitle styles end up in the metadata, which Markdown without a header drops:
-- the title becomes a `#` heading and the subtitle an italic paragraph below it (author and date are left out)
local function title(doc)
  local front = pandoc.Blocks({})
  local function inlines(m)
    if pandoc.utils.type(m) == 'Inlines' then return m end
    return pandoc.Inlines({pandoc.Str(pandoc.utils.stringify(m))})
  end
  if doc.meta.title then front:insert(pandoc.Header(1, inlines(doc.meta.title))) end
  if doc.meta.subtitle then front:insert(pandoc.Para({pandoc.Emph(inlines(doc.meta.subtitle))})) end
  if #front == 0 then return nil end
  front:extend(doc.blocks)
  doc.blocks = front
  return doc
end
return {
  {Meta = function(m) if m['shale-media'] then media = pandoc.utils.stringify(m['shale-media']) end end},
  {Figure = figure, Table = tbl, DefinitionList = deflist},
  {Image = image, Header = header, Span = span, SmallCaps = smallcaps, Div = div, Link = link, Math = math, LineBreak = linebreak},
  {Pandoc = title},
}
