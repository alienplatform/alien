# SyncReconcileResponseEnvironmentVariables

Snapshot of environment variables at a point in time

## Example Usage

```typescript
import { SyncReconcileResponseEnvironmentVariables } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseEnvironmentVariables = {
  createdAt: "1728118720601",
  hash: "<value>",
  variables: [],
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `createdAt`                                                                          | *string*                                                                             | :heavy_check_mark:                                                                   | ISO 8601 timestamp when snapshot was created                                         |
| `hash`                                                                               | *string*                                                                             | :heavy_check_mark:                                                                   | Deterministic hash of all variables (for change detection)                           |
| `variables`                                                                          | [models.SyncReconcileResponseVariable](../models/syncreconcileresponsevariable.md)[] | :heavy_check_mark:                                                                   | Environment variables in the snapshot                                                |