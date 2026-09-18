# GetAccessRequestCommand

## Example Usage

```typescript
import { GetAccessRequestCommand } from "@alienplatform/platform-api/models/operations";

let value: GetAccessRequestCommand = {
  command: "kubernetes/get-pods",
  summary: "List pods in the ingestion namespace",
  params: {
    "pod": "ingester-p4kwm",
  },
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        | Example                                                                            |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `command`                                                                          | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                | kubernetes/get-pods                                                                |
| `summary`                                                                          | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                | List pods in the ingestion namespace                                               |
| `params`                                                                           | *any*                                                                              | :heavy_minus_sign:                                                                 | N/A                                                                                | {<br/>"pod": "ingester-p4kwm"<br/>}                                                |
| `tier`                                                                             | [operations.GetAccessRequestTier](../../models/operations/getaccessrequesttier.md) | :heavy_minus_sign:                                                                 | How risky an operation is (declared by the plugin metadata).                       |                                                                                    |