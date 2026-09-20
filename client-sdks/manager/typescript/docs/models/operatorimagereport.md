# OperatorImageReport

Exact immutable Operator image identity observed by the running process.

## Example Usage

```typescript
import { OperatorImageReport } from "@alienplatform/manager-api/models";

let value: OperatorImageReport = {
  digest: "<value>",
  image: "https://picsum.photos/seed/F8IpwY/1265/2237",
  source: "package",
};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `digest`                                                       | *string*                                                       | :heavy_check_mark:                                             | Exact lowercase OCI digest in `sha256:digest` form.            |
| `image`                                                        | *string*                                                       | :heavy_check_mark:                                             | Exact OCI image reference in `repository@sha256:digest` form.  |
| `packageId`                                                    | *string*                                                       | :heavy_minus_sign:                                             | Package ID when `source` is `package`; otherwise `null`.       |
| `packageVersion`                                               | *string*                                                       | :heavy_minus_sign:                                             | Package version when `source` is `package`; otherwise `null`.  |
| `source`                                                       | [models.OperatorImageSource](../models/operatorimagesource.md) | :heavy_check_mark:                                             | Origin of the exact Operator image running this process.       |