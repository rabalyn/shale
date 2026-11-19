# Data usage dataflow

The scan of what is saved (`scan::saved`) and the clear (`clear`) and the prune (`prune`) of the caches (in `src/data_usage/mod.rs`). The diagrams below are recorded from real runs of the `dataflow_data_usage_scan`, `dataflow_data_usage_clear` and `dataflow_data_usage_prune` tests on temporary folders, not written by hand (DEV-026, DEV-031).

To regenerate them after a change: `SHALE_UPDATE_DATAFLOW=1 cargo test dataflow_data_usage`, and commit the result.

<!-- dataflow:begin data_usage_scan -->
```mermaid
sequenceDiagram
    participant app
    participant disk
    loop repeated
        app->>disk: read_folder
    end
```
<!-- dataflow:end data_usage_scan -->

<!-- dataflow:begin data_usage_clear -->
```mermaid
sequenceDiagram
    participant app
    participant disk
    app->>disk: remove_files
```
<!-- dataflow:end data_usage_clear -->

<!-- dataflow:begin data_usage_prune -->
```mermaid
sequenceDiagram
    participant app
    participant disk
    app->>disk: remove_files
```
<!-- dataflow:end data_usage_prune -->
