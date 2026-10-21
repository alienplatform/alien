# SyncManagementExternalIdResponse

## Example Usage

```typescript
import { SyncManagementExternalIdResponse } from "@alienplatform/platform-api/models";

let value: SyncManagementExternalIdResponse = {
  managementExternalId: "<id>",
};
```

## Fields

| Field                                          | Type                                           | Required                                       | Description                                    |
| ---------------------------------------------- | ---------------------------------------------- | ---------------------------------------------- | ---------------------------------------------- |
| `managementExternalId`                         | *string*                                       | :heavy_check_mark:                             | Value to send as sts:ExternalId on AssumeRole. |