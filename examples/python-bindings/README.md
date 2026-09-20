# Python bindings smoke workload

`app.py` exercises every application-facing Python resource binding through the
same public API used by an ordinary container. Link resources named `files`,
`cache`, `jobs`, `secrets`, `records`, `database`, `api`, `models`, `processor`,
and `runtime`, then run `python app.py` inside the workload.

The example intentionally contains no provider-specific configuration. Alien
injects binding descriptions and projected workload identity for the selected
platform.
