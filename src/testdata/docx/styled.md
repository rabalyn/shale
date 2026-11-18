---
title: Styled report
subtitle: A look to keep
---

# Heading one

Plain text with `<w:r><w:rPr><w:color w:val="C00000"/></w:rPr><w:t>red text</w:t></w:r>`{=openxml}, a `<w:r><w:rPr><w:highlight w:val="green"/></w:rPr><w:t>green highlight</w:t></w:r>`{=openxml} and a `<w:r><w:rPr><w:highlight w:val="yellow"/></w:rPr><w:t>yellow highlight</w:t></w:r>`{=openxml}.

Between spaces: `<w:r><w:rPr><w:b/><w:color w:val="0000FF"/></w:rPr><w:t xml:space="preserve"> bold blue </w:t></w:r>`{=openxml}and on.

A `<w:r><w:rPr><w:color w:val="00B050"/><w:shd w:val="clear" w:color="auto" w:fill="FFC000"/></w:rPr><w:t>green on orange</w:t></w:r>`{=openxml} run.

Before a manual break `<w:r><w:br w:type="page"/></w:r>`{=openxml}after it.

```{=openxml}
<w:p><w:pPr><w:pageBreakBefore/></w:pPr><w:r><w:t>Starts a page by itself.</w:t></w:r></w:p>
<w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/></w:sectPr></w:pPr><w:r><w:t>Ends a section.</w:t></w:r></w:p>
<w:p><w:pPr><w:sectPr><w:type w:val="continuous"/></w:sectPr></w:pPr><w:r><w:t>Ends a continuous section.</w:t></w:r></w:p>
```

The last paragraph.
