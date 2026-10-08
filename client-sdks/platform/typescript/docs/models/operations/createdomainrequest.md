# CreateDomainRequest

## Example Usage

```typescript
import { CreateDomainRequest } from "@alienplatform/platform-api/models/operations";

let value: CreateDomainRequest = {
  domain: "shimmering-majority.org",
  setup: {
    deploymentUrlProjectId: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  },
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `domain`                                                                     | *string*                                                                     | :heavy_check_mark:                                                           | N/A                                                                          |
| `setup`                                                                      | [operations.CreateDomainSetup](../../models/operations/createdomainsetup.md) | :heavy_minus_sign:                                                           | N/A                                                                          |