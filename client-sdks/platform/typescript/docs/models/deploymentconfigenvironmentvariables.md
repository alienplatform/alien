# DeploymentConfigEnvironmentVariables

Snapshot of environment variables at a point in time

## Example Usage

```typescript
import { DeploymentConfigEnvironmentVariables } from "@alienplatform/platform-api/models";

let value: DeploymentConfigEnvironmentVariables = {
  createdAt: "1729708872692",
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

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `createdAt`                                                                | *string*                                                                   | :heavy_check_mark:                                                         | ISO 8601 timestamp when snapshot was created                               |
| `hash`                                                                     | *string*                                                                   | :heavy_check_mark:                                                         | Deterministic hash of all variables (for change detection)                 |
| `variables`                                                                | [models.DeploymentConfigVariable](../models/deploymentconfigvariable.md)[] | :heavy_check_mark:                                                         | Environment variables in the snapshot                                      |