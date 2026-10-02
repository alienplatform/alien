# BundleSource

One OCI artifact and how to authenticate when pulling it.

## Example Usage

```typescript
import { BundleSource } from "@alienplatform/manager-api/models";

let value: BundleSource = {
  credentials: "caller",
  reference: "<value>",
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `credentials`                                              | [models.SourceCredentials](../models/sourcecredentials.md) | :heavy_check_mark:                                         | N/A                                                        |
| `reference`                                                | *string*                                                   | :heavy_check_mark:                                         | N/A                                                        |