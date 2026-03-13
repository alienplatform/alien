# SyncAcquireResponseEnvironmentVariables

Snapshot of environment variables at a point in time

## Example Usage

```typescript
import { SyncAcquireResponseEnvironmentVariables } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseEnvironmentVariables = {
  createdAt: "1726232482767",
  hash: "<value>",
  variables: [],
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `createdAt`                                                                      | *string*                                                                         | :heavy_check_mark:                                                               | ISO 8601 timestamp when snapshot was created                                     |
| `hash`                                                                           | *string*                                                                         | :heavy_check_mark:                                                               | Deterministic hash of all variables (for change detection)                       |
| `variables`                                                                      | [models.SyncAcquireResponseVariable](../models/syncacquireresponsevariable.md)[] | :heavy_check_mark:                                                               | Environment variables in the snapshot                                            |