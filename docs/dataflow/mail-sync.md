# Mail sync dataflow

The IMAP sync of an account (`sync_account` in `src/mail/imap_sync.rs`): it connects, lists and records the folders, then for each folder selects it, looks for new mail, fetches it and records what it did in the mail index. The diagram below is not written by hand: it is recorded from a real run of the mail-env test against the seeded Dovecot of `tests/mail-env`, and every arrow goes from the sync (the caller) to the other side of the step it names; the `folder` block is the work on one folder (DEV-026).

To regenerate it after a change to the sync: `tests/mail-env/mail-env.sh up`, then `SHALE_UPDATE_DATAFLOW=1 cargo test -- --ignored mail_env_password_account`, and commit the result.

<!-- dataflow:begin mail_sync -->
```mermaid
sequenceDiagram
    participant sync
    participant imap
    participant store
    sync->>imap: connect
    sync->>imap: list_folders
    loop repeated
        sync->>store: record_folder
    end
    loop folder
        sync->>imap: select_folder
        sync->>store: adopt_validity
        sync->>imap: search_new
        sync->>imap: fetch
        opt sometimes
            sync->>store: store_messages
        end
        sync->>store: store_folder_state
    end
    sync->>store: backup
```
<!-- dataflow:end mail_sync -->
