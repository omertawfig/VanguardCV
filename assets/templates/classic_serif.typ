// VanguardCV — Classic Serif resume template.
//
// This file is prepended to a generated `#cv((...))` call by
// `src/pdf/typst_compiler.rs`. All user text arrives as escaped Typst strings
// inside the data dictionary, so the layout below is never parsed as user
// markup.
//
// Typography follows the master specification §7: A4, 0.6in top/bottom,
// 0.65in left/right, centered header, 11pt bold uppercase section titles with
// a full-width 0.6pt rule, and two-column experience rows.

#let cv(data) = {
  set page(
    paper: "a4",
    margin: (top: 0.6in, bottom: 0.6in, left: 0.65in, right: 0.65in),
  )
  set text(font: "Libertinus Serif", size: 10pt, fill: black)
  set par(leading: 0.55em, spacing: 0.6em, justify: false)

  // -------------------------------------------------------------------------
  // Centered contact header
  // -------------------------------------------------------------------------
  align(center, block(width: 100%)[
    #text(size: 18pt, weight: "bold")[#data.name]
    #linebreak()
    #if data.headline != none [
      #text(size: 11.5pt, weight: "bold")[#data.headline]
      #linebreak()
    ]
    #if data.contacts.len() > 0 [
      #text(size: 9.5pt)[#data.contacts.join(" | ")]
    ]
    #if data.links.len() > 0 [
      #if data.contacts.len() > 0 [ #linebreak() ]
      #text(size: 9.5pt)[
        #for (i, item) in data.links.enumerate() {
          if i > 0 [ | ]
          if item.url != none { link(item.url)[#item.text] } else [ #item.text ]
        }
      ]
    ]
  ])

  // -------------------------------------------------------------------------
  // Sections
  // -------------------------------------------------------------------------
  for section in data.sections {
    block(breakable: false, sticky: true, above: 9pt, below: 3pt)[
      #text(size: 11pt, weight: "bold", upper(section.title))
      #v(-4pt)
      #line(length: 100%, stroke: 0.6pt)
    ]

    if section.kind == "summary" {
      text(section.text)
    } else if section.kind == "timeline" {
      for entry in section.entries {
        block(breakable: false, sticky: true)[
          #grid(
            columns: (1fr, auto),
            column-gutter: 12pt,
            align: (left, right),
            text(weight: "bold", entry.heading),
            text(weight: "bold", entry.dates),
          )
          #if entry.secondary != none [
            #text(weight: "bold", entry.secondary)
          ]
        ]
        for bullet in entry.bullets {
          grid(
            columns: (12pt, 1fr),
            text("•"),
            text(bullet),
          )
        }
        v(4pt)
      }
    } else if section.kind == "inline" {
      for cat in section.categories {
        par[
          • #text(weight: "bold")[#cat.label:]
          #if cat.items.len() > 0 [ #cat.items.join(cat.separator)]
        ]
      }
    } else if section.kind == "bullets" {
      for item in section.items {
        grid(
          columns: (12pt, 1fr),
          text("•"),
          text(item),
        )
      }
    }
  }
}
