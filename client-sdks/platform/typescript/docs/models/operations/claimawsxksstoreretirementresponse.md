# ClaimAwsXksStoreRetirementResponse

Resumable cleanup claim, or null while the store still has keys

## Example Usage

```typescript
import { ClaimAwsXksStoreRetirementResponse } from "@alienplatform/platform-api/models/operations";

let value: ClaimAwsXksStoreRetirementResponse = {
  retirement: {
    customKeyStoreId: "<id>",
  },
};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `retirement`                                                   | [operations.Retirement](../../models/operations/retirement.md) | :heavy_check_mark:                                             | N/A                                                            |