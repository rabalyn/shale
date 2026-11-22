# Vault search dataflow

The search across everything Shale has indexed (`all_hits` in `src/search_all.rs`): the notes of the vault, the mail index and the post index. The diagram below is recorded from a real run of the `dataflow_vault_search` test, not written by hand (DEV-026, DEV-031).

To regenerate it after a change: `SHALE_UPDATE_DATAFLOW=1 cargo test dataflow_vault_search`, and commit the result.

<!-- dataflow:begin vault_search -->
```mermaid
sequenceDiagram
    participant search_view
    participant vault
    participant mail_index
    participant post_index
    search_view->>vault: search_notes
    search_view->>mail_index: search_mail
    search_view->>post_index: search_posts
```
<!-- dataflow:end vault_search -->
