# Spec: Social feeds (SOC)

**IS NOW** = true of the code today (⚠ = known defect) · **SHOULD BE** = agreed target.
Format: [ADR-0001](../decisions/0001-spec-driven-development.md). Decisions: [ADR-0007](../decisions/0007-social-feeds.md).
Tests are named `file.rs::test_fn` (all under `src/`). The Mastodon and Bluesky code is tested against
small pretend servers driven through real `curl`; it has not been tried against the real servers
(SOC-016).

## Accounts and secrets

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| SOC-001 | A Mastodon account signs in with a read-only OAuth code (the user opens the server's page and pastes the code) or a pasted access token; the feeds are Home and the user's lists; timelines page with a cursor; a thread is fetched from the status context. | IS NOW | `social/mastodon.rs` | `mastodon.rs::sign_in_feeds_paging_thread_and_media_against_a_mock_server`, `mastodon.rs::status_parsing`, `mastodon.rs::errors_are_readable` |
| SOC-002 | A Bluesky account signs in with a handle and an **app password**; the feeds are Following and the saved feeds; an expired login is renewed by itself (refresh token, else the app password). | IS NOW | `social/bluesky.rs` | `bluesky.rs::sign_in_expiry_feeds_paging_thread_and_media_against_a_mock_server`, `bluesky.rs::saved_feeds_from_preferences` |
| SOC-003 | An account is saved in the settings file without its secret when the system keyring holds it (the entry carries the secret only as a fallback, in the owner-only file); old settings files without accounts still load; secrets never appear in debug output. | IS NOW | `config.rs`, `social/mod.rs` (`SocialAccountEntry`), `secrets::social_*` | `config.rs::social_accounts_are_saved_without_secrets_that_the_keyring_holds`, `social/mod.rs::network_names_and_debug_redaction`, `bluesky.rs::sessions_do_not_show_their_tokens`, `secrets.rs::social_secrets_have_their_own_keyring_entries` |
| SOC-004 | The accounts dialog checks the input (server, handle, app password, code, token) before it asks the network, shows every error inline, and shows a spinner while a sign-in runs; the dialog cannot be closed by Esc during a sign-in. | IS NOW | `social/accounts_ui.rs` | `accounts_ui.rs::the_account_dialog_checks_its_input_before_it_asks_the_network` |
| SOC-005 | Removing an account forgets its feeds and posts and clears its keyring entry. | IS NOW | `app/social.rs`, `social/accounts_ui.rs` | `accounts_ui.rs::the_account_dialog_checks_its_input_before_it_asks_the_network`, `app.rs::saving_a_post_opens_the_import_dialog_and_the_note_lands_in_the_vault` (the keyring call itself is untested) |

## The feed view

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| SOC-006 | The Feeds tab shows the posts of the chosen account and feed in a centred column: avatar, name, handle, age, "boosted / reposted by", pictures and video previews (a play mark), the quoted post, the link card, the counts, and the buttons Save to vault, Open in browser and Copy link. A content warning hides text and pictures until Show is clicked. Without an account there is an "Add account" start. | IS NOW | `social/feed_ui.rs` | `feed_ui.rs::the_feed_shows_posts_hides_content_warnings_and_saves_without_network` |
| SOC-008 | Links in a post text are clickable; only `http` and `https` addresses are opened in the browser. | IS NOW | `feed_ui.rs::segments`, `feed_ui::open_web` | `feed_ui.rs::post_text_becomes_pieces_with_links` (the click itself is checked by hand) |
| SOC-009 | Loaded posts are kept per account and feed (switching tabs does not fetch again). The next page loads once when the end of the list is on screen; posts already shown are not added twice; a page with nothing new ends the list; an error stops the loading until Retry is clicked. | IS NOW | `social/state.rs`, `feed_ui.rs` | `state.rs::pages_are_added_without_doubles_and_the_end_is_noticed`, `feed_ui.rs::the_end_of_the_list_loads_the_next_page_once_and_an_error_waits_for_retry` |
| SOC-010 | Feed, page, save and sign-in jobs run on background threads and always end with a result or an error message, also when the job panics. | IS NOW | `social/state.rs`, `accounts_ui.rs`, `images.rs` | `state.rs::a_crashing_job_ends_with_an_error` |
| SOC-013 | Pictures (avatars, previews) are fetched by 4 background workers into `~/.cache/shale/social/<hash>.<ext>`, once per address, without any login, and shown from the file; the window never waits for them. | IS NOW | `social/images.rs` | `images.rs::local_files_are_used_as_they_are_and_names_are_stable` |

## Saving posts

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| SOC-007 | "Save to vault" fetches the thread and the media of the post in the background (spinner on the button), then opens the Import dialog with one item: kind "Post" or "Thread (N posts)", author, date, one tick box per file, and a warning when files or the rest of the thread could not be fetched. Nothing is written before the user presses Import. | IS NOW | `social/state.rs`, `app/social.rs`, `import.rs`, `import_dialog.rs` | `app.rs::saving_a_post_opens_the_import_dialog_and_the_note_lands_in_the_vault`, `feed_ui.rs::the_feed_shows_posts_hides_content_warnings_and_saves_without_network` |
| SOC-011 | A post becomes one note: properties (type, network, author, date, url, post-id, thread ids, tags), the author, the date, a **Source** link back to the post, "boosted by", the text (a thread: all posts of its author in order, numbered), pictures embedded from `z_media/<YYYY>/<MM>/`, their descriptions, the quote and the link card. A file that is unticked or could not be downloaded stays a link to its web address. | IS NOW | `social/note.rs`, `import::import` | `import.rs::a_saved_post_becomes_a_note_with_pictures_and_a_second_import_is_spotted`, `note.rs::single_post_note`, `note.rs::thread_note_layout`, `note.rs::failed_thread_and_missing_paths_fall_back_to_links` |
| SOC-012 | A post imported before (also as one post of a saved thread) is marked "Imported before" and unticked. Posts go to the folder used for posts last time, first `Social`. | IS NOW | `import::mark_duplicates`, `app/drops.rs` | `import.rs::a_saved_post_becomes_a_note_with_pictures_and_a_second_import_is_spotted`, `app.rs::saving_a_post_opens_the_import_dialog_and_the_note_lands_in_the_vault` |
| SOC-014 | The thread of a post is the posts of the same author around it: the unbroken run of their posts above it, then their replies to posts already in the thread, in the order they were written. Replies of other people are not saved. | IS NOW | `social::self_thread` | `social/mod.rs::self_thread_rule`, `mastodon.rs::thread_from_a_context_fixture`, `bluesky.rs::thread_from_a_fixture` |

## Network

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| SOC-015 | Servers are reached only over `https://` (`http://` only on this computer) through `curl` with the secret on standard input; the token goes only to the account's own server, never to a picture server; downloads are limited to 64 MB, answers to 20 MB. | IS NOW | `social/mod.rs`, `cal_dav::request_bytes` | `social/mod.rs::urls_and_names`, `images.rs::local_files_are_used_as_they_are_and_names_are_stable` (refuses plain http) |
| SOC-016 | Mastodon and Bluesky are tried by hand against real servers before a release. | SHOULD BE | — | manual |
| SOC-017 | The picture cache folder is pruned (oldest files first) when it passes a size limit. ⚠ Today it only grows. | SHOULD BE | `social/images.rs` | — |
| SOC-018 | The author's contact note is linked in the saved note when there is one (as for e-mails). | SHOULD BE | `import::find_people` | — |
