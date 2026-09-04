# AiHeartbeatDataExternal

## Example Usage

```typescript
import { AiHeartbeatDataExternal } from "@alienplatform/manager-api/models";

let value: AiHeartbeatDataExternal = {
  provider: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "updating",
    partial: true,
    stale: true,
  },
  backend: "external",
};
```

## Fields

| Field                                                                                                                                                         | Type                                                                                                                                                          | Required                                                                                                                                                      | Description                                                                                                                                                   |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `availability`                                                                                                                                                | [models.AiAvailabilityObservation](../models/aiavailabilityobservation.md)                                                                                    | :heavy_minus_sign:                                                                                                                                            | N/A                                                                                                                                                           |
| `provider`                                                                                                                                                    | *string*                                                                                                                                                      | :heavy_check_mark:                                                                                                                                            | The BYO-key provider serving this binding (e.g. "openai"). Used on the Local<br/>platform, where the app brings its own provider key instead of an ambient cloud. |
| `status`                                                                                                                                                      | [models.AiHeartbeatStatus](../models/aiheartbeatstatus.md)                                                                                                    | :heavy_check_mark:                                                                                                                                            | N/A                                                                                                                                                           |
| `backend`                                                                                                                                                     | *"external"*                                                                                                                                                  | :heavy_check_mark:                                                                                                                                            | N/A                                                                                                                                                           |