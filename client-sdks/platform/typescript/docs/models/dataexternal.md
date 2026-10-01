# DataExternal

## Example Usage

```typescript
import { DataExternal } from "@alienplatform/platform-api/models";

let value: DataExternal = {
  provider: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: true,
    stale: false,
  },
  backend: "external",
};
```

## Fields

| Field                                                                                                                                                         | Type                                                                                                                                                          | Required                                                                                                                                                      | Description                                                                                                                                                   |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `availability`                                                                                                                                                | *models.Availability*                                                                                                                                         | :heavy_minus_sign:                                                                                                                                            | N/A                                                                                                                                                           |
| `provider`                                                                                                                                                    | *string*                                                                                                                                                      | :heavy_check_mark:                                                                                                                                            | The BYO-key provider serving this binding (e.g. "openai"). Used on the Local<br/>platform, where the app brings its own provider key instead of an ambient cloud. |
| `status`                                                                                                                                                      | [models.SyncReconcileRequestStatus69](../models/syncreconcilerequeststatus69.md)                                                                              | :heavy_check_mark:                                                                                                                                            | N/A                                                                                                                                                           |
| `backend`                                                                                                                                                     | *"external"*                                                                                                                                                  | :heavy_check_mark:                                                                                                                                            | N/A                                                                                                                                                           |