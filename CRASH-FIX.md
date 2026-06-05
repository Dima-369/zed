# CRASH-FIX.md

## Crash Summary

- **Panic message:** `cannot seek backward`
- **Location:** `crates/multi_buffer/src/multi_buffer.rs:5051`
- **Session ID:** `38da10dc-f825-4b6e-9458-d835e7ec50a5`
- **Zed version:** 1.6.0 dev (commit `962d36207694c2d038aa734912c2eaab3fd82dae`)
- **Release channel:** dev
- **Minidump:** `~/Library/Logs/Zed/38da10dc-f825-4b6e-9458-d835e7ec50a5.dmp` (131KB)
- **Crash JSON:** `~/Library/Logs/Zed/38da10dc-f825-4b6e-9458-d835e7ec50a5.json`

## Root Cause

The crash occurs in `summaries_for_anchors_cb` in `crates/multi_buffer/src/multi_buffer.rs`. This function iterates over a list of `Anchor` values and resolves each one to a position in the multi-buffer by seeking a `sum_tree::Cursor` over the excerpts tree.

The function calls `cursor.seek_forward(&target, Bias::Left)` on each iteration, reusing the same cursor. `seek_forward` has an internal assertion that the target position must be >= the cursor's current position — it **panics** with `"cannot seek backward"` if asked to seek to an earlier position.

The problem: **callers do not guarantee that anchors are passed in sorted order.** The primary caller is `resolve_selections_point` in `crates/editor/src/selections_collection.rs:1109`:

```rust
let mut summaries = map
    .buffer_snapshot()
    .summaries_for_anchors::<Point, _>(to_summarize.flat_map(|s| [&s.start, &s.end]))
    .into_iter();
```

Selections can be in any order (e.g., multiple cursors created at different positions), so the flattened `[start, end]` pairs are not necessarily sorted by their position in the multi-buffer. When a later anchor happens to be positioned before where the cursor currently is, `seek_forward` panics.

Other callers with the same issue:
- `crates/vim/src/state.rs:462` — passes anchors from vim state
- `crates/editor/src/display_map/custom_highlights.rs:105,113,169,177` — passes anchor ranges for custom highlights

## Investigation Steps

1. Read the crash JSON at `~/Library/Logs/Zed/38da10dc-f825-4b6e-9458-d835e7ec50a5.json` to identify the panic message and location.
2. Read `crates/multi_buffer/src/multi_buffer.rs` around line 5051 to find the `seek_forward` call.
3. Traced the panic to `crates/sum_tree/src/cursor.rs:473` where the assertion lives:
   ```rust
   assert!(
       target.cmp(&self.position, self.cx).is_ge(),
       "cannot seek backward",
   );
   ```
4. Identified all callers of `summaries_for_anchors` / `summaries_for_anchors_cb` via grep.
5. Confirmed that `resolve_selections_point` passes unsorted anchors (selections are not ordered by position).

## Fix

Modified `summaries_for_anchors_cb` in `crates/multi_buffer/src/multi_buffer.rs`:

1. **Collect anchors with original indices** — `anchors.into_iter().enumerate().collect()` preserves the input order for the callback.
2. **Process `Min`/`Max` anchors immediately** — these don't use the cursor and can be resolved without seeking.
3. **Schwartzian transform for sorting** — seek a temporary cursor once per anchor (O(N)) to compute an `ExcerptSummary` sort key, then sort by comparing those keys (O(N log N) cheap comparisons instead of O(N log N) expensive cursor seeks).
4. **Three-level sort** — `(path_key, text.len, ExcerptAnchor::cmp)`. The first two levels use the cursor summary; the third level uses `ExcerptAnchor::cmp` which compares `text_anchor` values within the same buffer using the buffer snapshot. This guarantees both the main cursor and `diff_transforms_cursor` only move forward, even for anchors within the same excerpt.
5. **Process sorted anchors** — the existing batch optimization (grouping consecutive anchors from the same excerpt) still works correctly.
6. **Call the callback in original order** — iterate through the results vector and call `cb` for each anchor in the order they were passed in.

Also changed `diff_transforms_cursor.seek_forward()` to `diff_transforms_cursor.seek()` in the two places where the cursor might need to move backward (when the anchor is not in the current excerpt, or when the cursor item is `None`). The `seek` method resets the cursor first, avoiding the same class of panic.

## Attempted Approaches

1. **Sort by `AnchorSeekTarget`** — abandoned because `AnchorSeekTarget` doesn't implement `Ord`.
2. **Sort by `(PathKeyIndex, text_anchor)`** — abandoned because `text::Anchor` doesn't implement `Ord` (requires a `BufferSnapshot` for comparison) and getting the snapshot from a `PathKeyIndex` is non-trivial.
3. **Use `cursor.seek()` instead of `cursor.seek_forward()` everywhere** — considered but rejected because it would lose the batch optimization (grouping consecutive anchors from the same excerpt) and require resetting `diff_transforms_cursor` on every iteration.
4. **Sort using a temporary cursor inside `sort_by`** (first attempt) — worked but had two problems:
   - **Incomplete sort**: only compared `(path_key, text.len)`, which orders excerpts but not anchors within the same excerpt. This would still cause `diff_transforms_cursor.seek_forward` to panic for out-of-order intra-excerpt anchors.
   - **O(N log N log M) performance**: calling `sort_cursor.seek()` inside the `sort_by` closure means O(N log N) comparisons each doing an O(log M) tree seek.
5. **Schwartzian transform + `ExcerptAnchor::cmp`** (chosen) — seek the cursor once per anchor to compute sort keys (O(N log M)), then sort by comparing keys (O(N log N) cheap comparisons). Use `ExcerptAnchor::cmp` as the final tiebreaker to correctly order anchors within the same excerpt. This is both correct and performant.

## Files Changed

- `crates/multi_buffer/src/multi_buffer.rs` — rewrote `summaries_for_anchors_cb`

## Tests

- All 58 `multi_buffer` tests pass, including `test_summaries_for_anchors`.
- Pre-existing failures in `editor` and `vim` crates were confirmed unrelated (same failures on unmodified code).
