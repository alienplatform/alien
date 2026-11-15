# CreatePendingChangeRequest

Body of `POST /v1/projects/{project}/pending-changes`: the body of the
next signer list, for an admin to review and sign.

## Example Usage

```typescript
import { CreatePendingChangeRequest } from "@alienplatform/manager-api/models";

let value: CreatePendingChangeRequest = {
  basedOnVersion: 217411,
  body: {
    "key": "<value>",
  },
  summary: "<value>",
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `basedOnVersion`                                                                     | *number*                                                                             | :heavy_check_mark:                                                                   | Signer list version the body was prepared from.                                      |
| `body`                                                                               | Record<string, *any*>                                                                | :heavy_check_mark:                                                                   | The next list's body (`previous`, `adminThreshold`, `signers`,<br/>`deny`, `recovered`). |
| `summary`                                                                            | *string*                                                                             | :heavy_check_mark:                                                                   | One line for lists, such as `bob-thinkpad asks to: release`.                         |
