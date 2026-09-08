# KeyHeartbeatStatus

## Example Usage

```typescript
import { KeyHeartbeatStatus } from "@alienplatform/manager-api/models";

let value: KeyHeartbeatStatus = {
  health: "unknown",
  lifecycle: "unknown",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `health`                                                             | [models.ObservedHealth](../models/observedhealth.md)                 | :heavy_check_mark:                                                   | N/A                                                                  |
| `lifecycle`                                                          | [models.ProviderLifecycleState](../models/providerlifecyclestate.md) | :heavy_check_mark:                                                   | N/A                                                                  |
| `message`                                                            | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |