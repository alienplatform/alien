# queue-dead-letter

End-to-end app for a Queue that names another Queue as its dead-letter target.

`tasks` dead-letters to `tasks-failed` after 5 failed deliveries. The `consumer` Worker is triggered by `tasks` (with batching); a message marked `poison` always gets a `retry` outcome, every other message succeeds.

- `POST /send` `{ good, poison, prefix? }` sends the messages to `tasks`.
- `GET /deliveries` lists one record per delivery (message id, delivery id, reported attempt).
- `POST /failed/drain` receives everything on `tasks-failed` (pull path) and acknowledges it.
