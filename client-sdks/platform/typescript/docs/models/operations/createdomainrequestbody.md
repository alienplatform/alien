# CreateDomainRequestBody

## Example Usage

```typescript
import { CreateDomainRequestBody } from "@alienplatform/platform-api/models/operations";

let value: CreateDomainRequestBody = {
  domain: "definite-technologist.info",
  setup: {
    deploymentUrlProjectId: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  },
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `domain`                                             | *string*                                             | :heavy_check_mark:                                   | N/A                                                  |
| `setup`                                              | [operations.Setup](../../models/operations/setup.md) | :heavy_minus_sign:                                   | N/A                                                  |