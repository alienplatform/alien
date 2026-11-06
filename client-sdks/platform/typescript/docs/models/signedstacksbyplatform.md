# SignedStacksByPlatform

Signed `release/v2` statements by platform, from `alien release --sign`. Each must name this project and its platform and carry the stack sent for that platform. Stored and returned exactly as received; the API never verifies signatures.

## Example Usage

```typescript
import { SignedStacksByPlatform } from "@alienplatform/platform-api/models";

let value: SignedStacksByPlatform = {};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `aws`                                                  | [models.SignedStatement](../models/signedstatement.md) | :heavy_minus_sign:                                     | N/A                                                    |
| `gcp`                                                  | [models.SignedStatement](../models/signedstatement.md) | :heavy_minus_sign:                                     | N/A                                                    |
| `azure`                                                | [models.SignedStatement](../models/signedstatement.md) | :heavy_minus_sign:                                     | N/A                                                    |
| `kubernetes`                                           | [models.SignedStatement](../models/signedstatement.md) | :heavy_minus_sign:                                     | N/A                                                    |
| `machines`                                             | [models.SignedStatement](../models/signedstatement.md) | :heavy_minus_sign:                                     | N/A                                                    |
| `local`                                                | [models.SignedStatement](../models/signedstatement.md) | :heavy_minus_sign:                                     | N/A                                                    |
| `test`                                                 | [models.SignedStatement](../models/signedstatement.md) | :heavy_minus_sign:                                     | N/A                                                    |