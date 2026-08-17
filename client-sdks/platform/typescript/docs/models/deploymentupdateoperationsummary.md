# DeploymentUpdateOperationSummary

## Example Usage

```typescript
import { DeploymentUpdateOperationSummary } from "@alienplatform/platform-api/models";

let value: DeploymentUpdateOperationSummary = {
  id: "duop_0vtxpb1sw4sbcdwg2xo37q6",
  status: "applying",
  reasons: [],
  targetReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
  changedKeys: [],
  requestedAt: new Date("2024-03-09T19:13:29.092Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the deployment update operation.                                        | duop_0vtxpb1sw4sbcdwg2xo37q6                                                                  |
| `status`                                                                                      | [models.DeploymentUpdateOperationStatus](../models/deploymentupdateoperationstatus.md)        | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `reasons`                                                                                     | [models.DeploymentUpdateReason](../models/deploymentupdatereason.md)[]                        | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `targetReleaseId`                                                                             | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the release.                                                            | rel_WbhQgksrawSKIpEN0NAssHX9                                                                  |
| `changedKeys`                                                                                 | *string*[]                                                                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `requestedAt`                                                                                 | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `startedAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | N/A                                                                                           |                                                                                               |
| `completedAt`                                                                                 | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | N/A                                                                                           |                                                                                               |