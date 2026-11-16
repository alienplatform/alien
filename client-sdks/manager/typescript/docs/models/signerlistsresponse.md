# SignerListsResponse

## Example Usage

```typescript
import { SignerListsResponse } from "@alienplatform/manager-api/models";

let value: SignerListsResponse = {
  items: [],
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `items`                                                  | [models.SignedStatement](../models/signedstatement.md)[] | :heavy_check_mark:                                       | Every stored version, oldest first.                      |
