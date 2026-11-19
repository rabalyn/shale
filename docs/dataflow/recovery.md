# Recovery dataflow

Text that could not be saved at exit (`src/recovery.rs`): it is written into the recovery folder, listed at the next start and marked seen when the notice is dismissed. The diagram below is recorded from real runs of the `dataflow_recovery_rescue`, `dataflow_recovery_unseen` and `dataflow_recovery_dismiss` tests, not written by hand (DEV-026, DEV-031).

To regenerate it after a change: `SHALE_UPDATE_DATAFLOW=1 cargo test dataflow_recovery`, and commit the result.

<!-- dataflow:begin recovery_rescue -->
```mermaid
sequenceDiagram
    participant app
    participant disk
    app->>disk: write_copy
```
<!-- dataflow:end recovery_rescue -->

<!-- dataflow:begin recovery_unseen -->
```mermaid
sequenceDiagram
    participant app
    participant disk
    app->>disk: list_days
```
<!-- dataflow:end recovery_unseen -->

<!-- dataflow:begin recovery_dismiss -->
```mermaid
sequenceDiagram
    participant app
    participant disk
    app->>disk: mark_seen
```
<!-- dataflow:end recovery_dismiss -->
