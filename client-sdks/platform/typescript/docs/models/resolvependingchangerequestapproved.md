# ResolvePendingChangeRequestApproved

## Example Usage

```typescript
import { ResolvePendingChangeRequestApproved } from "@alienplatform/platform-api/models";

let value: ResolvePendingChangeRequestApproved = {
  status: "approved",
  signerListVersion: 172132,
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `status`                                               | *"approved"*                                           | :heavy_check_mark:                                     | N/A                                                    |
| `signerListVersion`                                    | *number*                                               | :heavy_check_mark:                                     | The stored signer list version that carries the change |