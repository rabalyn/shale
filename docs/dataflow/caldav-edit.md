# CalDAV edit dataflow

Writing to a CalDAV calendar: creating, changing and deleting an event on the server (`put_event` and `delete_event` in `src/calendar/dav.rs`, which check the version tag or, without one, what the server still has), moving an event to another calendar (`save_event` in `src/calendar/store.rs`: write to the new calendar first, then remove from the old one), and a conflict: a save or a delete that the server refuses because somebody changed the event meanwhile. The conflict has no spans of its own; its diagram is the `caldav_edit` steps that the refused writes run (shared code), shown with the name of their flow, `caldav_edit.<step>`. The diagrams are not written by hand: they are recorded from real runs of the caldav-env tests against the seeded Radicale of `tests/caldav-env`, and every arrow goes from the caller to the other side of the step it names (DEV-026, DEV-028).

To regenerate them after a change: `tests/caldav-env/caldav-env.sh up`, then `SHALE_UPDATE_DATAFLOW=1 cargo test -- --ignored caldav_env_creates_edits caldav_env_moves_an_event caldav_env_a_change_on_the_server`, and commit the result.

<!-- dataflow:begin caldav_edit -->
```mermaid
sequenceDiagram
    participant calendar
    participant caldav
    opt sometimes
        calendar->>caldav: check_server
    end
    opt sometimes
        calendar->>caldav: delete
    end
    opt sometimes
        calendar->>caldav: put
    end
```
<!-- dataflow:end caldav_edit -->

<!-- dataflow:begin caldav_move -->
```mermaid
sequenceDiagram
    participant calendar
    participant caldav
    calendar->>caldav: write_new
    Note over calendar: caldav_edit
    calendar->>caldav: caldav_edit.put
    calendar->>caldav: remove_old
    Note over calendar: caldav_edit
    calendar->>caldav: caldav_edit.delete
```
<!-- dataflow:end caldav_move -->

<!-- dataflow:begin caldav_conflict -->
```mermaid
sequenceDiagram
    participant calendar
    participant caldav
    loop caldav_edit
        opt sometimes
            calendar->>caldav: caldav_edit.delete
        end
        opt sometimes
            calendar->>caldav: caldav_edit.fetch_etag
        end
        opt sometimes
            calendar->>caldav: caldav_edit.check_server
        end
        opt sometimes
            calendar->>caldav: caldav_edit.put
        end
    end
```
<!-- dataflow:end caldav_conflict -->
