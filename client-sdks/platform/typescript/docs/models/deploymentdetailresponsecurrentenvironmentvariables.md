# DeploymentDetailResponseCurrentEnvironmentVariables

Snapshot of current environment variables for the deployment

## Example Usage

```typescript
import { DeploymentDetailResponseCurrentEnvironmentVariables } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseCurrentEnvironmentVariables = {
  variables: [
    {
      name: "<value>",
      value: "<value>",
      type: "secret",
      targetResources: [
        "<value 1>",
      ],
    },
  ],
  hash: "<value>",
  createdAt: new Date("2026-08-12T16:01:30.661Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `variables`                                                                                   | [models.EnvironmentVariableConfig](../models/environmentvariableconfig.md)[]                  | :heavy_check_mark:                                                                            | Environment variables in the snapshot                                                         |
| `hash`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | Deterministic hash of all variables for change detection                                      |
| `createdAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | ISO 8601 timestamp when snapshot was created                                                  |