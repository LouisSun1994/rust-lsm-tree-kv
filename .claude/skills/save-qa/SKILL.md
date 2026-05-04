---
name: save-qa
description: Save a meaningful Q&A from the current conversation to this project's qa/ folder for the user's long-term learning review. Use proactively (without waiting for the user to ask) when the user asks a conceptual question and the answer is one they would benefit from re-reading later — i.e. it taught them a transferable mental model, exposed a non-obvious tradeoff, or led them to derive something themselves. Skip routine operational requests, simple how-to questions, and questions whose answers are already captured in this project's docs/ or code comments.
---

# save-qa

Persist a Q&A pair from the current conversation as an individual markdown file under `qa/`, so the user can review and revisit their learning over time.

## Project context

This skill lives in this LSM-Tree learning project. The user is at the **enlightenment phase** of learning Rust + systems programming. Each Q&A entry is a learning checkpoint they will re-read weeks later to test whether the concept has internalized.

The `qa/` folder already exists with entries 01–04. **Read at least one existing entry before writing a new one** to match the established format and tone.

## When to use this skill

**Trigger proactively** (without the user asking) right after responding to a question if all three are true:

1. **Conceptual or design-level question** — not a "please run X" or "what does this command do" request
2. **Answer contains transferable knowledge** — a mental model, a tradeoff explanation, a derivation the user did themselves, or a non-obvious detail they're likely to forget
3. **The user is in a learning posture** — they're trying to build understanding, not just complete a task

**Do NOT trigger** for:
- Operational requests ("commit this", "run tests", "rename this file")
- Trivial syntax lookups ("what's the syntax for X")
- Questions whose full answer is already in the project's `docs/` or source comments
- Questions that were answered with "I don't know" or required a follow-up correction
- Off-topic chat

When in doubt, **ask the user once**: "This feels worth saving to qa/ — agree?" Only proceed on yes. Don't ask every time; only when genuinely uncertain.

## How to save

### 1. Pick the next question number

- List existing `qa/NN-*.md` files
- Use the next zero-padded number (e.g. existing 01–04 → next is 05)

### 2. Pick a slug

- Short, kebab-case, English, captures the question's core
- Examples (existing): `lsm-essence`, `memtable-sorted-not-hashmap`, `wal-fsync-tradeoff`, `no-wal-small-memtable`
- Filename: `qa/NN-<slug>.md`

### 3. Write the Q&A file

The user prefers traditional Chinese for prose, English for technical terms / code. Match the existing entries' format:

```markdown
# QNN：<question title in user's words, condensed>

## 我的提問

> <quote the user's question as faithfully as possible — preserve their original phrasing, including incomplete or imprecise framing, because that's what they'll recognize on review>

## 結論：<one-line takeaway>

<substantive answer — focus on what's worth re-reading. Keep:
- mental models / frameworks
- concrete examples / numbers / benchmarks from the actual project
- tradeoffs and why they exist
- references to specific code paths (use clickable relative links like ../src/wal.rs:80)>

## 我學到了什麼

<one paragraph — the durable lesson, phrased as something carryable to other projects, not just facts about this codebase>
```

**Style rules**:
- Read at least one existing `qa/NN-*.md` first to match tone
- Keep each file focused on ONE question — never bundle
- If the user did most of the reasoning themselves, lead with "你完全抓對了..." or similar — make their authorship visible to future-them
- Code references must be clickable relative paths (e.g. `../src/wal.rs:80`)

### 4. Update qa/README.md index

Add one row to the table:
```markdown
| [NN](NN-<slug>.md) | <short topic> | <key concept tags> |
```

Keep `qa/README.md` to ≤30 lines total. It's an index. Never put answer content in the README.

### 5. Tell the user what you saved

After writing, one line: "Saved as `qa/NN-<slug>.md`." Don't recap content — they just read it.

## Edge cases

- **Question spans multiple turns**: capture the user's clearest framing (often the last one), not every turn.
- **User explicitly says "save this"**: still apply the quality filter. If it's operational disguised as a question, ask whether they really want it saved.
- **Multiple distinct questions in one user turn**: split into separate files only if each meets the trigger criteria independently. Otherwise pick the most substantive one.
- **User corrected your earlier answer in a later turn**: save the corrected version. Optionally add a "踩過的坑" subsection documenting the correction so future-them sees the path, not just the destination.
- **Question is closely related to an existing entry**: link to the existing one in a "相關" section at the bottom rather than duplicating.

## Why this skill exists

The user is in a learning-by-doing phase. Their growth depends on revisiting earlier reasoning — especially the moments where they derived a non-obvious conclusion themselves. Each saved Q&A is a future checkpoint: "Two weeks ago I struggled with this; can I now answer it cold?" Capture them at the moment they happen, in their own words, with minimal friction.
