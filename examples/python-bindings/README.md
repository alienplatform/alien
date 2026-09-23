# Python bindings smoke workload

`app.py` exercises every application-facing Python resource binding through the
same public API used by an ordinary container. Link resources named `files`,
`cache`, `jobs`, `secrets`, `records`, `database`, `api`, `models`, `processor`,
and `runtime`, then run `python app.py` inside the workload.

The example intentionally contains no provider-specific configuration. Alien
injects binding descriptions and projected workload identity for the selected
platform.

Python containers can use the source toolchain with a checked-in `uv.lock`:

```ts
toolchain: {
  type: "python",
  pythonVersion: "3.12",
  package: "my-package", // omit for a single-package project
  command: ["python", "-m", "my_package"],
}
```

For an unpublished SDK build, set `ALIEN_PYTHON_SDK_WHEEL` to the target
platform's wheel. Alien stages it through a separate build context; the wheel
does not need to be copied into the application source. Once the SDK is
published, declare `alienplatform` in the project's locked dependencies and
omit that environment variable.
