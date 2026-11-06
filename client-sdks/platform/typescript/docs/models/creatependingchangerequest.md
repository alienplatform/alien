# CreatePendingChangeRequest

## Example Usage

```typescript
import { CreatePendingChangeRequest } from "@alienplatform/platform-api/models";

let value: CreatePendingChangeRequest = {
  summary: "<value>",
  basedOnVersion: 217411,
  body: {
    "key": "<value>",
  },
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `summary`                                                                        | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `basedOnVersion`                                                                 | *number*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `body`                                                                           | Record<string, *any*>                                                            | :heavy_check_mark:                                                               | Body of the next signer list: previous, adminThreshold, signers, deny, recovered |