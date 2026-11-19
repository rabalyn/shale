# CalDAV edit dataflow

Writing to a CalDAV calendar: creating and changing (`caldav_put`) and deleting (`caldav_delete`) an event on the server (`put_event` and `delete_event` in `src/calendar/dav.rs`, which check the version tag or, without one, what the server still has), moving an event to another calendar (`save_event` in `src/calendar/store.rs`: write to the new calendar first, then remove from the old one), and a conflict: a save or a delete that the server refuses because somebody changed the event meanwhile. The conflict has no spans of its own; its diagram is the `caldav_put` and `caldav_delete` steps that the refused writes run (shared code), shown with the name of their flow (`caldav_put.<step>`), in the order they ran. The diagrams are not written by hand: they are recorded from real runs of the caldav-env tests against the seeded Radicale of `tests/caldav-env`, and every arrow goes from the caller to the other side of the step it names (DEV-026, DEV-028).

To regenerate them after a change: `tests/caldav-env/caldav-env.sh up`, then `SHALE_UPDATE_DATAFLOW=1 cargo test -- --ignored caldav_env_creates_edits caldav_env_moves_an_event caldav_env_a_change_on_the_server`, and commit the result.

<!-- dataflow:begin caldav_put -->
```mermaid
sequenceDiagram
    participant calendar
    participant caldav
    calendar->>caldav: put
```
<!-- dataflow:end caldav_put -->

<!-- dataflow:begin caldav_delete -->
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
```
<!-- dataflow:end caldav_delete -->

<!-- dataflow:begin caldav_move -->
```mermaid
sequenceDiagram
    participant calendar
    participant caldav
    calendar->>caldav: write_new
    rect rgba(127, 127, 127, 0.12)
        Note over calendar: caldav_put
        calendar->>caldav: caldav_put.put
    end
    calendar->>caldav: remove_old
    rect rgba(127, 127, 127, 0.12)
        Note over calendar: caldav_delete
        calendar->>caldav: caldav_delete.delete
    end
```
<!-- dataflow:end caldav_move -->

<!-- dataflow:begin caldav_conflict -->
```mermaid
sequenceDiagram
    participant calendar
    participant caldav
    loop caldav_put
        opt sometimes
            calendar->>caldav: caldav_put.fetch_etag
        end
        opt sometimes
            calendar->>caldav: caldav_put.check_server
        end
        opt sometimes
            calendar->>caldav: caldav_put.put
        end
    end
    loop caldav_delete
        opt sometimes
            calendar->>caldav: caldav_delete.check_server
        end
        opt sometimes
            calendar->>caldav: caldav_delete.delete
        end
    end
```
<!-- dataflow:end caldav_conflict -->
