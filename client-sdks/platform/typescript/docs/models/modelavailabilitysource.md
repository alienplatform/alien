# ModelAvailabilitySource

## Example Usage

```typescript
import { ModelAvailabilitySource } from "@alienplatform/platform-api/models";

let value: ModelAvailabilitySource = {
  deploymentId: "<id>",
  resourceId: "<id>",
  observedAt: new Date("2026-12-30T11:30:38.604Z"),
  status: "current",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `deploymentId`                                                                                | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `resourceId`                                                                                  | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `observedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `status`                                                                                      | [models.ModelAvailabilitySourceStatus](../models/modelavailabilitysourcestatus.md)            | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `availability`                                                                                | [models.AiAvailabilityObservation](../models/aiavailabilityobservation.md)                    | :heavy_minus_sign:                                                                            | N/A                                                                                           |