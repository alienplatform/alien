# SyncAcquireResponseDeploymentEnvironmentVariables

Snapshot of environment variables at a point in time

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentEnvironmentVariables } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentEnvironmentVariables = {
  createdAt: "1705652773860",
  hash: "<value>",
  variables: [
    {
      name: "<value>",
      type: "plain",
      value: "<value>",
    },
  ],
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `createdAt`                                                                                          | *string*                                                                                             | :heavy_check_mark:                                                                                   | ISO 8601 timestamp when snapshot was created                                                         |
| `hash`                                                                                               | *string*                                                                                             | :heavy_check_mark:                                                                                   | Deterministic hash of all variables (for change detection)                                           |
| `variables`                                                                                          | [models.SyncAcquireResponseDeploymentVariable](../models/syncacquireresponsedeploymentvariable.md)[] | :heavy_check_mark:                                                                                   | Environment variables in the snapshot                                                                |