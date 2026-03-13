# DeploymentDetailResponseTargetEnvironmentVariables

Snapshot of target environment variables for the deployment

## Example Usage

```typescript
import { DeploymentDetailResponseTargetEnvironmentVariables } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseTargetEnvironmentVariables = {
  variables: [],
  hash: "<value>",
  createdAt: new Date("2026-04-12T20:04:59.257Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `variables`                                                                                   | [models.EnvironmentVariableConfig](../models/environmentvariableconfig.md)[]                  | :heavy_check_mark:                                                                            | Environment variables in the snapshot                                                         |
| `hash`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | Deterministic hash of all variables for change detection                                      |
| `createdAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | ISO 8601 timestamp when snapshot was created                                                  |