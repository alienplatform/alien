# ListAwsKeySetupCleanupResponse

Keys awaiting final CloudFormation cleanup

## Example Usage

```typescript
import { ListAwsKeySetupCleanupResponse } from "@alienplatform/platform-api/models/operations";

let value: ListAwsKeySetupCleanupResponse = {
  items: [
    {
      stackId: "<id>",
      deletionDate: new Date("2026-12-01T21:51:04.202Z"),
    },
  ],
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `items`                                                                                          | [operations.ListAwsKeySetupCleanupItem](../../models/operations/listawskeysetupcleanupitem.md)[] | :heavy_check_mark:                                                                               | N/A                                                                                              |