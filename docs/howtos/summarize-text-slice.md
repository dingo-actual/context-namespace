# How to Summarize a Text Slice Within One Record

When a single record's render field is very long, you can summarize a
character range within it rather than replacing entire records.

## Prerequisites

- A `ContextNamespace` with at least one record
- A fresh `RenderId` from `ns.render()`

## Steps

1. Render to obtain a fresh `RenderId`:

   ```rust
   let view = ns.render().expect("render");
   ```

2. Prepare a text-slice summary with a `TextSelection`:

   ```rust
   use context_namespace::{TextSelection, RecordLocator, NodeLabel};

   let label = NodeLabel::new("intro-summary", &ns.config().limits).unwrap();
   let prepared = ns.prepare_text_summary(
       TextSelection {
           record: RecordLocator::Visible {
               render_id: view.render_id,
               index: 0,
           },
           start_char: 0,
           end_char: 500,
       },
       label,
   ).expect("prepare");
   ```

   Character offsets are Unicode scalar-value offsets. `Ref` segments
   occupy zero character width, so a range cannot split one; if the
   range covers a `Ref` position, the referenced node becomes a child
   of the new summary.

3. Commit the summary:

   ```rust
   use context_namespace::SummaryResult;

   ns.commit_summary(prepared, SummaryResult {
       summary: "Introduction summary".to_string(),
       gloss: "Intro".to_string(),
   }).expect("commit");
   ```

   The character range is replaced by a `Segment::Ref` pointing to the
   new node. The record remains in place with its prefix and suffix text
   intact.

## Locating a record by ID instead of visible index

If you have the `RecordId` from an `AppendReceipt`, use
`RecordLocator::Id`:

```rust
let prepared = ns.prepare_text_summary(
    TextSelection {
        record: RecordLocator::Id(record_id),
        start_char: 0,
        end_char: 500,
    },
    label,
).expect("prepare");
```

This does not require a `RenderId` and is not subject to stale-render
rejection.

## Verification

After commit, `ns.stats().nodes` increments by one. The new node
appears in `ns.list_nodes()` at path `/intro-summary`.

## Common errors

- **`StaleRender`**: You used a `RenderId` from before a canonical
  mutation (append or commit). Re-render and use the new `RenderId`.
- **`InvalidSelection`**: `start_char >= end_char`, or `end_char`
  exceeds the render field length.
