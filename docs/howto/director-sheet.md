# A sheet for the director (Regie)

A plan with only the dimensions the director and stage management need, without
fixtures, labels or notes. It works with any document.

1. **Draw the dimensions.** Dimension tool (or ⌘K "Dimension"): pick two points.
   Give each a name in the Inspector ("wing 1", "rear wall"); with "Dimension names" on,
   the sheet prints "wing 1 1857".
2. **Tag them.** Click a dimension; Shift-click more. In the Inspector's Tags field type a
   tag, e.g. `regie`, and press Enter, or pick an existing tag from the suggestions while
   typing. The × on a tag removes it from the selected dimensions.
3. **Make the sheet.** The + in the status bar adds a sheet; add a plan viewport
   (⌘K "Add Viewport").
4. **Choose what it shows.** Select the viewport. In the Inspector: untick the tags and
   layers to leave out (e.g. `light`, `rigging`, `notes`; layers with the seating), tick
   `regie` under "Only dimensions tagged" and switch on "Dimension names".
5. **Keep them off the other sheets.** On the lighting plan's viewport untick `regie`
   under the shown tags: tagged dimensions are hidden there.
6. **Scale and crop**, then File > Export PDF (or `sheet.export_pdf {sheets: ["LB-R"]}`)
   for this sheet alone.

A second viewport on the same sheet can show another part (the hall at 1:125) with its
own dimension tag (`regie-hall`).

Over MCP/CLI the same: `dimension.set_tags {ids, add: ["regie"]}`, `viewport.update
{dimension_tags: ["regie"], dimension_names: true, hidden_tags: [...]}`.
