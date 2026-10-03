# PromoteManagerReleaseRequest

## Example Usage

```typescript
import { PromoteManagerReleaseRequest } from "@alienplatform/manager-api/models/operations";

let value: PromoteManagerReleaseRequest = {
  id: "<id>",
  promoteReleaseRequest: {
    channel: "<value>",
  },
};
```

## Fields

| Field                                                                 | Type                                                                  | Required                                                              | Description                                                           |
| --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- |
| `id`                                                                  | *string*                                                              | :heavy_check_mark:                                                    | Release ID                                                            |
| `promoteReleaseRequest`                                               | [models.PromoteReleaseRequest](../../models/promotereleaserequest.md) | :heavy_check_mark:                                                    | N/A                                                                   |