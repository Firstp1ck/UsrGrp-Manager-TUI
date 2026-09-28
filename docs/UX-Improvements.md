# UX improvement findings

Date: 2026-09-28
Scope: the current TUI as of commit `c78176d` (main). Findings come from reading `src/ui`, `src/app`, the config loaders, the wiki, and the v0.3.0 screenshots. The app and its tests were not run, so nothing here was measured with real users. A second reviewer (gpt-6-astra) checked every claim against the source and added the items marked "found in review".

The rewrite in `e1b0ae4` made the core much safer (typed operation bridge, redacted previews, atomic config). It also dropped a lot of the v0.3.0 front end. Several findings below are regressions rather than new ideas, and the old code in `git show d1cb11a:src/ui/` is a usable starting point for restoring them.

Severity scale: **P1** loses work or produces a wrong result, **P2** confuses or slows the user, **P3** polish.

## Summary

| # | Finding | Severity |
|---|---------|----------|
| 1 | A typo in the sudo password discards the whole prepared operation | P1 |
| 2 | Validation errors close the form and throw away everything typed | P1 |
| 3 | "Add members" checkboxes can submit a different user than the one ticked | P1 |
| 4 | The search query is never shown, and Enter leaves a hidden filter active | P1 |
| 5 | The first delete dialog names no user, and deleting yourself is not flagged | P1 |
| 6 | "Add to sudo group" and adding anyone to wheel/sudo always fail under the shipped policy | P1 |
| 7 | Config errors are flagged but the messages are never displayed | P2 |
| 8 | No refresh key | P2 |
| 9 | Filter menu shows no on/off state, and shadow filters silently no-op | P2 |
| 10 | Five theme colors are parsed and written but ignored by the renderer | P2 |
| 11 | Details pane lost most of the v0.3.0 fields | P2 |
| 12 | Keybindings panel prints raw enum names in alphabetical order | P2 |
| 13 | Help modal is one line, and its scroll state is dead | P2 |
| 14 | Operation results and confirmations use developer wording | P2 |
| 15 | Mouse capture is on, so terminal text selection is broken, and nothing uses the mouse | P2 |
| 16 | Escape closes the whole modal stack; Backspace goes back in only two menus | P2 |
| 17 | Yes/No dialogs and menus disagree on which keys move the cursor | P2 |
| 18 | Text inputs start empty, show no cursor, and hit silent length caps | P2 |
| 19 | Candidate lists have no search, no page indicator, no selected count | P2 |
| 20 | No interrupt binding; Ctrl+C inserts a character in text fields | P2 |
| 21 | Enter on "No" does nothing in delete dialogs | P2 |
| 22 | Moving the parent selection leaves the child pane with a stale cursor | P2 |
| 23 | The Groups-tab filter hides memberships in the Users tab | P2 |
| 24 | Bulk confirmations show only the first 12 commands with no way to see the rest | P2 |
| 25 | Page keys move by a constant 10, not by the visible page | P3 |
| 26 | Header and status bar repeat each other, and the stale reason is clipped off | P3 |
| 27 | Empty lists render as blank space | P3 |
| 28 | No `--help`, `--version`, or `--dry-run` | P3 |
| 29 | Wiki documents keys and behavior that no longer exist | P3 |
| 30 | Missing actions people expect: lock/unlock, primary group, expiry, sort | P3 |
| 31 | Smaller copy and layout issues | P3 |

## P1: loses work or produces a wrong result

### 1. Sudo typo discards the prepared operation

`src/app/update.rs:1194` treats every error other than `AuthenticationRequired` the same way, including `AuthenticationDenied`. It calls `clear_pending_operation` and shows an Info modal. The user then has to navigate back through Actions, Modify, pick the group again, confirm the preview again, and retype the password.

Fix: on `AuthenticationDenied`, keep `pending_operation` and reopen `SudoPrompt` with `error: Some("Password rejected, try again")`. Cap retries at three, then clear. The `SudoPrompt` struct already has an `error` field for exactly this, and it is only used for the empty-password case (`src/app/update.rs:1106`).

### 2. Validation errors close the form

`handle_password_input` and `handle_user_add` call `open_modal(Info { ... })` on mismatch (`src/app/update.rs:667` and `:1039`). `open_modal` replaces `app.modal`, so the `ChangePassword` or `UserAddInput` state is dropped along with the typed username and both password fields. The same happens when `PasswordRecord::new` rejects the input.

Fix: give the form modals an `error: Option<String>` field like `SudoPrompt` has, render it inline under the fields, and leave the form open. Validate the username against `UserName::new` on every keystroke so the "must start with an ASCII letter" style messages appear before Submit.

### 3. "Add members" ticks the wrong user (found in review)

On the Groups tab, the "Add members" modal renders every user (`src/ui/groups.rs:184`, `app.users_all` unfiltered), but the key handler builds its candidate list from users who are *not already members* (`src/app/update.rs:951`). The checkbox indices refer to the rendered list, the submitted names come from the filtered list. Every existing member that sorts before the user you tick shifts the target one user further down, and an index past the end of the filtered list submits nothing. The confirmation preview does show the real target, so a careful user can catch it, but the dialog itself lies.

Fix: build one candidate list and pass it to both the renderer and the handler. The Users-tab "Add groups" modal avoids the bug today only because its renderer and handler duplicate the same filter expression (`src/ui/users.rs:282` and `src/app/update.rs:560`); it should share the list too.

### 4. Search is invisible and sticky

`grep search_query src/ui` returns nothing. While typing in search mode the list shrinks but the query itself is drawn nowhere, and the status bar does not say the app is in search mode (v0.3.0 showed `SEARCH(users)`).

Enter switches back to `Normal` without re-running the filter (`src/app/update.rs:122`), so the list stays filtered by a query the user can no longer see. The next `sort_and_filter` call, which happens on any filter toggle or after any operation that returns a report, silently drops the query because `apply_filters_and_search` only applies it when `input_mode` is a search mode. So the list changes under the user for no visible reason. There is no binding that clears the search directly; Esc in normal mode is bound to `Ignore`.

Fix: draw the query in the status bar or as the table title (`Users  /alice  12 of 45`), keep the query as persistent state independent of input mode, and bind Esc in normal mode to clear it.

### 5. The first delete dialog is anonymous and unguarded

`render_user_modal` prints "Delete selected user?" with no username (`src/ui/users.rs:225`). The command preview that follows does show `userdel -r alice`, so the name appears before anything runs, but the dialog where the user picks "also delete home" should carry the name, UID, and home path itself.

`is_default_protected_user` blocks UID 0, the name `root`, and UIDs below 1000. Deleting `app.current_username` or any user with running processes goes straight through to the preview. v0.3.0 showed a process count. Add a line "This is your own account" or "N running processes" and keep the cursor default on No, which it already is.

Also, the checkbox in that dialog has marker index 2 but Left/Right only toggle 0 and 1 (`src/app/update.rs:711`), so the cursor arrow can never land on the checkbox, and Space toggles it from anywhere. The layout implies a focusable row that does not exist.

### 6. Sudo-group membership can never succeed (found in review)

The create-user form offers "Add to sudo group" (`src/ui/users.rs:261`), and the "Add groups" modal lists `wheel` and `sudo` like any other group. Production always uses `ProtectedIdentityPolicy::fail_closed()` (`src/sys/mod.rs:186`), whose allowlist for elevation groups is empty, and nothing in `main.rs` or the config loaders can add to it. With the default `UGM_SUDO_GROUP` (wheel) or with `sudo`, the request fails at prepare time after the user has filled in the whole form. Because both groups normally have a GID below 1000, the message is usually `service group is protected by policy` rather than the elevation-specific one. The checkbox only works if `UGM_SUDO_GROUP` names some other group with a GID of 1000 or more.

README says the policy can be overridden by "a reviewed runtime policy", but no runtime policy file or flag exists. Either add one, or remove the checkbox and grey out `sudo`/`wheel` in the candidate list with the reason shown.

## P2: confusing or slow

### 7. Config errors are hidden

`render_status_bar` appends ` config-error` when `diagnostics.config_messages` is non-empty (`src/ui/components.rs:21`), but no renderer prints the messages. The loaders produce useful text like `configuration line 7: invalid theme color`, and the user never sees it. Show them in the Info modal on startup, or make the status bar segment expandable with a key.

### 8. No refresh

`KeyAction` has no refresh variant. `refresh_accounts` only runs on startup and after an operation returns a report, whether or not that report contains a failed step. An admin who runs `useradd` in another terminal has to quit and restart. The wiki FAQ says "use the refresh action", which does not exist. Add `Refresh = r` and show the last refresh time in the status bar next to the stale marker.

### 9. Filter menu has no state, and some filters do nothing

The `FilterMenu` render shows a cursor arrow and a label (`src/ui/components.rs:102`). It does not show which chips are on. A user who toggled "Locked" last week and now sees a short list has no indication why, because active filters are also absent from the header and status bar.

`human_only` and `system_only` can both be on, which yields an empty list with no explanation.

The three shadow-backed chips (locked, no password, expired) are skipped entirely when shadow is unavailable or when any user remaining after the other chips lacks a shadow record (`src/search.rs:218`). The menu still lets you toggle them and saves them to disk. Render those entries as disabled with the reason, and show the active chip set in the table title.

The users menu is a multi-toggle list while the groups menu is a radio. `filter.conf` also carries a `users_filter` key that nothing in the UI can set, and "Show all" resets the chips but not that key (`src/app/update.rs:381`), so a hand-edited `users_filter = OnlySystemIds` survives "Show all". Pick one model.

### 10. Theme file is mostly decorative

`Theme` has ten fields. The renderer reads `title`, `border`, `header_fg`, `status_fg`, `status_bg`. `text`, `muted`, `header_bg`, `highlight_fg`, and `highlight_bg` are parsed, validated, written back to disk, and never applied. Row selection is hardcoded to `Color::Yellow` bold in four places (`src/ui/users.rs:36` and `:136`, `src/ui/groups.rs:33` and `:118`). A user who edits `highlight_bg` gets no change and no error. Either wire them up or drop them from the file. The `_muted` alias accepted by the parser is an internal field name leaking into user config.

Related: the default theme is Catppuccin Mocha with RGB values. Named and indexed colors are accepted, so a user can work around a terminal without truecolor, but the dark fallback in `Theme::dark` is unreachable from config and there is no preset selector. Untested how badly the RGB defaults degrade on a 16-color terminal.

### 11. Details pane regressed

v0.3.0 showed full name, primary group name, shell validity and interactivity, password last-change and expiry, sudo membership, SSH key count, and process count. The current pane shows six lines (`src/ui/users.rs:94`) and drops the full name even though `SystemUser.full_name` is loaded and searched. `ShadowStatus` still carries `last_change_days` and `expire_abs_days` and nothing displays them.

Group details print `/etc/group mtime days: 20729`. Days since the epoch means nothing to a person. Show a date or "3 days ago". The line `Shadow (known) locked=0 empty=0 expired=0` reads like a debug dump.

The details block is `Min(7)` and the members block `Min(5)` (`src/ui/mod.rs:48`), so ratatui splits them about evenly. Details needs eight lines and the rest is blank while the member list below is cramped.

### 12. Keybindings panel prints enum names

`render_keybinds_panel` sorts by `format_action` in a `BTreeMap`, so the panel reads `DeleteSelection: Delete`, `Ignore: Esc`, `MoveLeftPage: Left, h`, `ToggleKeybindsPane: K, Shift+K, Shift+k`. The v0.3.0 panel had General, Navigation, Contextual, and per-tab groups with human labels. Restore that grouping, hide `Ignore`, and collapse modifier duplicates.

The panel also vanishes below 90 columns even when toggled on (`src/ui/mod.rs:33`), with no indication that it is hidden.

### 13. Help modal is one line

`ModalState::Help` renders a single string (`src/ui/components.rs:90`). The `scroll` field is updated by Up/Down/PageUp/PageDown and never passed to the widget. The old `render_help_modal` in `git show d1cb11a:src/ui/components.rs` had grouped, scrollable content and is a good starting point.

### 14. Developer wording in user-facing text

Examples pulled from the code:

- The confirmation modal is titled "Confirm prepared operation" and its first line is always "Apply the exact prepared operation" (`src/app/update.rs:1150`). Neither says what will happen. Use the intent: "Change shell of alice to /bin/zsh".
- Result modal: `completed: 2, skipped: 0` and `reconciliation: verified`. Say "Added alice to wheel and docker" and put the step counts on a second line.
- Removing a member from the groups tab shows `Remove wheel:alice?` because `group_name` carries an encoded `group:user` string (`src/app/update.rs:165`).
- `Protected identities require explicit policy allowlisting.` when trying to delete a system user.
- Error codes like `E-VALIDATION: invalid user name: must contain 1 to 32 bytes` are fine for logs but the code prefix adds noise on screen.
- "Expire password" in the password menu means "force change at next login". Say that.
- "Add to sudo group" does not say which group. The default is `wheel`, which is wrong on Debian and Ubuntu. Show the resolved name from `sudo_group_name()` in the label, and document `UGM_SUDO_GROUP` in the README, which currently does not mention it. (See finding 6 for why the checkbox cannot work at all today.)

### 15. Mouse capture without mouse handling

`TerminalResources::acquire_with` enables mouse capture (`src/terminal.rs:70`). `run_app` matches only `Event::Key` (`src/app/update.rs:28`). The net effect is that the user cannot drag-select text in the terminal (a common way to copy a UID or path), and gets nothing in return. Either handle clicks and wheel scroll or drop the capture.

### 16. Escape unwinds everything

Modal flows are up to four levels deep (Actions, Modify, Modify details, Change username). Esc closes to the main view from any depth. Backspace goes one level up in `ModifyMenu` and `ModifyDetailsMenu` only (`src/app/update.rs:426` and `:456`). From `ModifyGroupsAdd`, `ModifyShell`, `ChangePassword`, or any group modal there is no way back one step. Keep a modal stack, make Esc pop one level, and show "Esc back" in every modal footer.

### 17. Inconsistent modal keys

- Menus accept Up/Down/j/k.
- `DeleteConfirm`, `GroupDeleteConfirm`, `ConfirmRemoveUserFromGroup` accept Left/Right only. j/k/Up/Down do nothing.
- `OperationConfirm` accepts all four arrows.
- `ChangePassword` and `UserAddInput` accept Up/Down but not j/k, which is correct for text fields but the boundary is invisible.
- `h`/`l` are page keys in the main view (`src/app/keymap.rs:58`), which nobody expects from a vim binding, while Left/Right also page instead of switching panes.

Pick one movement scheme for choice lists and one for yes/no dialogs, and print the keys in the modal footer.

### 18. Text input ergonomics

- Rename username and full name modals open with an empty string. Prefill with the current value so an edit is an edit, not a retype.
- No cursor glyph, so an empty field looks the same as a field that has not received focus.
- Length caps are silent and inconsistent. Create user and group name inputs stop at 32 bytes (`src/app/update.rs:804`, `:904`, `:1029`). The rename and full-name input accepts up to 4096 bytes locally (`:505`) but the bridge rejects a GECOS over 256 bytes and a username over 32 only at submit time. The caps count bytes, so multibyte input can overshoot.
- The two password fields share one limit check, `password.len().max(confirm.len()) < 1024` (`:660`, `:1031`). Fill one field to the cap and the other stops accepting input.
- Create user checks `!password.is_empty() && !password.matches(confirm)` (`:1040`). An empty password with a non-empty confirm passes and creates the account without a password.
- Shell picker opens on index 0 rather than the user's current shell, and does not mark the current one.
- Password fields have no strength hint and no way to reveal what was typed.
- Renaming a user runs `usermod -l` only (`src/sys/mod.rs:542`). The home directory keeps its old name. The dialog should say so.

### 19. Candidate lists

Add-member and add-group lists page 12 rows at a time with no "page 3 of 9", no type-to-filter, no selected count, and no select-all. On a host with a few hundred users, adding one to a group means holding Down. Both lists cut off at 1024 entries without saying so (`src/app/update.rs:559`, `:950`). Enter with nothing checked applies to the row under the cursor, which is handy but undocumented.

### 20. No interrupt binding

Raw mode disables SIGINT. `Quit` only fires in `InputMode::Normal`, and no default binding uses a Ctrl modifier. In search and text fields the key handler ignores modifiers, so Ctrl+C inserts a `c` (`src/app/update.rs:127`, `:505`). With the default bindings, Esc followed by `q` gets out, but a user who tries Ctrl+C first will think the app hung. Treat Ctrl+C as quit in every mode, with a confirmation only when a `pending_operation` exists.

### 21. Enter on "No" does nothing (found in review)

In `handle_user_delete`, `handle_member_delete`, and `handle_group_delete`, Enter is handled only when the cursor is on Yes (`src/app/update.rs:713`, `:738`, `:824`). With No selected, Enter is a no-op and the user has to discover Esc. For a protected user the `Enter if !*allowed` arm fires regardless of cursor, so pressing Enter on No shows the "Protected identities" error. Close the dialog when No is confirmed.

### 22. Child pane keeps a stale cursor (found in review)

`move_selection` updates the parent index and calls `capture_selection_identities`, but never clamps `selected_user_group_index` or `selected_group_member_index` to the new parent's list (`src/app/update.rs:230`). Move from a user with six groups, cursor on the sixth, to a user with one group: the "Member of" pane shows no highlighted row, and Enter or Delete on it does nothing. Clamp child indices whenever the parent changes.

### 23. Groups-tab filter hides memberships in the Users tab (found in review)

`selected_user_groups` filters `self.groups`, the list after the Groups-tab filter (`src/app/mod.rs:1020`). With "System GIDs" active on the Groups tab, a user's "Member of" pane on the Users tab silently drops their user-range groups. Resolve memberships against `groups_all`.

### 24. Bulk confirmations are cut at 12 lines (found in review)

`OperationConfirm` renders `preview.iter().take(12)` and prints "additional steps omitted from this modal" for the rest (`src/ui/components.rs:96`). The modal has no scrolling. Adding a user to fifteen groups means confirming three commands nobody saw. Make the preview scrollable, or refuse to show Apply until the user has paged to the end.

## P3: polish

### 25. Page size is a constant

`rows_per_page` is fixed at 10 (`src/app/mod.rs:815`) and shown in the status bar as `rows/page:10`. The tables compute their own `visible_rows(area)` from the terminal height. PageDown therefore jumps by 10 rows on a 40-row table, and the status bar number is meaningless. Derive the page size from the last rendered area.

### 26. Header and status bar overlap, and the stale reason is clipped

The header block has "usrgrp-manager" as its border title and again as the first word of its text, then `users:45 groups:89 shadow:known`. The status bar repeats users, groups, and shadow. Neither shows the input mode, the search query, active filters, the selected row position (`12/45`), or key hints.

The header is `Length(4)`, which leaves two interior rows inside the border. The `STALE: <reason>` text is appended as a third line (`src/ui/mod.rs:60`), so it is never visible. The status bar shows only the word `STALE` with no reason. Move the reason to the status bar or an Info modal.

### 27. Empty states

A filtered-out list is blank. Show "No users match /alice with filters: locked" so the user knows it is a filter and not a data problem. When the snapshot failed, put the stale reason from finding 26 in the empty-state text too.

### 28. No command-line interface

`main.rs` parses no arguments. `usrgrp-manager --version` enters the TUI. Add `--version`, `--help`, `--config-dir`, and a `--dry-run` flag that only ever shows previews. v0.1.0 had a `USRGRP_MANAGER_LOG` env var; a `--log-file` option would help bug reports. Today the only diagnostics are the bounded Info modal strings plus the one-line startup, application, and cleanup errors that `main.rs` prints to stderr on exit.

### 29. Wiki and README drift

- `wiki/Keyboard-Shortcuts.md` lists `a`, `e`, `d`, `r`, and `wiki/Quick-Start.md` lists `a`, `e`, `d`. None are bound. Actual keys are `n`, Enter, Delete, and there is no refresh.
- `wiki/Home.md` says the low-GID warning "does not block the action". It now blocks.
- `wiki/Releases.md` points to `docs: RELEASE.md`, which does not exist. The files are `RELEASE_v0.1.0.md` and `RELEASE_v0.3.0.md`, and `Cargo.toml` says 0.2.0.
- Wiki tells people to run under `sudo`. README says that is unnecessary and per-action elevation is the model. Pick one story.
- README does not mention `UGM_SUDO_GROUP`, the keybinds panel, or the filter chips, and it describes a runtime policy allowlist that has no user-facing mechanism (finding 6).

### 30. Missing actions

The pre-rewrite roadmap (deleted in `e1b0ae4`, readable via `git show d1cb11a:docs/roadmap.md`) lists these, and the data to support them is already loaded:

- Lock and unlock (`usermod -L/-U`). The details pane shows `locked=true` but nothing can change it.
- Change primary group and home directory.
- Set account expiry date (`chage -E`). Shadow expiry is parsed but not shown.
- Sort by name, UID, or shell. Users are UID-ordered and groups GID-ordered, with no toggle.
- Multi-select in the main tables for bulk group changes. Multi-select exists only inside the candidate modals.
- Copy selected value to clipboard, or at least a way to select text (see finding 15).

### 31. Smaller items

- Groups tab Enter opens "Create, Delete, Modify" for the highlighted group (`src/ui/groups.rs:151`). "Create" does not belong in a per-group menu; `n` already does it.
- Enter on a member in the Members pane jumps straight to a remove confirmation. A menu with "Go to user" and "Remove from group" would be less surprising.
- The `Delete` key is the only delete binding. Laptop keyboards without a Delete key (most Macs) need `Fn+Backspace`. Offer `d` or `x` as an alternative default.
- Selection wraps from the last row to the first (`src/app/update.rs:256`). Some people like that, some do not. Make it a config key.
- Startup does a synchronous refresh including a `stat` of every home directory and a read of every `authorized_keys`, up to 10,000 users (`src/app/mod.rs:572`). On NFS or autofs homes this can take seconds with a blank screen. Draw a "loading" frame first, or defer home diagnostics. Not measured.
- Minimum size is 30x8 but the three-column layout only kicks in at 90 columns. Between 30 and 90 the details pane gets half the width; table cells truncate horizontally and the wrapped details text clips vertically. Consider a stacked layout below 70 columns.
- `filter.conf` is rewritten on every chip toggle. Fine, but a write failure only shows as the same hidden `config-error` flag from finding 7.

## Suggested order

1. Findings 1, 2, 3, 6. These stop the app from discarding input or targeting the wrong account. Not sized; 3 and 6 touch a renderer as well as a handler.
2. Findings 4, 7, 9, 26. Make current state visible: query, filters, config errors, stale reason.
3. Findings 8, 5, 21, 22, 23. Refresh key, delete dialog, and the three selection bugs.
4. Restore the v0.3.0 help, keybinds panel, and details pane from `d1cb11a` (findings 11, 12, 13). The old code needs adapting to the new `AppState`; it is a reference, not a drop-in.
5. Wording pass over every modal string (finding 14) and the modal key scheme (16, 17).
6. Docs sync (29).
7. Everything else as time allows.
