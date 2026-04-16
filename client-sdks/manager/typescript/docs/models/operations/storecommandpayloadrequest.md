# StoreCommandPayloadRequest

## Example Usage

```typescript
import { StoreCommandPayloadRequest } from "@alienplatform/manager-api/models/operations";

let value: StoreCommandPayloadRequest = {
  commandId: "<id>",
  storePayloadRequest: {},
};
```

## Fields

| Field                                                             | Type                                                              | Required                                                          | Description                                                       |
| ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| `commandId`                                                       | *string*                                                          | :heavy_check_mark:                                                | Command identifier                                                |
| `storePayloadRequest`                                             | [models.StorePayloadRequest](../../models/storepayloadrequest.md) | :heavy_check_mark:                                                | N/A                                                               |