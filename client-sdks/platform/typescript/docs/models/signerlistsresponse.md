# SignerListsResponse

## Example Usage

```typescript
import { SignerListsResponse } from "@alienplatform/platform-api/models";

let value: SignerListsResponse = {
  items: [],
  versions: [],
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `items`                                                                | [models.SignedStatement](../models/signedstatement.md)[]               | :heavy_check_mark:                                                     | Every stored version, oldest first                                     |
| `versions`                                                             | [models.Version](../models/version.md)[]                               | :heavy_check_mark:                                                     | Where each item came from, in the same order as `items`. Display only. |