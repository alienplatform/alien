# TargetDeploymentEnvironmentVariables

Snapshot of environment variables at a point in time

## Example Usage

```typescript
import { TargetDeploymentEnvironmentVariables } from "@alienplatform/platform-api/models";

let value: TargetDeploymentEnvironmentVariables = {
  createdAt: "1708590249188",
  hash: "<value>",
  variables: [],
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `createdAt`                                                                | *string*                                                                   | :heavy_check_mark:                                                         | ISO 8601 timestamp when snapshot was created                               |
| `hash`                                                                     | *string*                                                                   | :heavy_check_mark:                                                         | Deterministic hash of all variables (for change detection)                 |
| `variables`                                                                | [models.TargetDeploymentVariable](../models/targetdeploymentvariable.md)[] | :heavy_check_mark:                                                         | Environment variables in the snapshot                                      |