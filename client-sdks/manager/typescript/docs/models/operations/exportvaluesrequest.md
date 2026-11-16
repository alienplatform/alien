# ExportValuesRequest

## Example Usage

```typescript
import { ExportValuesRequest } from "@alienplatform/manager-api/models/operations";

let value: ExportValuesRequest = {
  scope: "<value>",
  exportValuesRequest: {
    slots: [
      "<value 1>",
    ],
    targetToken: "<value>",
    targetUrl: "https://outlying-antelope.com/",
  },
};
```

## Fields

| Field                                                             | Type                                                              | Required                                                          | Description                                                       |
| ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| `scope`                                                           | *string*                                                          | :heavy_check_mark:                                                | dep:<deploymentId>                                                |
| `exportValuesRequest`                                             | [models.ExportValuesRequest](../../models/exportvaluesrequest.md) | :heavy_check_mark:                                                | N/A                                                               |
