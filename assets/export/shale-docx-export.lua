-- Shale's Word export filter (DOCX-004 to DOCX-006): turns the Markdown of a note, as pandoc's commonmark reader
-- sees it, back into the Word formatting the docx import (IMP-037 to IMP-046) made Markdown of.
-- Shale prepares the text first (DOCX-003): the front matter is gone and every embed names its file by its
-- vault path. What the docx writer cannot say itself is left as marks that Shale finishes after pandoc
-- (DOCX-011): a zero-length bookmark `_shale_jc_<align>` in an aligned paragraph, and character styles named
-- `Shale colour RRGGBB`, `Shale colour RRGGBB on RRGGBB` and `Shale on RRGGBB` for text and highlight colours.

-- the width of the editor's text column an embed's size is a share of (PDF-005, embed_size::DEFAULT_COLUMN)
local COLUMN = 720
-- the width of the Word page's text in inches, which Shale gives with `-M shale-text-width=<inches>`: pandoc's docx
-- writer takes no share of the page, so a size is a width in inches (without it, a share of the page)
local text_width
local PICTURES = {png = true, jpg = true, jpeg = true, gif = true, bmp = true, tif = true, tiff = true, svg = true, emf = true, wmf = true}

local function lower(s) return s:lower() end

-- an embed `![[path|size]]`: pandoc reads `!` and a wikilink. A picture becomes an image as wide as the
-- editor shows it; anything else (a note, a PDF, a sound) its name
local function embed(link)
  local target = link.target
  local path = target:gsub('#.*$', '')
  local ext = lower(path:match('%.([%w]+)$') or '')
  local label = pandoc.utils.stringify(link.content)
  if not PICTURES[ext] then
    local name = path:match('([^/]+)$') or path
    return pandoc.Emph({pandoc.Str(name:gsub('%.md$', ''))})
  end
  local attr = pandoc.Attr()
  local width = label:match('^(%d+)$') or label:match('^(%d+)x%d+$')
  local alt = {}
  if width then
    local share = math.min(1, tonumber(width) / COLUMN)
    if text_width then
      attr.attributes.width = string.format('%.4fin', share * text_width)
    else
      attr.attributes.width = string.format('%d%%', math.floor(share * 100 + 0.5))
    end
  elseif label ~= target then
    alt = {pandoc.Str(label)}
  end
  return pandoc.Image(alt, path, '', attr)
end

-- a link `[[note|alias]]`: its words (a note of the vault is no place in the Word file)
local function wikilink(link)
  local words = pandoc.utils.stringify(link.content)
  if words == link.target then words = words:gsub('%.md$', ''):gsub('#', ' > ') end
  return pandoc.Str(words)
end

local function is_wikilink(i) return i and i.t == 'Link' and i.classes:includes('wikilink') end

-- the sixteen highlight colours of Word that pandoc writes itself: only yellow (a `.mark` span)
local YELLOW = 'FFFF00'

local function hex(s)
  local h = s:match('#(%x%x%x%x%x%x)%f[^%x]') or s:match('#(%x%x%x)%f[^%x]')
  if not h then return nil end
  if #h == 3 then h = h:sub(1, 1):rep(2) .. h:sub(2, 2):rep(2) .. h:sub(3, 3):rep(2) end
  return h:upper()
end

-- the character style of a colour and a background (one style holds both: a run has one style)
local function colour_style(fg, bg)
  if fg and bg then return 'Shale colour ' .. fg .. ' on ' .. bg end
  if fg then return 'Shale colour ' .. fg end
  return 'Shale on ' .. bg
end

-- the HTML tags of a note's text that are Word formatting, and what they become
local function tag_of(raw)
  if raw.t ~= 'RawInline' or raw.format ~= 'html' then return nil end
  local s = raw.text
  local close = s:match('^</%s*(%a+)%s*>$')
  if close then return {name = lower(close), close = true} end
  local name, rest = s:match('^<%s*(%a+)(.-)/?>$')
  if not name then return nil end
  name = lower(name)
  local t = {name = name}
  local style = rest:match('style%s*=%s*"([^"]*)"') or rest:match("style%s*=%s*'([^']*)'") or ''
  if name == 'span' then t.fg = hex(style:match('[^-]color%s*:%s*([^;]+)') or style:match('^color%s*:%s*([^;]+)') or '') end
  if name == 'mark' then t.bg = hex(style:match('background[%w-]*%s*:%s*([^;]+)') or '') or YELLOW end
  return t
end

-- `==text==` in one list of inlines becomes a highlight (pandoc's commonmark has no `mark` extension)
local function marks(list)
  local out = pandoc.Inlines({})
  local i = 1
  while i <= #list do
    local el = list[i]
    local opened = el.t == 'Str' and el.text:match('^==(.+)$')
    local stop
    if opened then
      for j = i, #list do
        local s = list[j]
        if s.t == 'Str' and s.text:match('.==$') and (j > i or #s.text > 4) then stop = j; break end
      end
    end
    if stop then
      local inner = pandoc.Inlines({})
      for j = i, stop do inner:insert(list[j]:clone()) end
      inner[1].text = inner[1].text:sub(3)
      inner[#inner].text = inner[#inner].text:sub(1, -3)
      out:insert(pandoc.Span(inner, {class = 'mark'}))
      i = stop + 1
    else
      out:insert(el); i = i + 1
    end
  end
  return out
end

-- pair the tags of one list and wrap what lies between; `fg` and `bg` are the colours around it
local function tags(list, fg, bg)
  local out = pandoc.Inlines({})
  local i = 1
  while i <= #list do
    local el = list[i]
    local t = tag_of(el)
    if t and t.name == 'br' and not t.close then
      out:insert(pandoc.LineBreak()); i = i + 1
    elseif t and not t.close then
      local depth, stop = 0, nil
      for j = i + 1, #list do
        local u = tag_of(list[j])
        if u and u.name == t.name then
          if not u.close then depth = depth + 1
          elseif depth == 0 then stop = j; break
          else depth = depth - 1 end
        end
      end
      if stop then
        local middle = pandoc.Inlines({})
        for j = i + 1, stop - 1 do middle:insert(list[j]) end
        local nfg, nbg = fg, bg
        if t.name == 'span' and t.fg then nfg = t.fg end
        if t.name == 'mark' then nbg = t.bg end
        middle = tags(middle, nfg, nbg)
        if t.name == 'u' or t.name == 'ins' then out:insert(pandoc.Underline(middle))
        elseif t.name == 'sub' then out:insert(pandoc.Subscript(middle))
        elseif t.name == 'sup' then out:insert(pandoc.Superscript(middle))
        elseif t.name == 's' or t.name == 'del' or t.name == 'strike' then out:insert(pandoc.Strikeout(middle))
        elseif t.name == 'b' or t.name == 'strong' then out:insert(pandoc.Strong(middle))
        elseif t.name == 'i' or t.name == 'em' then out:insert(pandoc.Emph(middle))
        elseif nfg ~= fg or (nbg ~= bg and nbg ~= YELLOW) then
          -- a colour of its own: the style of both colours (a yellow background stays Word's highlight)
          local sbg = nbg ~= YELLOW and nbg or nil
          local span = pandoc.Span(middle, {['custom-style'] = colour_style(nfg, sbg)})
          if nbg == YELLOW and bg ~= YELLOW then span = pandoc.Span({span}, {class = 'mark'}) end
          out:insert(span)
        elseif nbg == YELLOW and bg ~= YELLOW then
          out:insert(pandoc.Span(middle, {class = 'mark'}))
        else
          out:extend(middle)
        end
        i = stop + 1
      else
        i = i + 1 -- a tag without its end is left out, like any HTML Word cannot show
      end
    elseif t then
      i = i + 1 -- a stray closing tag
    else
      out:insert(el); i = i + 1
    end
  end
  return out
end

local function inlines(list)
  local out = pandoc.Inlines({})
  local i = 1
  while i <= #list do
    local el = list[i]
    if el.t == 'Str' and el.text:sub(-1) == '!' and is_wikilink(list[i + 1]) then
      if #el.text > 1 then out:insert(pandoc.Str(el.text:sub(1, -2))) end
      out:insert(embed(list[i + 1])); i = i + 2
    elseif is_wikilink(el) then
      out:insert(wikilink(el)); i = i + 1
    elseif el.t == 'SoftBreak' then
      -- a new line inside a paragraph is a line of its own in Word too (IMP-037)
      out:insert(pandoc.LineBreak()); i = i + 1
    else
      out:insert(el); i = i + 1
    end
  end
  return tags(marks(out))
end

-- the wrapper lines of an alignment (PDF-028) and a page-break marker (PDF-020)
local function align_open(b)
  if b.t ~= 'RawBlock' or b.format ~= 'html' then return nil end
  local a = b.text:match('^%s*<[dD][iI][vV]%s+[aA][lL][iI][gG][nN]%s*=%s*["\'](%a+)["\']%s*>%s*$')
  a = a and lower(a)
  if a == 'center' or a == 'right' or a == 'justify' or a == 'left' then return a end
  return nil
end
local function align_close(b)
  return b.t == 'RawBlock' and b.format == 'html' and b.text:match('^%s*</%s*[dD][iI][vV]%s*>%s*$') ~= nil
end
local function page_break(b)
  if b.t ~= 'RawBlock' or b.format ~= 'html' then return false end
  local style = b.text:match('^%s*<[dD][iI][vV]%s+[sS][tT][yY][lL][eE]%s*=%s*["\']([^"\']*)["\']%s*>%s*</[dD][iI][vV]>%s*$')
  if not style then return false end
  style = lower(style):gsub('%s', '')
  for _, p in ipairs({'page-break-after:always', 'page-break-before:always', 'break-after:page', 'break-before:page'}) do
    if style:find(p, 1, true) then return true end
  end
  return false
end
local BREAK = '<w:p><w:r><w:br w:type="page"/></w:r></w:p>'
-- Word's `w:jc` for an alignment of the wrapper
local JC = {center = 'center', right = 'right', justify = 'both', left = 'left'}
local function mark_align(b, a)
  if b.t == 'Para' or b.t == 'Plain' or b.t == 'Header' then
    b.content:insert(1, pandoc.RawInline('openxml', '<w:bookmarkStart w:id="0" w:name="_shale_jc_' .. JC[a] .. '"/><w:bookmarkEnd w:id="0"/>'))
  end
  return b
end

-- a picture alone in its paragraph
local function lone_image(b)
  return b and (b.t == 'Para' or b.t == 'Plain') and #b.content == 1 and b.content[1].t == 'Image'
end
-- an italic paragraph: the caption the import writes below a picture
local function caption(b)
  if not b or b.t ~= 'Para' then return nil end
  local c = pandoc.Inlines({})
  for _, i in ipairs(b.content) do
    if i.t == 'Emph' then c:extend(i.content)
    elseif i.t == 'Space' and #c > 0 then c:insert(i)
    else return nil end
  end
  if #c == 0 then return nil end
  return c
end

local function blocks(list)
  local out = pandoc.Blocks({})
  local align
  local i = 1
  while i <= #list do
    local b = list[i]
    local a = align_open(b)
    if a then
      -- an opening line counts when a closing line follows before the next opening one (PDF-028)
      local closed = false
      for j = i + 1, #list do
        if align_open(list[j]) then break end
        if align_close(list[j]) then closed = true; break end
      end
      if closed then align = a end
      i = i + 1
    elseif align and align_close(b) then
      align = nil; i = i + 1
    elseif page_break(b) then
      out:insert(pandoc.RawBlock('openxml', BREAK)); i = i + 1
    elseif lone_image(b) and caption(list[i + 1]) then
      local img = b.content[1]
      local words = caption(list[i + 1])
      img.caption = words -- the caption is the picture's alternative text too
      out:insert(pandoc.Figure({pandoc.Plain({img})}, {long = {pandoc.Plain(words)}}))
      i = i + 2
    else
      if align then b = mark_align(b, align) end
      out:insert(b); i = i + 1
    end
  end
  return out
end

-- the title block the import writes (IMP-042, IMP-046): a centred `#` heading at the very top, with an
-- italic line below it, becomes Word's Title and Subtitle
local function title(doc)
  local w = doc.meta['shale-text-width']
  text_width = w and tonumber(pandoc.utils.stringify(w)) or nil
  local b = doc.blocks
  if not (b[1] and align_open(b[1]) == 'center' and b[2] and b[2].t == 'Header' and b[2].level == 1) then return nil end
  local sub = caption(b[3])
  local stop = sub and 4 or 3
  if not (b[stop] and align_close(b[stop])) then return nil end
  doc.meta.title = b[2].content
  if sub then doc.meta.subtitle = sub end
  local rest = pandoc.Blocks({})
  for i = stop + 1, #b do rest:insert(b[i]) end
  doc.blocks = rest
  return doc
end

return {
  {Pandoc = title},
  {Inlines = inlines},
  {Blocks = blocks},
}
