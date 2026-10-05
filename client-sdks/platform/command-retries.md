# Retrying a command invocation

An invocation can reach the manager even if the API response fails. Supply an
`idempotencyKey` when invoking a command with `params` so a retry can recover the
accepted command instead of executing it again.

Generate the key once per logical invocation, before calling the SDK:

```typescript
const request = {
  deploymentId,
  target: "api",
  name: "reindex",
  params: { full: true },
  idempotencyKey: crypto.randomUUID(),
};

const command = await sdk.commands.create(request, {
  retries: {
    strategy: "backoff",
    backoff: {
      initialInterval: 500,
      maxInterval: 5_000,
      exponent: 1.5,
      maxElapsedTime: 30_000,
    },
  },
});
```

Configured SDK retries preserve the same request and key. If the method still
fails with a retryable error, keep `request` and pass it to a later
`sdk.commands.create(request)` call. For retries across process restarts, save
the request and key before sending. Do not generate a new key for each attempt.
The SDK does not generate a key or enable retries by default.

The key must contain 1 to 128 characters. Replay applies to the same workspace,
authenticated caller, deployment, resolved target, and command name. The manager
retains the mapping for up to 24 hours. A new invocation needs a new key.

The first accepted request wins. Reusing its key with different params or a
different deadline returns the original command; it does not update that
command or reject the changed payload. Preserve the entire original request
when retrying. Every attempt must still pass current authorization checks.

Calls without a key keep their existing behavior and can execute again on
retry. A key on a metadata-only creation without `params` does not deduplicate
that creation. These retry guarantees require an API version that supports
`idempotencyKey`; older servers may ignore the field.
